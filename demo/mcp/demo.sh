#!/usr/bin/env bash
# Live demo: same query_orders tools/call, sent by consumers built against
# different data-contract versions. The Data Contract Version Gate policy allows
# current versions, flags deprecated ones with Deprecation/Sunset headers, and
# blocks unsupported ones (HTTP 426 + upgrade hint).
set -uo pipefail
DIR="$(cd "$(dirname "$0")" && pwd)"
[ -f "$DIR/env.local.sh" ] && . "$DIR/env.local.sh"
GW="${DCG_GW_URL:?Set DCG_GW_URL (see demo/env.local.sh.example)}"

HDR=(-H 'Content-Type: application/json' -H 'Accept: application/json, text/event-stream' -H 'Accept-Encoding: identity' -H 'x-data-contract: orders')
CALL='{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"query_orders","arguments":{"customerId":"C-1"}}}'

probe() {
  local label="$1" ver="$2"
  local code dep sun adv body
  code=$(curl -sS --max-time 25 "${HDR[@]}" ${ver:+-H "x-data-contract-version: $ver"} \
         -D /tmp/.dcg_h -o /tmp/.dcg_b -w "%{http_code}" -X POST "$GW" -d "$CALL")
  echo "── $label  (version=${ver:-<none>})  →  HTTP $code"
  dep=$(grep -i '^deprecation:' /tmp/.dcg_h | tr -d '\r'); [ -n "$dep" ] && echo "     $dep"
  sun=$(grep -i '^sunset:' /tmp/.dcg_h | tr -d '\r'); [ -n "$sun" ] && echo "     $sun"
  adv=$(grep -i '^x-data-contract-advice:' /tmp/.dcg_h | tr -d '\r'); [ -n "$adv" ] && echo "     $adv"
  body=$(sed -n 's/^data: //p' /tmp/.dcg_b | head -1); [ -z "$body" ] && body=$(head -c 200 /tmp/.dcg_b)
  echo "     body: ${body:0:140}"
  echo
}

echo "Data Contract Version Gate — contract 'orders' (blockedBelow 2.0.0, deprecatedBelow 3.0.0)"
echo "=========================================================================================="
probe "CURRENT   (allow)"            "3.1.0"
probe "DEPRECATED(allow + headers)"  "2.4.0"
probe "BLOCKED   (426 upgrade)"      "1.5.0"
probe "MISSING   (fail-closed 426)"  ""
