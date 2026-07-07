Write-Host "=== Laboratory Safety System Quickstart Deploy ===" -ForegroundColor Cyan

# Check if docker-compose.integrated.yml exists, otherwise download it
if (-not (Test-Path "docker-compose.integrated.yml")) {
    Write-Host "Downloading docker-compose.integrated.yml..."
    Invoke-WebRequest -Uri "https://raw.githubusercontent.com/LIghtJUNction/lab-safety-system/main/docker-compose.integrated.yml" -OutFile "docker-compose.integrated.yml"
}

# Run docker-compose
Write-Host "Starting services..."
docker compose -f docker-compose.integrated.yml up -d

Write-Host "Waiting for backend services to be healthy..."
for ($i = 1; $i -le 30; $i++) {
    $containerId = (docker compose -f docker-compose.integrated.yml ps -q app)
    if ($containerId) {
        $status = (docker inspect --format='{{json .State.Health.Status}}' $containerId)
        if ($status -eq '"healthy"') {
            break
        }
    }
    Start-Sleep -Seconds 2
}

# Bootstrap super admin
Write-Host "Creating default super admin..."
try {
    docker compose -f docker-compose.integrated.yml exec app lab-safety-system users bootstrap-super-admin --username admin --password 'Admin123!' --email admin@example.local
} catch {
    Write-Host "Super admin already exists or creation skipped."
}

Write-Host "=== Deployment Successful ===" -ForegroundColor Green
Write-Host "Access the system at: http://localhost:8080"
Write-Host "Default Credentials:"
Write-Host "Username: admin"
Write-Host "Password: Admin123!"
