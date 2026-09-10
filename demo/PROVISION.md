# Demo provisioning runbook (MCP + REST)

Both demos use `anypoint-cli-v4` (already authenticated) + the A2D MCP tools —
**no bearer token / connected-app secret needed**. Replace `<...>` placeholders
with your own (identifiers, not secrets).

| Placeholder | What it is |
|---|---|
| `<orgId>` | Business-group / org id (`account:business-group:list`) |
| `<gatewayId>` | Managed Flex Gateway **resource** id (`runtime-mgr:gateways:managed:list`) |
| `<gatewayPublicHost>` | Gateway public ingress (`…:describe <gatewayId>` → `configuration.ingress.publicUrl`) |
| `<mcpServerId>` / `<restApiId>` | A2D mock ids returned by `design_mcp_server` / `design_rest_api` |

Policy version: **1.0.1** (adds REST/HTTP + `applyTo`).

---

## Demo 1 — MCP

1. A2D: `design_mcp_server` + `add_mcp_tool query_orders` (returns sample orders).
2. Publish `type=mcp` asset:
   ```bash
   anypoint-cli-v4 exchange:asset:upload --name "Data Contract Gate Test Server" \
     --type mcp --status published --properties='{"platform":"a2d"}' \
     --files='{"mcp-metadata.json":"./mcp/mcp-metadata.json"}' data-contract-gate-test-server/1.0.0
   ```
3. Create + deploy the MCP Flex instance (upstream = mock surface minus `/mcp`):
   ```bash
   anypoint-cli-v4 api-mgr:api:manage data-contract-gate-test-server 1.0.0 <orgId> \
     --environment Sandbox --isFlex --type mcp \
     --uri "https://www.a2d-ai.com/api/platform/<mcpServerId>/" --apiInstanceLabel data-contract-gate-demo
   anypoint-cli-v4 api-mgr:api:edit <id> --environment Sandbox --isFlex --type mcp \
     --withProxy --scheme http --port 8081 --path "/data-contract-gate-demo/" \
     --uri "https://www.a2d-ai.com/api/platform/<mcpServerId>/"
   anypoint-cli-v4 api-mgr:api:deploy <id> --environment Sandbox --target <gatewayId> --gatewayVersion 1.0.0 --overwrite
   ```
4. Apply the policy (default `applyTo: auto` gates `tools/call`):
   ```bash
   anypoint-cli-v4 api-mgr:policy:apply <id> data-contract-version-gate \
     --environment Sandbox --groupId <orgId> --policyVersion 1.0.1 --configFile ./mcp/config.json
   anypoint-cli-v4 api-mgr:api:redeploy <id> --environment Sandbox
   ```
5. `cp mcp/env.local.sh.example mcp/env.local.sh` (set `DCG_GW_URL`) then `./mcp/demo.sh`.

---

## Demo 2 — REST / HTTP

1. A2D: `design_rest_api` (base_path `/orders-api`) + `add_rest_endpoint GET /orders`
   with a **catch-all mock scenario** (`conditions: []`). The live mock URL comes
   from the OpenAPI `servers[0].url`: `…/api/platform/<restApiId>/api` + `/orders-api/orders`.
2. Publish the OpenAPI as a `rest-api` asset (note the required `apiVersion` property):
   ```bash
   anypoint-cli-v4 exchange:asset:upload --name "Orders Data Product API" \
     --type rest-api --status published \
     --properties='{"mainFile":"oas.json","apiVersion":"v1"}' \
     --files='{"oas.json":"./api/oas.json"}' orders-data-product-api/1.0.0
   ```
3. Create + deploy the HTTP Flex instance (upstream = mock base ending `/api/`):
   ```bash
   anypoint-cli-v4 api-mgr:api:manage orders-data-product-api 1.0.0 <orgId> \
     --environment Sandbox --isFlex --type http \
     --uri "https://www.a2d-ai.com/api/platform/<restApiId>/api/" --apiInstanceLabel orders-gate-demo
   anypoint-cli-v4 api-mgr:api:edit <id> --environment Sandbox --isFlex --type http \
     --withProxy --scheme http --port 8081 --path "/orders-gate-demo/" \
     --uri "https://www.a2d-ai.com/api/platform/<restApiId>/api/"
   anypoint-cli-v4 api-mgr:api:deploy <id> --environment Sandbox --target <gatewayId> --gatewayVersion 1.0.0 --overwrite
   ```
4. Apply the policy in header-only mode (`applyTo: all`):
   ```bash
   anypoint-cli-v4 api-mgr:policy:apply <id> data-contract-version-gate \
     --environment Sandbox --groupId <orgId> --policyVersion 1.0.1 --configFile ./api/config.json
   anypoint-cli-v4 api-mgr:api:redeploy <id> --environment Sandbox
   ```
5. `cp api/env.local.sh.example api/env.local.sh` (set `DCG_API_URL` to the governed
   `…/orders-gate-demo/orders-api/orders`) then `./api/demo.sh`.

---

## Notes / gotchas

- **A2D REST mocks:** served at `…/api/platform/<restApiId>/api<base_path><path>`
  (the extra `/api` segment is in the OpenAPI `servers` url); need a **catch-all
  scenario** (`conditions: []`) to match, and `apiVersion` when publishing the OAS.
- **HTTP proxy path mapping:** upstream must be the mock **base** (`…/api/`); the
  gateway strips the proxy base path and appends the remainder. A full-URL upstream
  double-paths.
- **Deploy target** is the gateway **resource** id; `api:manage` alone leaves
  `deployment: null` until `api:edit --withProxy` + `api:deploy`.
- **REST allowed-forward quirk:** this A2D HTTPS mock's success (200) body is
  surfaced as a 500 by the managed `http` proxy on the allowed forward (its error
  responses pass through — which is how the `2.4.0` deprecation headers show live).
  It's an A2D-mock pass-through quirk, not the policy — the gate's 426/allow/deprecate
  decision executes at the gateway regardless.
