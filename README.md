# Lab Safety System

实验室信息管理系统，当前重点是实验室安全管理。主仓库是 Rust 后端项目，前端项目以 Git 子模块维护在 [`frontend/`](./frontend)。

- 后端仓库：`lab-safety-system`，提供认证授权、业务 API、文件上传、命令行用户管理和 PostgreSQL 数据访问。
- 前端子模块：`frontend/`，提供实验室安全管理界面。
- 前端仓库：<https://github.com/LIghtJUNction/lab-safety-system-frontend>
- 多平台部署与首次管理员登录教程：[`docs/deployment.md`](./docs/deployment.md)
- 后端功能与前端对接文档：[`docs/backend-api.md`](./docs/backend-api.md)
- 前后端开发与联调协作指南：[`docs/collaboration-guide.md`](./docs/collaboration-guide.md)

## 功能范围

- 多实验室管理：实验室主数据、实验室成员关系、实验室内角色
- 实验室安全隐患管理：按实验室上报问题照片、责任认领、整改照片上传、整改提交和闭环统计
- 系统管理员、实验室管理员、实验室成员、访客登录后看到不同界面
- 法规条例、事故案例、培训考核、设备预约、报修工单、用户管理和统计分析
- 账号密码登录、Passkey、SSO 单点登录回调、OAuth 授权登录回调
- PostgreSQL 持久化存储
- 命令行用户管理：仅系统管理员可用

## 技术栈

- 后端：Rust + Axum
- 数据库：PostgreSQL
- 前端：React + Vite
- 部署：Docker / Docker Compose
- 镜像仓库：Docker Hub + GHCR

## 快速部署

推荐使用整合镜像。它把前端静态页面和 Rust API 打包到一个容器，另起一个 PostgreSQL 容器。

```bash
mkdir -p lab-safety-system
cd lab-safety-system
curl -fsSLO https://raw.githubusercontent.com/LIghtJUNction/lab-safety-system/main/docker-compose.integrated.yml
curl -fsSLo .env https://raw.githubusercontent.com/LIghtJUNction/lab-safety-system/main/.env.example
```

生成强密码和强密钥：

```bash
openssl rand -base64 32
openssl rand -hex 32
```

编辑 `.env`，至少替换：

```env
POSTGRES_PASSWORD=替换为数据库强密码
SECRET_KEY=替换为随机长密钥
```

启动：

```bash
docker compose -f docker-compose.integrated.yml up -d
```

首次创建系统管理员，后端会自动生成强密码：

```bash
docker compose -f docker-compose.integrated.yml exec app \
  lab-safety-system users bootstrap-super-admin \
  --generate-password true
```

命令输出 `Generated password` 后，使用下面的信息登录：

- 地址：`http://服务器IP:8080`
- 用户名：`admin`
- 密码：命令输出的强密码

完整部署、Windows/macOS/Linux/NAS/ARM64、分离部署、升级和排错见 [`docs/deployment.md`](./docs/deployment.md)。

管理员密码不能从 Docker 或数据库中读取明文；首次创建和忘记密码时都使用 `--generate-password true` 让后端生成并打印新密码，具体命令见部署教程。

## 镜像地址

Docker Hub：

- 整合镜像：`docker.io/lightjunction/lab-safety-system:latest`
- 后端镜像：`docker.io/lightjunction/lab-safety-system-backend:latest`
- 前端镜像：`docker.io/lightjunction/lab-safety-system-frontend:latest`

GHCR：

- 整合镜像：`ghcr.io/lightjunction/lab-safety-system:latest`
- 后端镜像：`ghcr.io/lightjunction/lab-safety-system-backend:latest`
- 前端镜像：`ghcr.io/lightjunction/lab-safety-system-frontend:latest`

镜像发布为 `linux/amd64` 和 `linux/arm64` 多平台镜像。

Arch Linux 用户也可以通过 AUR 安装：

```bash
paru -S lab-safety-system-git
```

## 环境变量

常用配置通过 `.env` 提供：

```env
APP_ENV=production
APP_HOST=0.0.0.0
APP_PORT=8080
FRONTEND_PORT=8081
APP_IMAGE=docker.io/lightjunction/lab-safety-system:latest
BACKEND_IMAGE=docker.io/lightjunction/lab-safety-system-backend:latest
FRONTEND_IMAGE=docker.io/lightjunction/lab-safety-system-frontend:latest
POSTGRES_DB=lab_safety
POSTGRES_USER=lab_safety
POSTGRES_PASSWORD=必须替换为强密码
POSTGRES_PORT=5432
SECRET_KEY=必须替换为随机长密钥
TOKEN_TTL_SECONDS=3600
UPLOAD_DIR=/app/uploads
SSO_ENABLED=false
OAUTH_ENABLED=false
SSO_LOGIN_URL=
OAUTH_LOGIN_URL=
FEDERATED_LOGIN_SECRET=
WEBAUTHN_RP_ID=你的域名
WEBAUTHN_ORIGIN=https://你的域名
```

## SSO / OAuth

启用 SSO 或 OAuth：

```env
SSO_ENABLED=true
SSO_LOGIN_URL=https://idp.example.com/sso/login
OAUTH_ENABLED=true
OAUTH_LOGIN_URL=https://idp.example.com/oauth/authorize
FEDERATED_LOGIN_SECRET=必须替换为随机长密钥
```

回调地址：

- SSO：`https://你的域名/api/v1/auth/sso/callback`
- OAuth：`https://你的域名/api/v1/auth/oauth/callback`

企业 SSO 网关、OAuth2 Proxy 或 IdP 回调时需要传入 `username`、`email`、`display_name`、`role`、`department`、`exp`、`sig`。`role` 只能是 `lab_member` 或 `visitor`，不能通过联邦登录创建系统管理员。

## Passkey

Passkey 使用浏览器 WebAuthn API。用户必须先用账号密码、SSO 或 OAuth 登录，再在个人资料界面绑定 Passkey。

生产部署必须配置真实 HTTPS 域名：

```env
WEBAUTHN_RP_ID=lab.example.com
WEBAUTHN_ORIGIN=https://lab.example.com
```

## 命令行用户管理

首次部署：

```bash
lab-safety-system users bootstrap-super-admin --generate-password true
```

创建用户：

```bash
lab-safety-system users create \
  --actor admin \
  --actor-password '系统管理员强密码' \
  --username member01 \
  --password 'MemberPass2026!' \
  --email member01@example.com \
  --role lab_member \
  --display-name 实验室成员01
```

重置密码并生成强密码：

```bash
lab-safety-system users set-password \
  --actor admin \
  --actor-password '系统管理员强密码' \
  --username admin \
  --generate-password true
```

密码策略：至少 12 位，并包含小写字母、大写字母、数字和符号。

## 本地源码运行

```bash
git clone --recurse-submodules https://github.com/LIghtJUNction/lab-safety-system.git
cd lab-safety-system
cp .env.example .env
docker compose up -d --build
```

从源码构建整合镜像：

```bash
docker build -f Dockerfile.integrated -t lab-safety-system:latest .
```

## 验证

后端验证：

```bash
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo test
```

前端冒烟验证见 [`frontend/tests/e2e-smoke.mjs`](./frontend/tests/e2e-smoke.mjs)。

## 镜像发布

仓库内置 GitHub Actions 工作流 [`Docker Publish`](.github/workflows/docker-publish.yml)，会同时发布到 GHCR 和 Docker Hub。

触发方式：

- 推送到 `main`：发布 `latest`、`main` 和 `sha-*` 标签
- 推送 `v*` tag：发布对应版本标签
- 手动运行 workflow，并可选填写 `version`

手动发布示例：

```bash
gh workflow run "Docker Publish" -f version=v0.1.0
```

Docker Hub 发布依赖仓库 Secrets：

- `DOCKERHUB_USERNAME`
- `DOCKERHUB_TOKEN`

## 数据和上传文件

Compose 会创建两个持久化卷：

- `postgres-data`：PostgreSQL 数据
- `backend-uploads`：法规附件、事故附件、隐患照片和整改照片

执行 `docker compose down` 不会删除数据卷。只有执行 `docker compose down -v` 才会删除数据库和上传文件。

## 开源许可和声明

本项目使用 AGPL-3.0-only 许可证，详见 [`LICENSE`](./LICENSE)。软件按现状提供，不附带任何明示或默示担保。部署者需要自行负责生产环境的数据安全、账号策略、备份、审计和合规要求。

源码仓库：<https://github.com/LIghtJUNction/lab-safety-system>
