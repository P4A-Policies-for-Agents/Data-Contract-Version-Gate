# Data Contract Version Gate — MuleSoft Omni/Flex Gateway Policy

An **inbound** custom policy for the MuleSoft Omni/Flex Gateway that works on both
**MCP** and **REST/HTTP** APIs. It reads a **data-contract id + version** that a
caller declares on the request and gates the declared version against per-contract
(or global) **block / deprecate** thresholds:

- below `blockedBelow` → **reject** (HTTP **426 Upgrade Required** + machine-readable upgrade hint);
- below `deprecatedBelow` → **allow**, but stamp **`Deprecation`/`Sunset`** response headers;
- otherwise → **allow**.

Stateless — a pure semver comparison against config. Built with the PDK,
Rust → `wasm32-wasip1`, split-model. Attaches to `mcp`, `rest`, and `http`
instances.

> **Scope caveat — this is version *compatibility*, not schema validation.** It
> gates on the *declared* version; it does not validate the payload against the
> contract's schema (pair with a schema-validation policy for that).

---

## Where does the "data contract" live?

The full contract (an [ODCS](https://bitol.io) / [Data Contract Spec](https://datacontract.com)
artifact describing schema, SLAs, semantics, version) lives in **Git / a data
catalog / a contract registry** — *not* in this policy. The policy consumes only
(1) the **version the request declares** (a header) and (2) the **block/deprecate
thresholds** (policy config here, or an optional registry lookup).

---

## MCP and REST/HTTP — the `applyTo` mode

The contract id/version are **headers**, so the check is protocol-agnostic. The
only protocol-specific bit is *which* traffic to gate:

| `applyTo` | Behavior | Use on |
|---|---|---|
| `auto` (default) | Inspects the body: MCP JSON-RPC gates only `tools/call` (handshake passes); anything else is treated as a REST/HTTP request gated on the headers. | either |
| `mcp` | Gates only MCP `tools/call`. | MCP instances |
| `all` | Gates **every** request on the headers, without reading the body. | REST/HTTP instances |

Blocked responses adapt to the protocol: **JSON-RPC error envelope** for MCP,
**plain JSON** for REST/HTTP — both HTTP 426.

---

## How it decides

```
read contract id + version headers
resolve thresholds: per-contract entry overrides global fallback
  no thresholds        → NotGoverned → allow
  version missing/bad  → failMode closed → 426 ; open → allow
  version < blockedBelow    → BLOCK  (426 + { current, required, upgradeHint })
  version < deprecatedBelow → ALLOW  + Deprecation / Sunset / advice headers
  otherwise                 → ALLOW
```

Semver comparison is on the numeric core (`MAJOR.MINOR.PATCH`); a trailing
`-rc1` / `+build` is ignored.

---

## Configuration reference

| Property | Type | Default | Description |
|---|---|---|---|
| `contractHeader` | string | `x-data-contract` | Header carrying the contract id. |
| `versionHeader` | string | `x-data-contract-version` | Header carrying the declared semver. |
| `blockedBelow` | string | `""` | Global fallback: versions strictly below are blocked. Empty disables. |
| `deprecatedBelow` | string | `""` | Global fallback: versions below (but not blocked) are allowed + deprecated. |
| `contracts` | array | `[]` | Per-contract overrides: `{ contract, blockedBelow?, deprecatedBelow?, sunset? }`. |
| `applyTo` | `auto`\|`mcp`\|`all` | `auto` | Which traffic to gate (see table above). |
| `failMode` | `closed`\|`open` | `closed` | Missing/unparseable version for a governed contract → block, or allow+log. |

---

## Repository layout

```
data-contract-version-gate-definition/   # gcl.yaml (schema), exchange.json, Makefile
data-contract-version-gate-flex/          # Rust implementation
  src/lib.rs        # entrypoint + inbound filter (MCP + REST) + response header stamping
  src/semver.rs     # PURE semver core parse + compare — unit tested
  src/contract.rs   # PURE threshold resolve + decide — unit tested
  src/generated/    # config.rs generated from gcl.yaml
  tests/requests.rs # Docker-based integration test (make test)
demo/
  PROVISION.md          # exact anypoint-cli + A2D commands for both demos
  mcp/                  # MCP demo (A2D mock MCP server + config + demo.sh)
  api/                  # REST demo (A2D mock REST API + OAS + config + demo.sh)
```

---

## Build, test & release

```bash
cd data-contract-version-gate-definition && make release   # publish definition
cd ../data-contract-version-gate-flex
make build-asset-files
cargo build --target wasm32-wasip1 --release
cargo test --lib            # 10 pure unit tests (semver + gate decisions)
make release                # publish implementation
```

Published to Exchange at **1.0.1** (1.0.0 was MCP-only; 1.0.1 adds REST/HTTP +
`applyTo`).

---

## Live demos

Two demos, both fronted by a managed Flex Gateway with this policy applied.

### 1) MCP — `demo/mcp/`

A2D mock MCP server (`query_orders` tool), policy on the `mcp` instance.

```bash
cp demo/mcp/env.local.sh.example demo/mcp/env.local.sh   # set DCG_GW_URL (MCP endpoint)
./demo/mcp/demo.sh
```

| Declared version | Result |
|---|---|
| `3.1.0` | ✅ 200 (current) |
| `2.4.0` | ✅ 200 + `deprecation: true` / `sunset` / advice headers |
| `1.5.0` | ⛔ 426 + JSON-RPC error `{current, required, upgradeHint}` |
| missing | ⛔ 426 (fail-closed) |

### 2) REST/HTTP — `demo/api/`

A2D mock REST API (`GET /orders-api/orders`), policy on an `http` instance with
`applyTo: all`.

```bash
cp demo/api/env.local.sh.example demo/api/env.local.sh   # set DCG_API_URL (REST endpoint)
./demo/api/demo.sh
```

The gate runs on the request leg — **`426` == blocked by the gate**, any non-426
== the gate allowed it and forwarded upstream:

| Declared version | Gate verdict |
|---|---|
| `1.5.0` | ⛔ **426** + plain-JSON `{current, required, upgradeHint}` |
| missing | ⛔ **426** (fail-closed) |
| `2.4.0` | ✅ allowed + `deprecation: true` / `x-data-contract-advice` response headers |
| `3.1.0` | ✅ allowed (forwarded) |

> **Env note on the REST forward:** the A2D REST mock returns its **success (200)**
> body in a shape the managed Flex `http` proxy surfaces as a 500 on the *allowed*
> forward in this environment (its *error* responses pass through fine — which is
> how the `2.4.0` deprecation headers above are captured live). This is an
> A2D-mock/HTTP-proxy pass-through quirk, **not** the policy: the gate's decision
> (block 426 / allow / deprecate + headers) is what executes at the gateway and is
> demonstrated live. Against a normal HTTP upstream the allowed path returns the
> upstream's 200 (see the `make test` integration test and the MCP demo, where the
> forward succeeds).

Full provisioning for both in [`demo/PROVISION.md`](demo/PROVISION.md).

---

## Design notes & gotchas

- **Stateless & deterministic** — no shared storage, no per-replica caveat.
- **Deny = HTTP 426** with a machine-readable `upgradeHint`; `Deprecation`/`Sunset`
  (RFC 9745 / 8594) stamped on the response for deprecated-but-allowed versions
  (threaded request→response).
- **Protocol-adaptive block body** — JSON-RPC envelope for MCP, plain JSON for REST.
- **Fail-open** on non-JSON / non-governed; **fail-closed** (default) on a
  missing/unparseable version for a governed contract.
- **Optional extension:** fetch thresholds from a **contract registry** over HTTP
  (`format: service`, cached) instead of static config.

---

## Skills used

- **PDK** (`omni-gateway-pdk-skills`): `pdk-create-policy`, `pdk-mcp`,
  `pdk-schema-definition`, `pdk-request-headers-bodies`, `pdk-stop-execution`.
- **P4A** (`p4a-skills`): `p4a-build-policy`, `p4a-verify-requirements`,
  `p4a-mcp-usage`, `p4a-test-mcp-policies-with-a2d`.
