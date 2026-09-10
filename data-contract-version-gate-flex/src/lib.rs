// Copyright 2026 Salesforce, Inc. All rights reserved.
//! Data Contract Version Gate — inbound Omni/Flex Gateway policy for MCP and REST/HTTP.
//!
//! Reads a data-contract id + version from request headers and gates the declared
//! version against per-contract (or global) block/deprecate thresholds:
//!   * below blockedBelow    → reject (HTTP 426 Upgrade Required + upgrade hint);
//!   * below deprecatedBelow → allow + Deprecation/Sunset response headers;
//!   * otherwise             → allow.
//!
//! Works on both MCP `tools/call` and plain REST/HTTP APIs (see `applyTo`).
//! Stateless — a pure semver comparison against config; version compatibility
//! only (NOT payload schema validation).

mod contract;
mod generated;
mod semver;

use std::rc::Rc;

use anyhow::{anyhow, Result};
use pdk::hl::*;
use pdk::logger;
use serde_json::{json, Value};

use crate::contract::{evaluate, resolve_rules, Decision, Rules};
use crate::generated::config::Config;

const TOOLS_CALL: &str = "tools/call";

/// Carried request → response: emit deprecation headers for this call?
#[derive(Clone, Default)]
struct Deprecation {
    emit: bool,
    from_version: String,
    upgrade_to: String,
    sunset: String,
}

/// Block response. MCP → JSON-RPC error envelope; REST/HTTP → plain JSON error.
fn block_response(is_mcp: bool, id: Value, current: &str, rules: &Rules) -> Response {
    let hint = format!("upgrade the data contract to >= {}", rules.blocked_below);
    let msg = format!(
        "data-contract version '{}' is not supported (must be >= {})",
        current, rules.blocked_below
    );
    let body = if is_mcp {
        json!({
            "jsonrpc": "2.0", "id": id,
            "error": { "code": -32010, "message": msg,
                "data": { "current": current, "required": rules.blocked_below, "upgradeHint": hint } }
        })
    } else {
        json!({
            "error": "unsupported_data_contract_version",
            "message": msg,
            "current": current, "required": rules.blocked_below, "upgradeHint": hint
        })
    }
    .to_string();
    Response::new(426)
        .with_headers(vec![("content-type".to_string(), "application/json".to_string())])
        .with_body(body.into_bytes())
}

fn resolve_for(config: &Config, contract_id: &str) -> Rules {
    let g_blocked = config.blocked_below.clone().unwrap_or_default();
    let g_dep = config.deprecated_below.clone().unwrap_or_default();
    if let Some(list) = &config.contracts {
        for c in list {
            if c.contract == contract_id {
                return resolve_rules(
                    &g_blocked, &g_dep,
                    c.blocked_below.as_deref(), c.deprecated_below.as_deref(), c.sunset.as_deref(),
                );
            }
        }
    }
    resolve_rules(&g_blocked, &g_dep, None, None, None)
}

/// Pure decision → Flow (independent of MCP vs REST, except the block body shape).
fn decide(
    rules: &Rules,
    version: Option<&str>,
    is_mcp: bool,
    id: Value,
    fail_closed: bool,
    contract_id: &str,
) -> Flow<Deprecation> {
    match evaluate(rules, version) {
        Decision::NotGoverned | Decision::Allow => Flow::Continue(Deprecation::default()),
        Decision::Deprecated => {
            let v = version.unwrap_or_default().to_string();
            logger::info!("data-contract-gate: contract '{contract_id}' version '{v}' deprecated");
            Flow::Continue(Deprecation {
                emit: true,
                from_version: v,
                upgrade_to: rules.deprecated_below.clone(),
                sunset: rules.sunset.clone(),
            })
        }
        Decision::Blocked => {
            let v = version.unwrap_or_default();
            logger::warn!("data-contract-gate: blocked contract '{contract_id}' version '{v}'");
            Flow::Break(block_response(is_mcp, id, v, rules))
        }
        Decision::Invalid => {
            if fail_closed {
                logger::warn!("data-contract-gate: missing/invalid version for '{contract_id}'");
                Flow::Break(block_response(is_mcp, id, version.unwrap_or_default(), rules))
            } else {
                Flow::Continue(Deprecation::default())
            }
        }
    }
}

async fn request_filter(request_state: RequestState, config: Rc<Config>) -> Flow<Deprecation> {
    let headers_state = request_state.into_headers_state().await;
    let h = headers_state.handler();

    let contract_header = config.contract_header.as_deref().unwrap_or("x-data-contract");
    let version_header = config.version_header.as_deref().unwrap_or("x-data-contract-version");
    let contract_id = h.header(contract_header).unwrap_or_default();
    let version = h.header(version_header);
    let is_post_json = headers_state.method().as_str() == "POST"
        && h.header("content-type").map(|c| c.starts_with("application/json")).unwrap_or(false);

    let rules = resolve_for(&config, &contract_id);
    let fail_closed = config.fail_mode.as_deref().unwrap_or("closed") != "open";
    let mode = config.apply_to.as_deref().unwrap_or("auto");

    // Fast exits that need no body: nothing configured, or header-only modes.
    if mode == "all" {
        return decide(&rules, version.as_deref(), false, Value::Null, fail_closed, &contract_id);
    }
    if !is_post_json {
        // Not an MCP JSON-RPC POST. In 'mcp' mode we only gate tools/call → pass.
        // In 'auto' mode treat it as a REST/HTTP request gated on headers.
        return if mode == "mcp" {
            Flow::Continue(Deprecation::default())
        } else {
            decide(&rules, version.as_deref(), false, Value::Null, fail_closed, &contract_id)
        };
    }

    // POST + JSON: could be MCP JSON-RPC. Read the body to classify.
    let body_state = headers_state.into_body_state().await;
    let body = body_state.handler().body();
    let parsed: Option<Value> = serde_json::from_slice(&body).ok();

    match parsed {
        Some(v) if v.get("jsonrpc").is_some() || v.get("method").is_some() => {
            // MCP JSON-RPC: gate only tools/call; let handshake/other pass.
            if v.get("method").and_then(Value::as_str) == Some(TOOLS_CALL) {
                let id = v.get("id").cloned().unwrap_or(Value::Null);
                decide(&rules, version.as_deref(), true, id, fail_closed, &contract_id)
            } else {
                Flow::Continue(Deprecation::default())
            }
        }
        _ => {
            // Non-JSON-RPC JSON body (a REST/HTTP request). 'mcp' mode ignores it.
            if mode == "mcp" {
                Flow::Continue(Deprecation::default())
            } else {
                decide(&rules, version.as_deref(), false, Value::Null, fail_closed, &contract_id)
            }
        }
    }
}

async fn response_filter(response_state: ResponseState, request_data: RequestData<Deprecation>) {
    let dep = match request_data {
        RequestData::Continue(d) if d.emit => d,
        _ => return,
    };
    let headers_state = response_state.into_headers_state().await;
    let hh = headers_state.handler();
    hh.set_header("deprecation", "true");
    hh.set_header(
        "x-data-contract-advice",
        &format!("contract version {} is deprecated; upgrade to >= {}", dep.from_version, dep.upgrade_to),
    );
    if !dep.sunset.trim().is_empty() {
        hh.set_header("sunset", &dep.sunset);
    }
}

#[entrypoint]
async fn configure(launcher: Launcher, Configuration(bytes): Configuration) -> Result<()> {
    let config: Config = serde_json::from_slice(&bytes).map_err(|err| {
        anyhow!(
            "Failed to parse configuration '{}'. Cause: {}",
            String::from_utf8_lossy(&bytes),
            err
        )
    })?;
    let config = Rc::new(config);
    let filter = on_request(move |rs| {
        let config = config.clone();
        async move { request_filter(rs, config).await }
    })
    .on_response(response_filter);

    launcher.launch(filter).await?;
    Ok(())
}
