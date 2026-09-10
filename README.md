# Data Contract Version Gate — MuleSoft Omni/Flex Gateway Policy

An **MCP-native, inbound** custom policy for the MuleSoft Omni/Flex Gateway. It
reads a **data-contract id + version** that a caller declares on the request and
gates the declared version against per-contract (or global) **block / deprecate**
thresholds:

- below `blockedBelow` → **reject** (HTTP **426 Upgrade Required** + machine-readable upgrade hint);
- below `deprecatedBelow` → **allow**, but stamp **`Deprecation`/`Sunset`** response headers;
- otherwise → **allow**.

Stateless — a pure semver comparison against policy config. Built with the PDK,
Rust → `wasm32-wasip1`, split-model.

> **Scope caveat — this is version *compatibility*, not schema validation.** It
> answers "is this declared contract version still supported / deprecated /
> blocked?" It does **not** validate the payload against the contract's schema
> (pair it with a schema-validation policy for that). And it gates on the
> *declared* version — position the gateway as the choke point so callers can't
> bypass declaring it.

---

## Where does the "data contract" live?

The full contract (an [ODCS](https://bitol.io) / [Data Contract Spec](https://datacontract.com)
YAML describing schema, SLAs, semantics, version) lives in **Git / a data catalog
/ a contract registry** — *not* in this policy. The policy only consumes:

1. **the version the request declares** — a header (`x-data-contract` +
   `x-data-contract-version`); and
2. **the block/deprecate thresholds** — from **policy config** (used here), or, as
   an optional extension, fetched from a **contract registry** over HTTP.

So the contract is a file elsewhere; the gate enforces which of its versions are
still acceptable.

---

## How it decides

For each MCP `tools/call`:

```
read x-data-contract (id) and x-data-contract-version (semver) from headers
resolve thresholds: per-contract entry overrides global fallback
  no thresholds        → NotGoverned → allow
  version missing/bad  → failMode closed → 426 ; open → allow
  version < blockedBelow    → BLOCK  (HTTP 426 + { current, required, upgradeHint })
  version < deprecatedBelow → ALLOW  + Deprecation / Sunset / advice headers
  otherwise                 → ALLOW
```

Non-`tools/call` methods pass through. Semver comparison is on the numeric core
(`MAJOR.MINOR.PATCH`); a trailing `-rc1` / `+build` is ignored.

Blocked response body:

```json
{ "jsonrpc":"2.0","id":2,"error":{ "code":-32010,
  "message":"data-contract version '1.5.0' is not supported (must be >= 2.0.0)",
  "data":{ "current":"1.5.0","required":"2.0.0","upgradeHint":"upgrade the data contract to >= 2.0.0" } } }
```

---

## Configuration reference

| Property | Type | Default | Description |
|---|---|---|---|
| `contractHeader` | string | `x-data-contract` | Header carrying the contract id. |
| `versionHeader` | string | `x-data-contract-version` | Header carrying the declared semver. |
| `blockedBelow` | string | `""` | Global fallback: versions strictly below are blocked. Empty disables. |
| `deprecatedBelow` | string | `""` | Global fallback: versions below (but not blocked) are allowed + deprecated. |
| `contracts` | array | `[]` | Per-contract overrides: `{ contract, blockedBelow?, deprecatedBelow?, sunset? }`. |
| `failMode` | `closed` \| `open` | `closed` | Missing/unparseable version for a governed contract → block, or allow+log. |

### Example config

```json
{
  "contracts": [
    { "contract": "orders", "blockedBelow": "2.0.0", "deprecatedBelow": "3.0.0",
      "sunset": "Wed, 01 Jan 2027 00:00:00 GMT" }
  ],
  "failMode": "closed"
}
```

---

## Repository layout

```
data-contract-version-gate-definition/   # gcl.yaml (schema), exchange.json, Makefile
data-contract-version-gate-flex/          # Rust implementation
  src/lib.rs        # entrypoint + inbound request filter + response header stamping
  src/semver.rs     # PURE semver core parse + compare — unit tested
  src/contract.rs   # PURE threshold resolve + decide (allow/deprecate/block) — unit tested
  src/generated/    # config.rs generated from gcl.yaml
  tests/requests.rs # Docker-based integration test (make test)
demo/
  mcp-metadata.json     # MCP manifest published to Exchange as type=mcp
  config.json           # policy config applied in the demo (contract 'orders')
  demo.sh               # live 4-case demo (current / deprecated / blocked / missing)
  env.local.sh.example  # copy to env.local.sh (gitignored) with your endpoint
  PROVISION.md          # exact anypoint-cli + A2D commands to reproduce
```

---

## Build, test & release

```bash
cd data-contract-version-gate-definition && make release   # publish definition
cd ../data-contract-version-gate-flex
make build-asset-files
cargo build --target wasm32-wasip1 --release
cargo test --lib            # 10 pure unit tests (semver parse/compare + gate decisions)
make release                # publish implementation
```

---

## Live demo

A mock Orders data-product MCP server (`query_orders`) fronted by a managed Flex
Gateway with this policy applied (contract `orders`: `blockedBelow 2.0.0`,
`deprecatedBelow 3.0.0`).

```bash
cp demo/env.local.sh.example demo/env.local.sh   # set DCG_GW_URL to your governed endpoint
./demo/demo.sh
```

Observed live output:

```
── CURRENT    (version=3.1.0)  →  HTTP 200
── DEPRECATED (version=2.4.0)  →  HTTP 200
     deprecation: true
     sunset: Wed, 01 Jan 2027 00:00:00 GMT
     x-data-contract-advice: contract version 2.4.0 is deprecated; upgrade to >= 3.0.0
── BLOCKED    (version=1.5.0)  →  HTTP 426   {current:1.5.0, required:2.0.0, upgradeHint:…}
── MISSING    (version=<none>) →  HTTP 426   (fail-closed)
```

Every case is deterministic (stateless semver check). Full provisioning in
[`demo/PROVISION.md`](demo/PROVISION.md).

---

## Design notes & gotchas

- **Stateless & deterministic** — no shared storage, no per-replica caveat; the
  decision is a pure function of the header + config.
- **Deny = HTTP 426 Upgrade Required** (semantically apt) with a machine-readable
  `data.upgradeHint`; `Deprecation`/`Sunset` (RFC 9745 / 8594) are stamped on the
  *response* for deprecated-but-allowed versions (threaded request→response).
- **Fail-open** on non-JSON / non-`tools/call`; **fail-closed** (default) on a
  missing/unparseable version for a governed contract.
- **Version compatibility only** — not payload schema validation (separate policy).
- **Optional extension:** fetch thresholds from a **contract registry** over HTTP
  (`format: service`, cached) instead of static config; and **MCP-only** today
  (`assetTypes: mcp`) — the `semver`/`contract` cores are protocol-agnostic, so a
  REST/HTTP variant (gate on route + header) is a contained addition.

---

## Skills used

- **PDK** (`omni-gateway-pdk-skills`): `pdk-create-policy`, `pdk-mcp`,
  `pdk-schema-definition`, `pdk-request-headers-bodies`, `pdk-stop-execution`.
- **P4A** (`p4a-skills`): `p4a-build-policy`, `p4a-verify-requirements`,
  `p4a-mcp-usage`, `p4a-test-mcp-policies-with-a2d`.
