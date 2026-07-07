Write-Host "=== Laboratory Safety System Quickstart Deploy ===" -ForegroundColor Cyan

function New-RandomSecret {
    $bytes = New-Object byte[] 32
    $rng = [System.Security.Cryptography.RandomNumberGenerator]::Create()
    try {
        $rng.GetBytes($bytes)
    } finally {
        $rng.Dispose()
    }
    return ([BitConverter]::ToString($bytes) -replace "-", "").ToLowerInvariant()
}

# Check if docker-compose.integrated.yml exists, otherwise download it
if (-not (Test-Path "docker-compose.integrated.yml")) {
    Write-Host "Downloading docker-compose.integrated.yml..."
    Invoke-WebRequest -Uri "https://raw.githubusercontent.com/LIghtJUNction/lab-safety-system/main/docker-compose.integrated.yml" -OutFile "docker-compose.integrated.yml"
}

if (-not (Test-Path ".env")) {
    Write-Host "Creating .env with generated install secrets..."
    @"
APP_ENV=production
APP_HOST=0.0.0.0
APP_PORT=8080
POSTGRES_DB=lab_safety
POSTGRES_USER=lab_safety
POSTGRES_PASSWORD=$(New-RandomSecret)
POSTGRES_PORT=5432
SECRET_KEY=$(New-RandomSecret)
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
"@ | Set-Content -Encoding UTF8 ".env"
}

# Run docker-compose
Write-Host "Starting services..."
docker compose -f docker-compose.integrated.yml up -d

Write-Host "Waiting for backend services to be healthy..."
$healthy = $false
for ($i = 1; $i -le 30; $i++) {
    $containerId = (docker compose -f docker-compose.integrated.yml ps -q app)
    if ($containerId) {
        $status = (docker inspect --format='{{json .State.Health.Status}}' $containerId)
        if ($status -eq '"healthy"') {
            $healthy = $true
            break
        }
    }
    Start-Sleep -Seconds 2
}

if (-not $healthy) {
    Write-Error "Backend did not become healthy within 60 seconds."
    docker compose -f docker-compose.integrated.yml ps
    exit 1
}

# Bootstrap super admin
Write-Host "Creating system administrator with generated password..."
docker compose -f docker-compose.integrated.yml exec -T app lab-safety-system users bootstrap-super-admin --username admin --generate-password true --email admin@example.local
if ($LASTEXITCODE -ne 0) {
    Write-Host "System administrator already exists or creation skipped."
}

Write-Host "=== Deployment Successful ===" -ForegroundColor Green
Write-Host "Access the system at: http://localhost:8080"
Write-Host "Initial administrator:"
Write-Host "Username: admin"
Write-Host "Password: use the Generated password printed above"
