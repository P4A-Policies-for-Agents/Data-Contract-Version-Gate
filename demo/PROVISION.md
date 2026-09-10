# Demo provisioning runbook

Steps to stand up the live Data Contract Version Gate demo, following
`p4a-test-mcp-policies-with-a2d`. Done with `anypoint-cli-v4` (already
authenticated) + the A2D MCP tools — **no bearer token / connected-app secret
needed**. Replace `<...>` placeholders with your own (identifiers, not secrets).

| Placeholder | What it is | How to get it |
|---|---|---|
| `<orgId>` | Business-group / org id | `anypoint-cli-v4 account:business-group:list` |
| `<mockServerId>` | A2D mock MCP server id | returned by `design_mcp_server` |
| `<gatewayId>` | Managed Flex Gateway **resource** id | `runtime-mgr:gateways:managed:list --environment Sandbox` |
| `<gatewayPublicHost>` | Gateway public ingress host | `runtime-mgr:gateways:managed:describe <gatewayId>` → `configuration.ingress.publicUrl` |
| `<apiInstanceId>` | API Manager instance id | returned by `api-mgr:api:manage` |

Mock surface: `https://www.a2d-ai.com/api/platform/<mockServerId>/mcp`
Governed endpoint: `https://<gatewayPublicHost>/data-contract-gate-demo/mcp`

## 1. A2D mock (A2D MCP tools)

`design_mcp_server` (type `mock`, provider org + URL) → `add_mcp_tool`
`query_orders` (schemas meet A2D's quality gate) returning
`{"orderCount":3,"latestOrderId":"ORD-90210"}`.

## 2. Publish the manifest to Exchange as `type=mcp`

```bash
anypoint-cli-v4 exchange:asset:upload \
  --name "Data Contract Gate Test Server" --type mcp --status published \
  --description "Mock Orders data-product MCP server for the Data Contract Version Gate demo" \
  --properties='{"platform":"a2d"}' \
  --files='{"mcp-metadata.json":"./mcp-metadata.json"}' \
  data-contract-gate-test-server/1.0.0
```

## 3. Create + deploy the MCP Flex API instance

```bash
anypoint-cli-v4 api-mgr:api:manage data-contract-gate-test-server 1.0.0 <orgId> \
  --environment Sandbox --isFlex --type mcp \
  --uri "https://www.a2d-ai.com/api/platform/<mockServerId>/" \
  --apiInstanceLabel "data-contract-gate-demo"

anypoint-cli-v4 api-mgr:api:edit <apiInstanceId> --environment Sandbox --isFlex --type mcp \
  --withProxy --scheme http --port 8081 --path "/data-contract-gate-demo/" \
  --uri "https://www.a2d-ai.com/api/platform/<mockServerId>/"

# target = the GATEWAY resource id (not its targetId)
anypoint-cli-v4 api-mgr:api:deploy <apiInstanceId> --environment Sandbox \
  --target <gatewayId> --gatewayVersion 1.0.0 --overwrite
```

## 4. Apply the policy

```bash
anypoint-cli-v4 api-mgr:policy:apply <apiInstanceId> data-contract-version-gate \
  --environment Sandbox --groupId <orgId> \
  --policyVersion 1.0.0 --configFile ./config.json
anypoint-cli-v4 api-mgr:api:redeploy <apiInstanceId> --environment Sandbox
```

## 5. Run the demo

```bash
cp env.local.sh.example env.local.sh   # set DCG_GW_URL to your governed endpoint
./demo.sh
```

Expected: `3.1.0` → 200; `2.4.0` → 200 + Deprecation/Sunset headers; `1.5.0` → 426
with upgrade hint; missing version → 426 (fail-closed).

## Notes

- **Inbound, request-gating + a light response-header stamp** (Deprecation/Sunset);
  no body rewrite → **MCP Support not required**.
- **Stateless** — deterministic, no per-replica caveat.
- Upstream URI is the mock surface **minus** `/mcp`; deploy target is the gateway
  **resource** id; `api:manage` alone leaves `deployment: null` until
  `api:edit --withProxy` + `api:deploy`.
