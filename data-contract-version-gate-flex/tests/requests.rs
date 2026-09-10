// Copyright 2026 Salesforce, Inc. All rights reserved.
//
// Integration test (requires Docker; run with `make test`). Asserts the version
// gate: a current version passes, a blocked version is rejected (HTTP 426). The
// pure semver/decision logic is covered by `cargo test --lib`.

mod common;

use httpmock::MockServer;
use pdk_test::port::Port;
use pdk_test::services::flex::{ApiConfig, Flex, FlexConfig, PolicyConfig};
use pdk_test::services::httpmock::{HttpMock, HttpMockConfig};
use pdk_test::{pdk_test, TestComposite};

use common::*;

const FLEX_PORT: Port = 8081;

#[pdk_test]
async fn gates_contract_version() -> anyhow::Result<()> {
    let httpmock_config = HttpMockConfig::builder()
        .port(80)
        .version("latest")
        .hostname("backend")
        .build();

    let policy_config = PolicyConfig::builder()
        .name(POLICY_NAME)
        .configuration(serde_json::json!({
            "contracts": [
                { "contract": "orders", "blockedBelow": "2.0.0", "deprecatedBelow": "3.0.0" }
            ],
            "failMode": "closed"
        }))
        .build();

    let api_config = ApiConfig::builder()
        .name("myApi")
        .upstream(&httpmock_config)
        .path("/")
        .port(FLEX_PORT)
        .policies([policy_config])
        .build();

    let flex_config = FlexConfig::builder()
        .version("1.10.0")
        .hostname("local-flex")
        .with_api(api_config)
        .config_mounts([(POLICY_DIR, "policy"), (COMMON_CONFIG_DIR, "common")])
        .build();

    let composite = TestComposite::builder()
        .with_service(flex_config)
        .with_service(httpmock_config)
        .build()
        .await?;

    let flex: Flex = composite.service()?;
    let flex_url = flex.external_url(FLEX_PORT).unwrap();
    let httpmock: HttpMock = composite.service()?;
    let mock_server = MockServer::connect_async(httpmock.socket()).await;

    mock_server
        .mock_async(|when, then| {
            when.method(httpmock::Method::POST);
            then.status(200)
                .header("content-type", "application/json")
                .body(r#"{"jsonrpc":"2.0","id":2,"result":{"structuredContent":{"orderCount":3}}}"#);
        })
        .await;

    let client = reqwest::Client::new();
    let call = r#"{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"query_orders","arguments":{"customerId":"C-1"}}}"#;

    let send = |ver: &str| {
        let client = client.clone();
        let url = flex_url.clone();
        let ver = ver.to_string();
        async move {
            client
                .post(format!("{url}/"))
                .header("content-type", "application/json")
                .header("x-data-contract", "orders")
                .header("x-data-contract-version", ver)
                .body(call)
                .send()
                .await
                .map(|r| r.status().as_u16())
        }
    };

    assert_eq!(send("3.1.0").await?, 200); // current -> allowed
    assert_eq!(send("1.5.0").await?, 426); // below blockedBelow -> rejected

    Ok(())
}
