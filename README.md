# Lab Safety System

实验室信息管理系统后端项目，用于支撑实验室安全管理、账号认证、权限控制和业务数据持久化。

> 前端项目以 Git 子模块形式维护在 [`frontend/`](./frontend)，对应仓库：
> <https://github.com/LIghtJUNction/lab-safety-system-frontend>

## 项目定位

本仓库是实验室安全管理信息系统的后端工程，负责提供 API、认证授权、业务规则和数据库访问能力。

前后端职责划分如下：

- `lab-safety-system`：后端项目，提供认证、授权、实验室安全管理和 PostgreSQL 数据访问能力。
- `frontend/`：前端子模块，负责用户界面和前端交互。

## 功能范围

系统面向实验室安全管理场景，规划支持：

- 实验室基础信息管理
- 实验室安全管理
- 用户、角色和权限管理
- SSO 单点登录
- OAuth 第三方授权登录
- 账号密码登录
- PostgreSQL 数据库存储

## 技术约束

- 数据库：PostgreSQL
- 后端职责：认证、授权、业务 API、数据持久化
- 前端职责：通过 `frontend/` 子模块独立维护

## Docker 部署

当前后端运行时代码尚未实现，本仓库提供 Docker 基础配置，方便其他系统提前接入镜像构建、环境变量和 PostgreSQL 编排流程。后续接入真实后端框架时，只需要在 `Dockerfile` 中替换构建步骤和启动命令。

准备环境变量：

```bash
cp .env.example .env
```

构建后端镜像：

```bash
docker build -t lab-safety-system-backend:latest .
```

构建前端镜像：

```bash
docker build -t lab-safety-system-frontend:latest ./frontend
```

使用 Docker Compose 启动前端、后端容器和 PostgreSQL：

```bash
docker compose up -d --build
```

查看服务状态：

```bash
docker compose ps
```

停止服务：

```bash
docker compose down
```

默认数据库连接信息通过 `.env` 配置：

- `POSTGRES_DB`
- `POSTGRES_USER`
- `POSTGRES_PASSWORD`
- `DATABASE_URL`
- `SECRET_KEY`
- `UPLOAD_DIR`
- `SSO_ENABLED`
- `OAUTH_ENABLED`

默认端口：

- 后端 API：`http://localhost:8080/api/v1`
- 前端界面：`http://localhost:8081`

## 子模块使用

首次克隆本仓库时，使用以下命令同时拉取前端子模块：

```bash
git clone --recurse-submodules https://github.com/LIghtJUNction/lab-safety-system.git
```

如果已经克隆了主仓库，可以在仓库根目录执行：

```bash
git submodule update --init --recursive
```

更新前端子模块到其远端最新提交：

```bash
git submodule update --remote frontend
```

## 开发说明

后端项目应围绕实验室安全管理业务建模，并通过 PostgreSQL 保存系统数据。认证入口需要兼容 SSO、OAuth 和传统账号密码登录，便于对接学校、企业或实验室已有身份系统。
