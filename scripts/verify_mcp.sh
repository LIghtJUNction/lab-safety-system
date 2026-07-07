#!/bin/bash
set -euo pipefail

SCRATCH="/tmp/grok-goal-b3937704c092/implementer"
mkdir -p "$SCRATCH"

if [ -z "${TEST_DATABASE_URL:-}" ]; then
  echo "ERROR: TEST_DATABASE_URL is required"
  exit 1
fi

export DATABASE_URL="$TEST_DATABASE_URL"
export APP_HOST="127.0.0.1"
export APP_PORT="18081"
BASE="http://${APP_HOST}:${APP_PORT}"

echo "=== 1. cargo test mcp --include-ignored (drives dispatch tests + integration) ==="
cargo test mcp -- --include-ignored --nocapture 2>&1 | tee "$SCRATCH/test.log"

echo "=== 2. Build ==="
cargo build 2>&1 | tail -3

echo "=== 3. Start local server (background) ==="
cargo run > "$SCRATCH/server.log" 2>&1 &
SERVER_PID=$!
trap 'kill $SERVER_PID 2>/dev/null || true' EXIT

echo "Waiting for server on $BASE ..."
for i in {1..30}; do
  if curl -sf "$BASE/mcp" >/dev/null 2>&1; then break; fi
  sleep 0.5
done
curl -sf "$BASE/mcp" >/dev/null || (echo "server not ready"; cat "$SCRATCH/server.log" | tail -20; exit 1)

echo "=== 4. set -x curl sequence (config + all actions + gate) ==="
(
  set -x
  curl -sS -w '\nHTTPSTATUS:%{http_code}\n' "$BASE/mcp"
  curl -sS -X POST "$BASE/mcp" -H 'Content-Type: application/json' -d '{"enabled":true,"config":"{\"via\":\"verify-script\"}"}' -w '\nHTTPSTATUS:%{http_code}\n'
  curl -sS "$BASE/mcp" -w '\nHTTPSTATUS:%{http_code}\n'

  # happy paths + create to produce data
  curl -sS -X POST "$BASE/mcp/call" -H 'Content-Type: application/json' -d '{"action":"list_labs"}' -w '\nHTTPSTATUS:%{http_code}\n'
  curl -sS -X POST "$BASE/mcp/call" -H 'Content-Type: application/json' -d '{"action":"create_hazard","title":"script-hazard-'$(date +%s)'","lab_name":"script-lab","description":"from verify_mcp.sh"}' -w '\nHTTPSTATUS:%{http_code}\n'
  curl -sS -X POST "$BASE/mcp/call" -H 'Content-Type: application/json' -d '{"action":"list_hazards"}' -w '\nHTTPSTATUS:%{http_code}\n'

  # other actions (MCP style for one)
  curl -sS -X POST "$BASE/mcp/call" -H 'Content-Type: application/json' -d '{"action":"list_regulations"}' -w '\nHTTPSTATUS:%{http_code}\n'
  curl -sS -X POST "$BASE/mcp/call" -H 'Content-Type: application/json' -d '{"action":"list_documents"}' -w '\nHTTPSTATUS:%{http_code}\n'
  curl -sS -X POST "$BASE/mcp/call" -H 'Content-Type: application/json' -d '{"action":"list_equipment"}' -w '\nHTTPSTATUS:%{http_code}\n'
  curl -sS -X POST "$BASE/mcp/call" -H 'Content-Type: application/json' -d '{"action":"list_operations"}' -w '\nHTTPSTATUS:%{http_code}\n'
  curl -sS -X POST "$BASE/mcp/call" -H 'Content-Type: application/json' -d '{"action":"list_incidents"}' -w '\nHTTPSTATUS:%{http_code}\n'
  curl -sS -X POST "$BASE/mcp/call" -H 'Content-Type: application/json' -d '{"tool":"lab_safety","arguments":{"action":"list_hazards"}}' -w '\nHTTPSTATUS:%{http_code}\n'

  # gate test
  curl -sS -X POST "$BASE/mcp" -H 'Content-Type: application/json' -d '{"enabled":false}' -w '\nHTTPSTATUS:%{http_code}\n'
  curl -sS "$BASE/mcp" -w '\nHTTPSTATUS:%{http_code}\n'
  curl -sS -X POST "$BASE/mcp/call" -H 'Content-Type: application/json' -d '{"action":"list_hazards"}' -w '\nHTTPSTATUS:%{http_code}\n' || echo "EXPECTED 400 ABOVE"
  curl -sS -X POST "$BASE/mcp" -H 'Content-Type: application/json' -d '{"enabled":true}' -w '\nHTTPSTATUS:%{http_code}\n'
  curl -sS -X POST "$BASE/mcp/call" -H 'Content-Type: application/json' -d '{"action":"list_hazards"}' -w '\nHTTPSTATUS:%{http_code}\n'
) 2>&1 | tee "$SCRATCH/mcp-calls.log"

echo "=== 5. Done. Evidence in $SCRATCH ==="
ls -l "$SCRATCH"/test.log "$SCRATCH"/mcp-calls.log
