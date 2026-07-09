# Lab Safety System — Windows one-click deploy
# Requires: Docker Desktop (Linux containers). No .env, bash, curl, or openssl needed.
$ErrorActionPreference = "Stop"
Write-Host "=== Lab Safety System — one-click deploy ===" -ForegroundColor Cyan

$composeFile = "docker-compose.integrated.yml"
$composeUrl = "https://raw.githubusercontent.com/LIghtJUNction/lab-safety-system/main/docker-compose.integrated.yml"

if (-not (Get-Command docker -ErrorAction SilentlyContinue)) {
    Write-Error "Docker not found. Install Docker Desktop and ensure it is running."
    exit 1
}

if (-not (Test-Path $composeFile)) {
    Write-Host "Downloading $composeFile ..."
    Invoke-WebRequest -Uri $composeUrl -OutFile $composeFile
}

# Compose file already embeds safe defaults — no .env required for first run.
Write-Host "Starting app + postgres (docker compose)..."
docker compose -f $composeFile up -d
if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }

Write-Host "Waiting for app to become healthy..."
$healthy = $false
for ($i = 1; $i -le 45; $i++) {
    $id = docker compose -f $composeFile ps -q app 2>$null
    if ($id) {
        $status = docker inspect --format='{{if .State.Health}}{{.State.Health.Status}}{{else}}{{.State.Status}}{{end}}' $id 2>$null
        if ($status -eq "healthy" -or $status -eq "running") {
            # Prefer health when present
            if ($status -eq "healthy") { $healthy = $true; break }
            # Fallback: ready endpoint
            try {
                $r = Invoke-WebRequest -Uri "http://127.0.0.1:8080/api/v1/ready" -UseBasicParsing -TimeoutSec 2
                if ($r.StatusCode -eq 200) { $healthy = $true; break }
            } catch {}
        }
    }
    Start-Sleep -Seconds 2
}

if (-not $healthy) {
    Write-Host "Warning: health wait timed out; showing compose status:" -ForegroundColor Yellow
    docker compose -f $composeFile ps
}

Write-Host "Bootstrapping system administrator (if missing)..."
$out = docker compose -f $composeFile exec -T app `
    lab-safety-system users bootstrap-super-admin `
    --username admin --generate-password true --email admin@example.local 2>&1
Write-Host $out

Write-Host ""
Write-Host "=== Done ===" -ForegroundColor Green
Write-Host "Open:      http://localhost:8080"
Write-Host "Username:  admin"
Write-Host "Password:  see Generated password above (only shown on first create)"
Write-Host ""
Write-Host "Optional later: create a .env to override POSTGRES_PASSWORD / SECRET_KEY / APP_PORT"
