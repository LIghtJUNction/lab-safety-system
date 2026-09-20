#!/usr/bin/env bash
# Lab Safety System — one-click deploy (Linux / macOS / WSL)
# Requires: Docker + Compose v2. No .env required for first run.
set -euo pipefail

echo "=== Lab Safety System — one-click deploy ==="

COMPOSE_FILE=docker-compose.integrated.yml
COMPOSE_URL=https://raw.githubusercontent.com/LIghtJUNction/lab-safety-system/main/docker-compose.integrated.yml

if ! command -v docker >/dev/null 2>&1; then
  echo "Docker not found. Install Docker first." >&2
  exit 1
fi

docker compose version >/dev/null
docker info >/dev/null

if [ ! -f "$COMPOSE_FILE" ]; then
  echo "Downloading $COMPOSE_FILE ..."
  if command -v curl >/dev/null 2>&1; then
    curl -fsSLO "$COMPOSE_URL"
  elif command -v wget >/dev/null 2>&1; then
    wget -q "$COMPOSE_URL"
  else
    echo "Need curl or wget to download compose file, or copy docker-compose.integrated.yml into this directory." >&2
    exit 1
  fi
fi

echo "Starting app + postgres..."
docker compose -f "$COMPOSE_FILE" up -d

echo "Waiting for app to become healthy..."
HEALTHY=false
for _ in $(seq 1 45); do
  CID=$(docker compose -f "$COMPOSE_FILE" ps -q app 2>/dev/null || true)
  if [ -n "$CID" ]; then
    STATUS=$(docker inspect --format='{{if .State.Health}}{{.State.Health.Status}}{{else}}{{.State.Status}}{{end}}' "$CID" 2>/dev/null || true)
    if [ "$STATUS" = "healthy" ]; then
      HEALTHY=true
      break
    fi
    # Check the selected container, not a possibly unrelated host port.
    if [ "$STATUS" = "running" ] && docker compose -f "$COMPOSE_FILE" exec -T app \
      lab-safety-system --healthcheck >/dev/null 2>&1; then
      HEALTHY=true
      break
    fi
  fi
  sleep 2
done

if [ "$HEALTHY" != "true" ]; then
  echo "Error: app did not become healthy; administrator bootstrap was not attempted." >&2
  docker compose -f "$COMPOSE_FILE" ps || true
  echo "Check: docker compose -f $COMPOSE_FILE logs --tail=100 app postgres" >&2
  exit 1
fi

echo "Bootstrapping system administrator (if missing)..."
if BOOTSTRAP_OUTPUT=$(docker compose -f "$COMPOSE_FILE" exec -T app \
  lab-safety-system users bootstrap-super-admin \
  --username admin \
  --generate-password true \
  --email admin@example.local 2>&1); then
  printf '%s\n' "$BOOTSTRAP_OUTPUT"
else
  printf '%s\n' "$BOOTSTRAP_OUTPUT" >&2
  case "$BOOTSTRAP_OUTPUT" in
    *"System administrator already exists; use the existing system administrator for user management"*)
      echo "Use the existing administrator account; its password has not been changed."
      ;;
    *)
      echo "Error: administrator bootstrap failed. Deployment is not complete." >&2
      exit 1
      ;;
  esac
fi

echo ""
echo "=== Done ==="
echo "Open:      http://localhost:${APP_PORT:-8080}"
echo "Account:   newly created admin, or your existing administrator"
echo "Password:  see Generated password above (only shown on first create)"
echo ""
echo "Optional later: create a .env to override POSTGRES_PASSWORD / SECRET_KEY / APP_PORT"
