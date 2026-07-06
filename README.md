# Lab Safety System

实验室信息管理系统。主仓库是 Rust 后端，前端项目以 Git 子模块形式维护在 [`frontend/`](./frontend)。

- 后端仓库：`lab-safety-system`，负责认证授权、业务 API、文件上传和 PostgreSQL 数据访问。
- 前端子模块：`frontend/`，负责实验室安全管理界面和用户交互。
- 前端仓库：<https://github.com/LIghtJUNction/lab-safety-system-frontend>

## 功能范围

系统面向实验室安全管理场景，当前覆盖：

- 实验室安全隐患管理：问题照片上传、责任认领、整改照片上传、整改提交和闭环统计
- 管理员和普通用户不同视图
- 法规条例、事故案例、培训考核、设备预约、报修工单和统计分析
- 账号密码登录
- SSO 单点登录回调
- OAuth 第三方授权登录回调
- PostgreSQL 数据库存储

## 技术栈

- 后端：Rust
- Web 框架：Axum
- 数据库：PostgreSQL
- 前端：React + Vite，通过子模块维护
- 部署：Docker / Docker Compose
- 镜像仓库：Docker Hub + GHCR

## 推荐部署：整合镜像

推荐普通部署者使用整合镜像。这个镜像把前端静态页面和 Rust 后端 API 放在同一个容器里，只需要再启动一个 PostgreSQL 容器。

准备一台已安装 Docker 和 Docker Compose 的服务器，然后创建部署目录：

```bash
mkdir -p lab-safety-system
cd lab-safety-system
```

下载整合版 compose 文件和环境变量模板：

```bash
curl -fsSLO https://raw.githubusercontent.com/LIghtJUNction/lab-safety-system/main/docker-compose.integrated.yml
curl -fsSLo .env https://raw.githubusercontent.com/LIghtJUNction/lab-safety-system/main/.env.example
```

编辑 `.env`，至少修改这些值：

```env
POSTGRES_PASSWORD=请改成强密码
SECRET_KEY=请改成随机长密钥
```

启动服务：

```bash
docker compose -f docker-compose.integrated.yml up -d
```

首次部署后创建超级管理员。密码必须是强密码，至少 12 位并包含大小写字母、数字和符号：

```bash
docker compose -f docker-compose.integrated.yml exec app \
  lab-safety-system users bootstrap-super-admin \
  --username admin \
  --password '请替换为强密码' \
  --email admin@example.com \
  --display-name 超级管理员
```

查看状态：

```bash
docker compose -f docker-compose.integrated.yml ps
```

访问地址：

- 系统界面：`http://服务器IP:8080`
- 后端 API：`http://服务器IP:8080/api/v1`
- 健康检查：`http://服务器IP:8080/api/v1/ready`

停止服务：

```bash
docker compose -f docker-compose.integrated.yml down
```

升级到最新镜像：

```bash
docker compose -f docker-compose.integrated.yml pull
docker compose -f docker-compose.integrated.yml up -d
```

## 分离部署

如果需要前端和后端分开运行，可以使用分离版 compose 文件：

```bash
curl -fsSLO https://raw.githubusercontent.com/LIghtJUNction/lab-safety-system/main/docker-compose.prod.yml
curl -fsSLo .env https://raw.githubusercontent.com/LIghtJUNction/lab-safety-system/main/.env.example
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

POSTGRES_DB=lab_safety
POSTGRES_USER=lab_safety
POSTGRES_PASSWORD=change-me
POSTGRES_PORT=5432

SECRET_KEY=change-me-in-production
TOKEN_TTL_SECONDS=3600

SSO_ENABLED=false
OAUTH_ENABLED=false
SSO_LOGIN_URL=
OAUTH_LOGIN_URL=
FEDERATED_LOGIN_SECRET=
```

部署时必须修改 `POSTGRES_PASSWORD` 和 `SECRET_KEY`。如果服务器上 `5432`、`8080` 或 `8081` 已被占用，可以在 `.env` 中修改：

```env
POSTGRES_PORT=15432
APP_PORT=18080
FRONTEND_PORT=18081
```

整合版部署只使用 `APP_PORT` 作为访问端口；分离版部署会同时使用 `APP_PORT` 和 `FRONTEND_PORT`。

如果要接入 SSO 或 OAuth，把对应开关和跳转地址打开：

```env
SSO_ENABLED=true
SSO_LOGIN_URL=https://idp.example.com/sso/login
OAUTH_ENABLED=true
OAUTH_LOGIN_URL=https://idp.example.com/oauth/authorize
FEDERATED_LOGIN_SECRET=请改成随机长密钥
```

SSO 和 OAuth 回调地址：

- SSO：`https://你的域名/api/v1/auth/sso/callback`
- OAuth：`https://你的域名/api/v1/auth/oauth/callback`

企业 SSO 网关、OAuth2 Proxy 或 IdP 回调时需要传入 `username`、`email`、`display_name`、`role`、`department`、`exp`、`sig`。`role` 只能是 `admin` 或 `researcher`，不能通过联邦登录创建超级管理员。`sig` 使用 `FEDERATED_LOGIN_SECRET` 对下面的换行拼接内容做 HMAC-SHA256，并使用 base64url 无 padding 编码：

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

## 命令行用户管理

命令行用户管理仅允许超级管理员执行。首次部署只能在系统中不存在超级管理员时执行 bootstrap：

```bash
lab-safety-system users bootstrap-super-admin \
  --username admin \
  --password '请替换为强密码' \
  --email admin@example.com
```

之后所有用户管理命令都必须提供超级管理员账号和密码：

```bash
lab-safety-system users create \
  --actor admin \
  --actor-password '超级管理员强密码' \
  --username researcher01 \
  --password '普通用户强密码' \
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
  --password '新的普通用户强密码'
```

支持角色：

- `super_admin`：超级管理员，可以使用命令行管理用户
- `admin`：管理员，可以进入管理端界面
- `researcher`：普通用户，只进入个人安全任务界面

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

复制环境变量：

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

当前仓库已经配置好这两个 Secrets。

## 数据和上传文件

Compose 会创建两个持久化卷：

- `postgres-data`：PostgreSQL 数据
- `backend-uploads`：隐患照片、整改照片等上传文件

执行 `docker compose down` 不会删除数据卷。只有执行 `docker compose down -v` 才会删除数据库和上传文件。
