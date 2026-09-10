// Copyright 2026 Salesforce, Inc. All rights reserved.
//! Data Contract Version Gate — MCP-native, inbound Omni/Flex Gateway policy.
//!
//! Reads a data-contract id + version from request headers on MCP `tools/call`,
//! and gates the declared version against per-contract (or global) block/deprecate
//! thresholds:
//!   * below blockedBelow   → reject (HTTP 426 Upgrade Required + upgrade hint);
//!   * below deprecatedBelow→ allow, but stamp Deprecation/Sunset response headers;
//!   * otherwise            → allow.
//! Stateless — a pure semver comparison against config. Version compatibility only
//! (NOT payload schema validation).

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

/// Carried from request → response leg: emit deprecation headers for this call?
#[derive(Clone, Default)]
struct Deprecation {
    emit: bool,
    from_version: String,
    upgrade_to: String,
    sunset: String,
}

fn upgrade_response(id: Value, current: &str, rules: &Rules) -> Response {
    let body = json!({
        "jsonrpc": "2.0",
        "id": id,
        "error": {
            "code": -32010,
            "message": format!(
                "data-contract version '{}' is not supported (must be >= {})",
                current, rules.blocked_below
            ),
            "data": {
                "current": current,
                "required": rules.blocked_below,
                "upgradeHint": format!("upgrade the data contract to >= {}", rules.blocked_below)
            }
        }
    })
    .to_string();
    Response::new(426)
        .with_headers(vec![("content-type".to_string(), "application/json".to_string())])
        .with_body(body.into_bytes())
}

fn resolve_for<'a>(config: &'a Config, contract_id: &str) -> Rules {
    let g_blocked = config.blocked_below.clone().unwrap_or_default();
    let g_dep = config.deprecated_below.clone().unwrap_or_default();
    if let Some(list) = &config.contracts {
        for c in list {
            if c.contract == contract_id {
                return resolve_rules(
                    &g_blocked,
                    &g_dep,
                    c.blocked_below.as_deref(),
                    c.deprecated_below.as_deref(),
                    c.sunset.as_deref(),
                );
            }
        }
    }
    resolve_rules(&g_blocked, &g_dep, None, None, None)
}

async fn request_filter(request_state: RequestState, config: Rc<Config>) -> Flow<Deprecation> {
    let headers_state = request_state.into_headers_state().await;

    if headers_state.method().as_str() != "POST" {
        return Flow::Continue(Deprecation::default());
    }
    match headers_state.handler().header("content-type") {
        Some(ct) if ct.starts_with("application/json") => {}
        _ => return Flow::Continue(Deprecation::default()),
    }

    let contract_header = config.contract_header.as_deref().unwrap_or("x-data-contract");
    let version_header = config.version_header.as_deref().unwrap_or("x-data-contract-version");
    let contract_id = headers_state.handler().header(contract_header).unwrap_or_default();
    let version = headers_state.handler().header(version_header);

    let body_state = headers_state.into_body_state().await;
    let body = body_state.handler().body();
    let req: Value = match serde_json::from_slice(&body) {
        Ok(v) => v,
        Err(_) => return Flow::Continue(Deprecation::default()),
    };
    if req.get("method").and_then(Value::as_str) != Some(TOOLS_CALL) {
        return Flow::Continue(Deprecation::default());
    }

    let rules = resolve_for(&config, &contract_id);
    let fail_closed = config.fail_mode.as_deref().unwrap_or("closed") != "open";
    let id = req.get("id").cloned().unwrap_or(Value::Null);

    match evaluate(&rules, version.as_deref()) {
        Decision::NotGoverned | Decision::Allow => Flow::Continue(Deprecation::default()),
        Decision::Deprecated => {
            let v = version.unwrap_or_default();
            logger::info!(
                "data-contract-gate: contract '{}' version '{}' deprecated (< {})",
                contract_id, v, rules.deprecated_below
            );
            Flow::Continue(Deprecation {
                emit: true,
                from_version: v,
                upgrade_to: rules.deprecated_below.clone(),
                sunset: rules.sunset.clone(),
            })
        }
        Decision::Blocked => {
            let v = version.unwrap_or_default();
            logger::warn!(
                "data-contract-gate: blocked contract '{}' version '{}' (< {})",
                contract_id, v, rules.blocked_below
            );
            Flow::Break(upgrade_response(id, &v, &rules))
        }
        Decision::Invalid => {
            if fail_closed {
                logger::warn!(
                    "data-contract-gate: missing/invalid version for governed contract '{}'",
                    contract_id
                );
                Flow::Break(upgrade_response(id, &version.unwrap_or_default(), &rules))
            } else {
                Flow::Continue(Deprecation::default())
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
    let h = headers_state.handler();
    // RFC 9745 Deprecation + advisory upgrade hint; RFC 8594 Sunset if configured.
    h.set_header("deprecation", "true");
    h.set_header(
        "x-data-contract-advice",
        &format!(
            "contract version {} is deprecated; upgrade to >= {}",
            dep.from_version, dep.upgrade_to
        ),
    );
    if !dep.sunset.trim().is_empty() {
        h.set_header("sunset", &dep.sunset);
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
