#!/usr/bin/env bash
# Isolated regression checks. No real Docker daemon or network is used.
set -euo pipefail
SCRIPT=$(cd "$(dirname "$0")/.." && pwd)/deploy-quickstart.sh
WORK=$(mktemp -d)
trap 'rm -rf "$WORK"' EXIT
mkdir -p "$WORK/bin" "$WORK/run"
touch "$WORK/run/docker-compose.integrated.yml"
cat > "$WORK/bin/docker" <<'STUB'
#!/usr/bin/env bash
printf '%s\n' "$*" >> "$CALL_LOG"
case "$*" in
  'compose version') exit 0 ;;
  info) exit 0 ;;
  *'ps -q app') echo test-container ;;
  inspect*) echo "${MOCK_HEALTH:-healthy}" ;;
  *--healthcheck) exit "${MOCK_CHECK_EXIT:-0}" ;;
  *bootstrap-super-admin*)
    case "${MOCK_BOOTSTRAP:-ok}" in
      ok) echo 'Created system administrator: admin' ;;
      existing) echo 'Error: System administrator already exists; use the existing system administrator for user management' >&2; exit 1 ;;
      failure) echo 'Error: database connection refused' >&2; exit 1 ;;
    esac ;;
esac
STUB
printf '#!/usr/bin/env bash\nexit 0\n' > "$WORK/bin/sleep"
# A healthy service on a host port must not make an unhealthy app succeed.
printf '#!/usr/bin/env bash\nexit 0\n' > "$WORK/bin/curl"
chmod +x "$WORK/bin/"*
export PATH="$WORK/bin:$PATH" CALL_LOG="$WORK/calls"
cd "$WORK/run"
check() {
  local health=$1 bootstrap=$2 expected=$3 check_exit=${4:-0}
  : > "$CALL_LOG"
  local status=0
  MOCK_HEALTH="$health" MOCK_BOOTSTRAP="$bootstrap" MOCK_CHECK_EXIT="$check_exit" \
    bash "$SCRIPT" > "$WORK/output" 2>&1 || status=$?
  if [ "$expected" = success ]; then
    [ "$status" -eq 0 ] && grep -q '=== Done ===' "$WORK/output"
  else
    [ "$status" -ne 0 ] && ! grep -q '=== Done ===' "$WORK/output"
  fi
  if [ "$health" = unhealthy ] || { [ "$health" = running ] && [ "$check_exit" -ne 0 ]; }; then
    ! grep -q bootstrap-super-admin "$CALL_LOG"
  fi
  echo "PASS: health=$health bootstrap=$bootstrap check=$check_exit expected=$expected"
}
check healthy ok success
check unhealthy ok failure
check healthy failure failure
check healthy existing success
check running ok success
check running ok failure 1
