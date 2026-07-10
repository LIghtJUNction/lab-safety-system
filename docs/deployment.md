# 多平台部署与首次管理员登录教程

本文档面向第一次部署 Lab Safety System 的管理员。推荐使用整合镜像：前端页面、Rust 后端 API 在一个容器内运行，PostgreSQL 单独运行。

## 适用平台

整合镜像发布为多平台镜像：

- `linux/amd64`：常见 x86_64 服务器、桌面 Linux、Windows Docker Desktop、macOS Intel。
- `linux/arm64`：ARM 服务器、Apple Silicon macOS、常见 ARM64 NAS。

Docker 会自动拉取匹配当前机器的平台镜像。少数 NAS 或虚拟化环境识别错误时，可以在启动前指定：

```bash
export DOCKER_DEFAULT_PLATFORM=linux/amd64
# 或
export DOCKER_DEFAULT_PLATFORM=linux/arm64
```

## 准备环境

需要安装：

- Docker Engine 或 Docker Desktop
- Docker Compose v2

检查命令：

```bash
docker --version
docker compose version
```

**零配置原则**：`docker-compose.integrated.yml` 内置可用默认值，**不强制 `.env`**。首次启动会生成随机 `SECRET_KEY` 并保存在专用 `app-config` 卷中，PostgreSQL 默认只在 Compose 内网开放。Windows / Linux 一键脚本只做：下载 compose → `up -d` → 等健康 → bootstrap 管理员。

## Windows（PowerShell，推荐）

前置：安装 [Docker Desktop](https://www.docker.com/products/docker-desktop/)，使用 **Linux containers**。

```powershell
mkdir lab-safety-system; cd lab-safety-system
iwr https://raw.githubusercontent.com/LIghtJUNction/lab-safety-system/main/deploy-quickstart.ps1 -OutFile deploy-quickstart.ps1
powershell -ExecutionPolicy Bypass -File .\deploy-quickstart.ps1
```

- 不依赖 bash / curl / openssl，不写 `.env`
- 服务名 `app` + `postgres`；访问 <http://localhost:8080>
- 用户名 `admin`；密码看终端 `Generated password`（仅首次创建时打印）

极简手动版（同样无 `.env`）：

```powershell
iwr https://raw.githubusercontent.com/LIghtJUNction/lab-safety-system/main/docker-compose.integrated.yml -OutFile docker-compose.integrated.yml
docker compose -f docker-compose.integrated.yml up -d
docker compose -f docker-compose.integrated.yml exec -T app lab-safety-system users bootstrap-super-admin --generate-password true
```

## Linux / macOS 一键部署

```bash
mkdir -p lab-safety-system && cd lab-safety-system
curl -fsSLO https://raw.githubusercontent.com/LIghtJUNction/lab-safety-system/main/deploy-quickstart.sh
bash deploy-quickstart.sh
```

或：

```bash
curl -fsSLO https://raw.githubusercontent.com/LIghtJUNction/lab-safety-system/main/docker-compose.integrated.yml
docker compose -f docker-compose.integrated.yml up -d
docker compose -f docker-compose.integrated.yml exec -T app \
  lab-safety-system users bootstrap-super-admin --generate-password true
```

可选：以后用 `.env` 覆盖 `POSTGRES_PASSWORD` / `SECRET_KEY` / `APP_PORT`。未设置 `SECRET_KEY` 时会自动生成并持久化，无需手工配置。

## Arch Linux（AUR）

```bash
paru -S lab-safety-system-git
sudo lab-safety-system-setup
```

`lab-safety-system-setup` 会启用 PostgreSQL、生成 env（已存在则不覆盖）、启动 systemd、尝试 bootstrap 管理员。

## 第一次管理员登录

一键脚本通常已创建管理员。若需手动：

```bash
docker compose -f docker-compose.integrated.yml exec app \
  lab-safety-system users bootstrap-super-admin \
  --generate-password true
```

命令会输出类似：

```text
Created system administrator: admin
Generated password: Abc...!
```

第一次登录信息：

- 用户名：`admin`
- 密码：使用命令输出的 `Generated password`
- 登录地址：`http://服务器IP:8080`

如果你修改了 `APP_PORT`，登录地址中的端口也要对应修改。

## Docker 里如何获取管理员密码

不能从数据库或容器里“读取”现有管理员密码。后端只保存密码哈希，不保存明文密码。

正确流程是：

1. 第一次部署时，用 `bootstrap-super-admin --generate-password true` 创建管理员并打印一次强密码。
2. 如果忘记密码，用 `set-password --generate-password true` 重置密码，后端会打印新密码。

整合版 Docker 首次创建管理员：

```bash
docker compose -f docker-compose.integrated.yml exec app \
  lab-safety-system users bootstrap-super-admin \
  --generate-password true
```

整合版 Docker 重置管理员密码：

```bash
docker compose -f docker-compose.integrated.yml exec app \
  lab-safety-system users set-password \
  --actor admin \
  --actor-password '当前系统管理员密码' \
  --username admin \
  --generate-password true
```

分离版 Docker 把服务名从 `app` 换成 `backend`：

```bash
docker compose -f docker-compose.prod.yml exec backend \
  lab-safety-system users set-password \
  --actor admin \
  --actor-password '当前系统管理员密码' \
  --username admin \
  --generate-password true
```

也可以显式指定用户名和邮箱：

```bash
docker compose -f docker-compose.integrated.yml exec app \
  lab-safety-system users bootstrap-super-admin \
  --username admin \
  --email admin@example.com \
  --display-name 系统管理员 \
  --generate-password true
```

如果已经创建过系统管理员，再执行 bootstrap 会被拒绝。需要重置密码时，使用已有系统管理员执行：

```bash
docker compose -f docker-compose.integrated.yml exec app \
  lab-safety-system users set-password \
  --actor admin \
  --actor-password '当前系统管理员密码' \
  --username admin \
  --generate-password true
```

## 分离部署

如果需要前端、后端、数据库分开运行，下载分离版 Compose：

```bash
mkdir -p lab-safety-system
cd lab-safety-system
curl -fsSLO https://raw.githubusercontent.com/LIghtJUNction/lab-safety-system/main/docker-compose.prod.yml
curl -fsSLo .env https://raw.githubusercontent.com/LIghtJUNction/lab-safety-system/main/.env.example
```

编辑 `.env` 后启动：

```bash
docker compose -f docker-compose.prod.yml up -d
```

默认端口：

- 前端：`http://服务器IP:8081`
- 后端 API：`http://服务器IP:8080/api/v1`
- 数据库：服务器本机 `5432`

分离部署的首次管理员创建命令：

```bash
docker compose -f docker-compose.prod.yml exec backend \
  lab-safety-system users bootstrap-super-admin \
  --generate-password true
```

## 本地源码部署

首次克隆：

```bash
git clone --recurse-submodules https://github.com/LIghtJUNction/lab-safety-system.git
cd lab-safety-system
cp .env.example .env
```

编辑 `.env` 中的强密码和密钥，然后启动：

```bash
docker compose up -d --build
```

从源码构建整合镜像：

```bash
docker build -f Dockerfile.integrated -t lab-safety-system:local .
```

## Arch Linux / AUR 安装

本项目提供 AUR `-git` 包：

```bash
paru -S lab-safety-system-git
```

安装内容：

- `/usr/bin/lab-safety-system`
- `/usr/lib/systemd/system/lab-safety-system.service`
- `/etc/lab-safety-system/lab-safety-system.env`
- `/usr/share/lab-safety-system/public`
- `/var/lib/lab-safety-system/uploads`

配置目录是 `/etc/lab-safety-system/`。主要配置文件是：

```text
/etc/lab-safety-system/lab-safety-system.env
```

先准备本机 PostgreSQL 数据库。示例会创建 `lab_safety` 用户和 `lab_safety` 数据库，密码必须使用强密码：

```bash
sudo -u postgres createuser --pwprompt lab_safety
sudo -u postgres createdb -O lab_safety lab_safety
```

然后编辑环境文件：

```bash
sudoedit /etc/lab-safety-system/lab-safety-system.env
```

至少替换：

```env
DATABASE_URL=postgresql://lab_safety:数据库密码@127.0.0.1:5432/lab_safety
SECRET_KEY=使用 openssl rand -hex 32 生成
WEBAUTHN_ORIGIN=http://localhost:8080
```

`DATABASE_URL` 格式是：

```text
postgresql://用户名:密码@数据库主机:端口/数据库名
```

启用服务：

```bash
sudo systemctl enable --now lab-safety-system
```

AUR 本机首次创建管理员：

```bash
sudo -u lab-safety-system \
  env $(grep -v '^#' /etc/lab-safety-system/lab-safety-system.env | xargs) \
  lab-safety-system users bootstrap-super-admin \
  --generate-password true
```

如果只是临时测试，也可以不启动 systemd，直接指定 `DATABASE_URL` 运行命令：

```bash
DATABASE_URL='postgresql://lab_safety:密码@127.0.0.1:5432/lab_safety' \
  lab-safety-system users bootstrap-super-admin \
  --generate-password true
```

## HTTPS、域名和 Passkey

如果要在生产环境启用 Passkey，必须使用 HTTPS 域名。配置示例：

```env
WEBAUTHN_RP_ID=lab.example.com
WEBAUTHN_ORIGIN=https://lab.example.com
```

如果只是内网测试账号密码登录，可以先使用 `http://服务器IP:8080`。Passkey 不支持裸 IP 作为 RP ID。

## 常用运维命令

查看容器：

```bash
docker compose -f docker-compose.integrated.yml ps
```

查看日志：

```bash
docker compose -f docker-compose.integrated.yml logs -f app
docker compose -f docker-compose.integrated.yml logs -f postgres
```

升级镜像：

```bash
docker compose -f docker-compose.integrated.yml pull
docker compose -f docker-compose.integrated.yml up -d
```

停止服务：

```bash
docker compose -f docker-compose.integrated.yml down
```

停止并删除数据：

```bash
docker compose -f docker-compose.integrated.yml down -v
```

`down -v` 会删除 PostgreSQL 数据和上传文件，生产环境谨慎使用。

## 数据持久化

Compose 会创建两个卷：

- `postgres-data`：数据库数据。
- `backend-uploads`：法规附件、事故附件、隐患照片和整改照片。

建议定期备份 PostgreSQL 和上传卷。应用内置备份命令会生成一个 `tar.gz`
归档，包含：

- `database.sql`：PostgreSQL 逻辑备份。
- `uploads/`：法规附件、事故附件、隐患照片和整改照片。
- `metadata.json`：备份创建时间和应用版本。

整合 Docker 部署：

```bash
docker compose -f docker-compose.integrated.yml exec app \
  lab-safety-system backup create
```

分离 Docker 部署：

```bash
docker compose exec backend lab-safety-system backup create
```

AUR / systemd 部署：

```bash
sudo -u lab-safety-system lab-safety-system backup create
```

默认备份文件会写入上传目录的 `backups/` 子目录。也可以指定输出路径：

```bash
lab-safety-system backup create --output /var/backups/lab-safety-system-$(date +%F).tar.gz
```

备份命令默认拒绝覆盖已有归档；确需覆盖时显式追加 `--force true`。

备份归档包含敏感数据，生产环境应放入受控目录并纳入服务器级快照或异地备份。

## 镜像地址

Docker Hub：

- `docker.io/lightjunction/lab-safety-system:latest`
- `docker.io/lightjunction/lab-safety-system-backend:latest`
- `docker.io/lightjunction/lab-safety-system-frontend:latest`

GHCR：

- `ghcr.io/lightjunction/lab-safety-system:latest`
- `ghcr.io/lightjunction/lab-safety-system-backend:latest`
- `ghcr.io/lightjunction/lab-safety-system-frontend:latest`

## 排错

端口被占用：修改 `.env` 中的 `APP_PORT` 或 `POSTGRES_PORT`，然后重新 `up -d`。

忘记管理员密码：使用 `users set-password --generate-password true` 生成新密码。

数据库未就绪：执行 `docker compose -f docker-compose.integrated.yml ps`，等待 PostgreSQL healthcheck 通过。

无法使用 Passkey：确认使用 HTTPS 域名访问，并正确配置 `WEBAUTHN_RP_ID` 与 `WEBAUTHN_ORIGIN`。
