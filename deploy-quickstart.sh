#!/bin/bash
set -e

echo "=== Laboratory Safety System Quickstart Deploy ==="

random_hex() {
  if command -v openssl >/dev/null 2>&1; then
    openssl rand -hex 32
  else
    od -An -N32 -tx1 /dev/urandom | tr -d ' \n'
    printf '\n'
  fi
}

# Check if docker-compose.integrated.yml exists, otherwise download it
if [ ! -f "docker-compose.integrated.yml" ]; then
  echo "Downloading docker-compose.integrated.yml..."
  curl -fsSLO https://raw.githubusercontent.com/LIghtJUNction/lab-safety-system/main/docker-compose.integrated.yml
fi

if [ ! -f ".env" ]; then
  echo "Creating .env with generated install secrets..."
  cat > .env <<EOF
APP_ENV=production
APP_HOST=0.0.0.0
APP_PORT=8080
POSTGRES_DB=lab_safety
POSTGRES_USER=lab_safety
POSTGRES_PASSWORD=$(random_hex)
POSTGRES_PORT=5432
SECRET_KEY=$(random_hex)
TOKEN_TTL_SECONDS=3600
UPLOAD_DIR=/app/uploads
STATIC_DIR=/app/public
SSO_ENABLED=false
OAUTH_ENABLED=false
SSO_LOGIN_URL=
OAUTH_LOGIN_URL=
FEDERATED_LOGIN_SECRET=
WEBAUTHN_RP_ID=localhost
WEBAUTHN_ORIGIN=http://localhost:8080
CORS_ALLOWED_ORIGINS=
EOF
fi

# Run docker-compose
echo "Starting services..."
docker compose -f docker-compose.integrated.yml up -d

echo "Waiting for backend services to be healthy..."
HEALTHY=false
for i in {1..30}; do
  CONTAINER_ID=$(docker compose -f docker-compose.integrated.yml ps -q app)
  if [ -n "$CONTAINER_ID" ]; then
    STATUS=$(docker inspect --format='{{json .State.Health.Status}}' "$CONTAINER_ID")
    if [ "$STATUS" = "\"healthy\"" ]; then
      HEALTHY=true
      break
    fi
  fi
  sleep 2
done

if [ "$HEALTHY" != "true" ]; then
  echo "Backend did not become healthy within 60 seconds." >&2
  docker compose -f docker-compose.integrated.yml ps
  exit 1
fi

# Bootstrap super admin
echo "Creating system administrator with generated password..."
docker compose -f docker-compose.integrated.yml exec -T app \
  lab-safety-system users bootstrap-super-admin \
  --username admin \
  --generate-password true \
  --email admin@example.local || echo "System administrator already exists or creation skipped."

echo "=== Deployment Successful ==="
echo "Access the system at: http://localhost:8080"
echo "Initial administrator:"
echo "Username: admin"
echo "Password: use the Generated password printed above"
