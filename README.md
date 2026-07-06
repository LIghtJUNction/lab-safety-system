# Lab Safety System

实验室信息管理系统，当前重点是实验室安全管理。主仓库是 Rust 后端项目，前端项目以 Git 子模块维护在 [`frontend/`](./frontend)。

- 后端仓库：`lab-safety-system`，提供认证授权、业务 API、文件上传、命令行用户管理和 PostgreSQL 数据访问。
- 前端子模块：`frontend/`，提供实验室安全管理界面。
- 前端仓库：<https://github.com/LIghtJUNction/lab-safety-system-frontend>

## 功能范围

- 实验室安全隐患管理：问题照片上传、责任认领、整改照片上传、整改提交和闭环统计
- 管理员和普通用户登录后看到不同界面
- 法规条例、事故案例、培训考核、设备预约、报修工单、用户管理和统计分析
- 账号密码登录、Passkey、SSO 单点登录回调、OAuth 授权登录回调
- PostgreSQL 持久化存储
- 命令行用户管理：仅超级管理员可用

## 技术栈

- 后端：Rust + Axum
- 数据库：PostgreSQL
- 前端：React + Vite
- 部署：Docker / Docker Compose
- 镜像仓库：Docker Hub + GHCR

## 快速部署

推荐普通部署者使用整合镜像。这个镜像把前端静态页面和 Rust API 打包到同一个容器，另起一个 PostgreSQL 容器。

```bash
mkdir -p lab-safety-system
cd lab-safety-system
curl -fsSLO https://raw.githubusercontent.com/LIghtJUNction/lab-safety-system/main/docker-compose.integrated.yml
curl -fsSLo .env https://raw.githubusercontent.com/LIghtJUNction/lab-safety-system/main/.env.example
```

生成强密码和强密钥，并写入 `.env`。不要使用示例占位值作为生产密码。

```bash
openssl rand -base64 32
openssl rand -hex 32
```

至少修改：

```env
POSTGRES_PASSWORD=上面生成的数据库强密码
SECRET_KEY=上面生成的随机长密钥
```

启动：

```bash
docker compose -f docker-compose.integrated.yml up -d
```

首次部署后创建超级管理员。超级管理员密码必须至少 12 位，并包含小写字母、大写字母、数字和符号。

```bash
docker compose -f docker-compose.integrated.yml exec app \
  lab-safety-system users bootstrap-super-admin \
  --username admin \
  --password '请替换为强密码，例如 StrongAdminPass2026!' \
  --email admin@example.com \
  --display-name 超级管理员
```

访问地址：

- 系统界面：`http://服务器IP:8080`
- 后端 API：`http://服务器IP:8080/api/v1`
- 健康检查：`http://服务器IP:8080/api/v1/ready`

常用运维命令：

```bash
docker compose -f docker-compose.integrated.yml ps
docker compose -f docker-compose.integrated.yml pull
docker compose -f docker-compose.integrated.yml up -d
docker compose -f docker-compose.integrated.yml down
```

## 分离部署

如果需要前端和后端分开运行：

```bash
curl -fsSLO https://raw.githubusercontent.com/LIghtJUNction/lab-safety-system/main/docker-compose.prod.yml
curl -fsSLo .env https://raw.githubusercontent.com/LIghtJUNction/lab-safety-system/main/.env.example
```

修改 `.env` 中的 `POSTGRES_PASSWORD` 和 `SECRET_KEY` 后启动：

```bash
docker compose -f docker-compose.prod.yml up -d
```

分离版默认端口：

- 前端界面：`http://服务器IP:8081`
- 后端 API：`http://服务器IP:8080/api/v1`
- 健康检查：`http://服务器IP:8080/api/v1/ready`

## 镜像地址

Docker Hub：

- 整合镜像：`docker.io/lightjunction/lab-safety-system:latest`
- 后端镜像：`docker.io/lightjunction/lab-safety-system-backend:latest`
- 前端镜像：`docker.io/lightjunction/lab-safety-system-frontend:latest`

GHCR：

- 整合镜像：`ghcr.io/lightjunction/lab-safety-system:latest`
- 后端镜像：`ghcr.io/lightjunction/lab-safety-system-backend:latest`
- 前端镜像：`ghcr.io/lightjunction/lab-safety-system-frontend:latest`

如果需要固定版本，可以使用 GitHub Actions 手动发布时填写的版本标签，例如 `v0.1.0`。

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

整合版只使用 `APP_PORT` 作为访问端口；分离版同时使用 `APP_PORT` 和 `FRONTEND_PORT`。如果服务器上 `5432`、`8080` 或 `8081` 已被占用，可以在 `.env` 中修改：

```env
POSTGRES_PORT=15432
APP_PORT=18080
FRONTEND_PORT=18081
```

## SSO / OAuth

如果要接入 SSO 或 OAuth，把对应开关和跳转地址打开：

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

企业 SSO 网关、OAuth2 Proxy 或 IdP 回调时需要传入 `username`、`email`、`display_name`、`role`、`department`、`exp`、`sig`。`role` 只能是 `admin` 或 `researcher`，不能通过联邦登录创建超级管理员。

`sig` 使用 `FEDERATED_LOGIN_SECRET` 对下面的换行拼接内容做 HMAC-SHA256，并使用 base64url 无 padding 编码：

```text
provider
username
email
display_name
role
department
exp
```

其中 `provider` 是 `sso` 或 `oauth`，`exp` 是 Unix 秒级过期时间戳。首次成功回调会自动创建对应用户，后续回调会更新显示名、邮箱、角色和部门。

## Passkey

Passkey 使用浏览器 WebAuthn API。用户必须先用账号密码、SSO 或 OAuth 登录，再在右上角用户区域点击 `Passkey` 绑定；之后登录页密码框右侧可以使用 `Passkey` 登录。

生产部署必须把 WebAuthn 的 RP ID 和 Origin 配成真实访问域名，否则浏览器会拒绝凭据：

```env
WEBAUTHN_RP_ID=lab.example.com
WEBAUTHN_ORIGIN=https://lab.example.com
```

本地开发默认是：

```env
WEBAUTHN_RP_ID=localhost
WEBAUTHN_ORIGIN=http://localhost:5174
```

本地测试 Passkey 请使用 `http://localhost:5174` 访问前端。WebAuthn 不接受裸 IP 作为 RP ID；如果使用整合镜像或生产部署，建议配置 HTTPS 域名，例如 `lab.example.com`。

## 命令行用户管理

命令行用户管理仅允许超级管理员执行。首次部署只能在系统中不存在超级管理员时执行 bootstrap：

```bash
lab-safety-system users bootstrap-super-admin \
  --username admin \
  --password '请替换为强密码，例如 StrongAdminPass2026!' \
  --email admin@example.com
```

之后所有用户管理命令都必须提供超级管理员账号和密码：

```bash
lab-safety-system users create \
  --actor admin \
  --actor-password '超级管理员强密码' \
  --username researcher01 \
  --password '请替换为强密码，例如 ResearcherPass2026!' \
  --email researcher01@example.com \
  --role researcher \
  --display-name 研究员01

lab-safety-system users list \
  --actor admin \
  --actor-password '超级管理员强密码'

lab-safety-system users set-password \
  --actor admin \
  --actor-password '超级管理员强密码' \
  --username researcher01 \
  --password '请替换为新的强密码，例如 ResearcherPass2027!'

lab-safety-system users set-password \
  --actor admin \
  --actor-password '超级管理员强密码' \
  --username admin \
  --generate-password true
```

密码策略：至少 12 位，并包含小写字母、大写字母、数字和符号。弱密码会被后端和命令行工具拒绝。已保存的密码不会以明文取回；如需重新获取管理员可用密码，请在服务器终端执行 `set-password --generate-password true`，命令会生成强密码并立即重置目标用户密码。

支持角色：

- `super_admin`：超级管理员，可以使用命令行管理用户
- `admin`：管理员，可以进入管理端界面
- `researcher`：普通用户，只进入个人安全任务界面

## 本地验证

后端和前端服务启动后，可以执行以下命令做完整冒烟验证。该验证会打开登录页，使用账号密码登录，检查 SSO/OAuth 回调错误处理，创建法规、事故案例、培训、设备、用户、预约、报修、考核记录，并用普通用户完成隐患上报、责任认领、问题照片上传和整改照片上传。

```bash
cd frontend
npm run build
E2E_BASE_URL=http://127.0.0.1:5174 \
E2E_ADMIN_USER=cli_super \
E2E_ADMIN_PASSWORD='StrongerAdmin123!' \
E2E_FEDERATED_SECRET='federated-local-secret' \
npm run e2e:smoke
```

如果部署环境未启用 SSO/OAuth，登录页会显示对应入口未配置；后端回调只有在 `SSO_ENABLED` / `OAUTH_ENABLED` 和 `FEDERATED_LOGIN_SECRET` 配置正确时才会接受已签名的身份提供方回调。

## 本地源码运行

首次克隆需要拉取前端子模块：

```bash
git clone --recurse-submodules https://github.com/LIghtJUNction/lab-safety-system.git
cd lab-safety-system
```

如果已经克隆了主仓库：

```bash
git submodule update --init --recursive
```

复制环境变量并替换强密码/强密钥：

```bash
cp .env.example .env
```

从源码构建并启动前端、后端和 PostgreSQL：

```bash
docker compose up -d --build
```

从源码构建整合镜像：

```bash
docker build -f Dockerfile.integrated -t lab-safety-system:latest .
```

停止服务：

```bash
docker compose down
```

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
- `backend-uploads`：隐患照片、整改照片等上传文件

执行 `docker compose down` 不会删除数据卷。只有执行 `docker compose down -v` 才会删除数据库和上传文件。

## 开源许可和声明

本项目使用 AGPL-3.0-only 许可证，详见 [`LICENSE`](./LICENSE)。软件按现状提供，不附带任何明示或默示担保。部署者需要自行负责生产环境的数据安全、账号策略、备份、审计和合规要求。

源码仓库：<https://github.com/LIghtJUNction/lab-safety-system>
