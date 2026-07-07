#!/bin/bash
set -e

echo "=== Laboratory Safety System Quickstart Deploy ==="

# Check if docker-compose.integrated.yml exists, otherwise download it
if [ ! -f "docker-compose.integrated.yml" ]; then
  echo "Downloading docker-compose.integrated.yml..."
  curl -fsSLO https://raw.githubusercontent.com/LIghtJUNction/lab-safety-system/main/docker-compose.integrated.yml
fi

# Run docker-compose
echo "Starting services..."
docker compose -f docker-compose.integrated.yml up -d

echo "Waiting for backend services to be healthy..."
for i in {1..30}; do
  CONTAINER_ID=$(docker compose -f docker-compose.integrated.yml ps -q app)
  if [ -n "$CONTAINER_ID" ]; then
    STATUS=$(docker inspect --format='{{json .State.Health.Status}}' "$CONTAINER_ID")
    if [ "$STATUS" = "\"healthy\"" ]; then
      break
    fi
  fi
  sleep 2
done

# Bootstrap super admin
echo "Creating default super admin..."
docker compose -f docker-compose.integrated.yml exec app \
  lab-safety-system users bootstrap-super-admin \
  --username admin \
  --password 'Admin123!' \
  --email admin@example.local || echo "Super admin already exists or creation skipped."

echo "=== Deployment Successful ==="
echo "Access the system at: http://localhost:8080"
echo "Default Credentials:"
echo "Username: admin"
echo "Password: Admin123!"
