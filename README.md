# Lab Safety System

实验室信息管理系统后端项目。主仓库是 Rust 后端，前端项目以 Git 子模块形式维护在 [`frontend/`](./frontend)。

- 后端仓库：`lab-safety-system`，负责认证授权、业务 API、文件上传和 PostgreSQL 数据访问。
- 前端子模块：`frontend/`，负责实验室安全管理界面和用户交互。
- 前端仓库：<https://github.com/LIghtJUNction/lab-safety-system-frontend>

## 功能范围

系统面向实验室安全管理场景，当前覆盖：

- 实验室安全隐患管理：问题照片上传、责任认领、整改照片上传、整改提交和闭环统计
- 管理员和普通用户不同视图
- 法规条例、事故案例、培训考核、设备预约、报修工单和统计分析
- 账号密码登录扩展点
- SSO 单点登录扩展点
- OAuth 第三方授权登录扩展点
- PostgreSQL 数据库存储

## 技术栈

- 后端：Rust
- Web 框架：Axum
- 数据库：PostgreSQL
- 前端：React + Vite，通过子模块维护
- 部署：Docker / Docker Compose
- 镜像仓库：Docker Hub + GHCR

## 快速部署

推荐普通部署者使用 Docker Hub 镜像，不需要安装 Rust、Node.js，也不需要拉取前端子模块。

准备一台已安装 Docker 和 Docker Compose 的服务器，然后创建部署目录：

```bash
mkdir -p lab-safety-system
cd lab-safety-system
```

下载生产 compose 文件和环境变量模板：

```bash
curl -fsSLO https://raw.githubusercontent.com/LIghtJUNction/lab-safety-system/main/docker-compose.prod.yml
curl -fsSLo .env https://raw.githubusercontent.com/LIghtJUNction/lab-safety-system/main/.env.example
```

编辑 `.env`，至少修改这些值：

```env
POSTGRES_PASSWORD=请改成强密码
SECRET_KEY=请改成随机长密钥
```

启动服务：

```bash
docker compose -f docker-compose.prod.yml up -d
```

查看状态：

```bash
docker compose -f docker-compose.prod.yml ps
```

访问地址：

- 前端界面：`http://服务器IP:8081`
- 后端 API：`http://服务器IP:8080/api/v1`
- 健康检查：`http://服务器IP:8080/api/v1/ready`

停止服务：

```bash
docker compose -f docker-compose.prod.yml down
```

升级到最新镜像：

```bash
docker compose -f docker-compose.prod.yml pull
docker compose -f docker-compose.prod.yml up -d
```

## 镜像地址

Docker Hub：

- `docker.io/lightjunction/lab-safety-system-backend:latest`
- `docker.io/lightjunction/lab-safety-system-frontend:latest`

GHCR：

- `ghcr.io/lightjunction/lab-safety-system-backend:latest`
- `ghcr.io/lightjunction/lab-safety-system-frontend:latest`

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
SECRET_KEY=change-me-in-production
TOKEN_TTL_SECONDS=3600

SSO_ENABLED=false
OAUTH_ENABLED=false
```

部署时必须修改 `POSTGRES_PASSWORD` 和 `SECRET_KEY`。如果服务器上 `5432`、`8080` 或 `8081` 已被占用，可以在 `.env` 中修改：

```env
POSTGRES_PORT=15432
APP_PORT=18080
FRONTEND_PORT=18081
```

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

查看服务：

```bash
docker compose ps
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
