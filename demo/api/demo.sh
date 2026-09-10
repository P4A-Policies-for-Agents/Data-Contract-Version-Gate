#!/usr/bin/env bash
# Live demo (REST/HTTP): the SAME GET /orders-api/orders call, sent by consumers
# built against different data-contract versions, through a managed Flex Gateway
# HTTP instance with the Data Contract Version Gate policy applied (applyTo: all).
#
# The gate runs on the REQUEST leg:
#   * < blockedBelow  → 426 Upgrade Required + machine-readable hint (REST JSON) — BLOCKED here
#   * missing         → 426 (fail-closed)                                        — BLOCKED here
#   * >= deprecatedBelow / current → allowed → forwarded upstream                — ALLOWED (non-426)
# So "426" == the gate blocked the call; any non-426 == the gate allowed it and
# forwarded to the Orders API (the body is then whatever the upstream returns).
set -uo pipefail
DIR="$(cd "$(dirname "$0")" && pwd)"
[ -f "$DIR/env.local.sh" ] && . "$DIR/env.local.sh"
GW="${DCG_API_URL:?Set DCG_API_URL (see demo/api/env.local.sh.example)}"

probe() {
  local label="$1" ver="$2"
  local code dep adv
  code=$(curl -sS --max-time 25 -H 'x-data-contract: orders' ${ver:+-H "x-data-contract-version: $ver"} \
         -D /tmp/.dcgapi_h -o /tmp/.dcgapi_b -w "%{http_code}" "$GW")
  local verdict="ALLOWED (forwarded upstream)"; [ "$code" = "426" ] && verdict="BLOCKED by the gate"
  echo "── $label  (version=${ver:-<none>})  →  HTTP $code   [$verdict]"
  adv=$(grep -i '^x-data-contract-advice:' /tmp/.dcgapi_h | tr -d '\r'); [ -n "$adv" ] && echo "     $adv"
  dep=$(grep -i '^deprecation:' /tmp/.dcgapi_h | tr -d '\r'); [ -n "$dep" ] && echo "     $dep"
  echo "     body: $(head -c 150 /tmp/.dcgapi_b)"
  echo
}

echo "Data Contract Version Gate — REST API 'orders' (blockedBelow 2.0.0, deprecatedBelow 3.0.0)"
echo "GET \$DCG_API_URL  with header x-data-contract-version"
echo "==========================================================================================="
probe "CURRENT   (gate allows)"     "3.1.0"
probe "DEPRECATED(gate allows)"     "2.4.0"
probe "BLOCKED   (gate rejects)"    "1.5.0"
probe "MISSING   (fail-closed)"     ""
