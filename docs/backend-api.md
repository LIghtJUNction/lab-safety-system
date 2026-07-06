# 后端功能与前端对接指南

本文档描述 Rust 后端当前提供的能力，以及前端如何对接。主仓库是后端工程，前端以 `frontend/` Git 子模块维护。

## 基本约定

- API 前缀：`/api/v1`
- 数据格式：除文件上传外，请求和响应均为 JSON。
- 鉴权方式：受保护接口使用 `Authorization: Bearer <access_token>`。
- 错误格式：`{ "detail": "错误说明" }`。
- 时间格式：`DateTime<Utc>` 使用 ISO 8601 字符串，日期字段使用 `YYYY-MM-DD`。
- 上传文件：使用 `multipart/form-data`，文件字段名固定为 `file`。

## 后端能力

- 认证登录：账号密码、Passkey、SSO 签名回调、OAuth 签名回调。
- 用户管理：管理员通过 API 创建和查询用户；超级管理员通过命令行管理用户。
- 法规条例：附件上传、创建、查询、按类型和发布机构统计。
- 事故案例：附件上传、创建、查询、按分类和严重程度统计。
- 安全培训：培训创建/查询，考核结果创建/查询。
- 实验设备：设备创建/查询，预约创建/查询，报修创建/查询/状态更新。
- 安全隐患：问题照片上传、隐患上报、责任认领、整改照片上传、整改提交、管理员关闭、统计。
- 健康检查：进程健康和数据库可用性检查。

## 角色与权限

| 角色 | 说明 |
| --- | --- |
| `super_admin` | 只能通过命令行创建和维护；API 中按管理员权限访问。 |
| `admin` | 管理员，可以访问管理端功能。 |
| `researcher` | 普通用户，只能访问本人相关安全任务。 |

| 能力 | `super_admin` | `admin` | `researcher` |
| --- | --- | --- | --- |
| 登录系统 | 是 | 是 | 是 |
| 命令行用户管理 | 是 | 否 | 否 |
| API 创建/查询用户 | 是 | 是 | 否 |
| 创建法规、事故、培训、设备 | 是 | 是 | 否 |
| 查询基础数据 | 是 | 是 | 是 |
| 上报隐患 | 是 | 是 | 是，仅本人 |
| 认领隐患 | 是 | 是 | 是，仅认领给本人 |
| 提交整改 | 是 | 是 | 是，仅本人负责的隐患 |
| 更新隐患状态 | 是 | 是 | 否 |
| 更新报修状态 | 是 | 是 | 否 |
| 查看所有隐患/预约/报修/考核 | 是 | 是 | 否 |
| 查看本人隐患/预约/报修/考核 | 是 | 是 | 是 |

前端应根据登录响应或 `GET /auth/me` 的 `user.role` 切换界面：

- `admin` / `super_admin`：进入管理端。
- `researcher`：进入普通用户端。

## 认证接口

### 查询登录方式

```http
GET /api/v1/auth/methods
```

响应：

```json
{
  "password": true,
  "sso": false,
  "oauth": false,
  "sso_login_url": null,
  "oauth_login_url": null
}
```

### 账号密码登录

```http
POST /api/v1/auth/password-login
Content-Type: application/json

{
  "username": "admin",
  "password": "StrongPassword123!"
}
```

响应：

```json
{
  "access_token": "jwt-token",
  "token_type": "bearer",
  "expires_in": 3600,
  "user": {
    "id": 1,
    "username": "admin",
    "display_name": "超级管理员",
    "email": "admin@example.com",
    "role": "super_admin",
    "auth_provider": "password"
  }
}
```

### 当前用户

```http
GET /api/v1/auth/me
Authorization: Bearer <access_token>
```

### SSO / OAuth 回调

回调地址：

- `GET /api/v1/auth/sso/callback`
- `GET /api/v1/auth/oauth/callback`

上游身份系统需要传入 query 参数：

| 参数 | 必填 | 说明 |
| --- | --- | --- |
| `username` | 是 | 系统内用户名 |
| `email` | 是 | 邮箱 |
| `display_name` | 否 | 显示名，缺省为 `username` |
| `role` | 否 | `admin` 或 `researcher`，缺省为 `researcher` |
| `department` | 否 | 部门 |
| `exp` | 是 | Unix 秒级过期时间 |
| `sig` | 是 | HMAC-SHA256 签名 |
| `redirect` | 否 | 登录成功后的站内跳转路径，必须以 `/` 开头 |

签名原文：

```text
provider
username
email
display_name
role
department
exp
```

`provider` 为 `sso` 或 `oauth`。使用 `FEDERATED_LOGIN_SECRET` 做 HMAC-SHA256，并用 base64url no padding 编码。成功后，后端会把会话写入：

```text
/#session=<base64url-json>
```

前端应解析 hash 中的 `session`，保存 `access_token`，然后清理 URL hash。

### Passkey

Passkey 登录：

```http
POST /api/v1/auth/passkey/login/start
Content-Type: application/json

{ "username": "researcher01" }
```

前端将响应里的 `options` 传给 `navigator.credentials.get`，再调用：

```http
POST /api/v1/auth/passkey/login/finish
Content-Type: application/json

{
  "challenge_id": "uuid",
  "credential": {}
}
```

绑定 Passkey 需要已登录：

```http
POST /api/v1/auth/passkey/register/start
Authorization: Bearer <access_token>
```

前端将响应里的 `options` 传给 `navigator.credentials.create`，再调用：

```http
POST /api/v1/auth/passkey/register/finish
Authorization: Bearer <access_token>
Content-Type: application/json

{
  "challenge_id": "uuid",
  "name": "MacBook Passkey",
  "credential": {}
}
```

生产环境必须配置：

```env
WEBAUTHN_RP_ID=lab.example.com
WEBAUTHN_ORIGIN=https://lab.example.com
```

## API 目录

所有路径以下均省略 `/api/v1` 前缀。

### 健康检查

| 方法 | 路径 | 鉴权 | 说明 |
| --- | --- | --- | --- |
| GET | `/health` | 否 | 进程健康 |
| GET | `/ready` | 否 | 数据库连通性 |

### 认证

| 方法 | 路径 | 鉴权 | 说明 |
| --- | --- | --- | --- |
| GET | `/auth/methods` | 否 | 查询登录方式 |
| POST | `/auth/password-login` | 否 | 账号密码登录 |
| GET | `/auth/me` | 是 | 当前用户 |
| GET | `/auth/sso/callback` | 否 | SSO 签名回调 |
| GET | `/auth/oauth/callback` | 否 | OAuth 签名回调 |
| POST | `/auth/passkey/login/start` | 否 | 开始 Passkey 登录 |
| POST | `/auth/passkey/login/finish` | 否 | 完成 Passkey 登录 |
| POST | `/auth/passkey/register/start` | 是 | 开始绑定 Passkey |
| POST | `/auth/passkey/register/finish` | 是 | 完成绑定 Passkey |
| GET | `/auth/passkeys` | 是 | 查询本人 Passkey |

### 用户

| 方法 | 路径 | 权限 | 说明 |
| --- | --- | --- | --- |
| GET | `/users` | 管理员 | 查询用户，支持 `q`、`role`、`limit`、`offset` |
| POST | `/users` | 管理员 | 创建 `admin` 或 `researcher` |

创建用户：

```json
{
  "username": "researcher01",
  "display_name": "研究员01",
  "email": "researcher01@example.com",
  "role": "researcher",
  "auth_provider": "password",
  "department": "化学实验室",
  "password": "StrongResearcher123!"
}
```

API 只能创建 `admin` 和 `researcher`。`super_admin` 只能通过命令行创建。

### 法规条例

| 方法 | 路径 | 权限 | 说明 |
| --- | --- | --- | --- |
| GET | `/regulations` | 登录用户 | 查询法规，支持 `q`、`limit`、`offset` |
| POST | `/regulations` | 管理员 | 创建法规 |
| POST | `/regulations/upload` | 管理员 | 上传法规文件 |

创建法规：

```json
{
  "title": "危险化学品安全管理条例",
  "regulation_type": "国家法规",
  "issuing_authority": "国务院",
  "effective_date": "2026-01-01",
  "summary": "危险化学品采购、储存、使用和处置要求。",
  "file_url": "/uploads/regulations/example.pdf"
}
```

### 事故案例

| 方法 | 路径 | 权限 | 说明 |
| --- | --- | --- | --- |
| GET | `/incidents` | 登录用户 | 查询事故案例，支持 `q`、`limit`、`offset` |
| POST | `/incidents` | 管理员 | 创建事故案例 |
| POST | `/incidents/upload` | 管理员 | 上传事故附件 |

创建事故案例：

```json
{
  "title": "通风橱操作不当事故",
  "lab_name": "有机化学实验室",
  "occurred_on": "2026-05-10",
  "severity": "major",
  "category": "chemical",
  "root_cause": "未按规程开启通风设备",
  "corrective_actions": "重新培训并增加班前检查",
  "file_url": "/uploads/incidents/example.pdf"
}
```

### 培训与考核

| 方法 | 路径 | 权限 | 说明 |
| --- | --- | --- | --- |
| GET | `/trainings` | 登录用户 | 查询培训，支持 `status`、`limit`、`offset` |
| POST | `/trainings` | 管理员 | 创建培训 |
| GET | `/exam-results` | 登录用户 | 查询考核结果；普通用户仅本人 |
| POST | `/exam-results` | 本人或管理员 | 创建考核结果 |

创建培训：

```json
{
  "title": "化学品入门安全培训",
  "target_role": "researcher",
  "status": "published",
  "starts_on": "2026-07-01",
  "exam_required_score": 80
}
```

创建考核结果：

```json
{
  "training_id": 1,
  "user_id": 2,
  "score": 92,
  "status": "passed"
}
```

### 设备、预约与报修

| 方法 | 路径 | 权限 | 说明 |
| --- | --- | --- | --- |
| GET | `/equipment` | 登录用户 | 查询设备，支持 `q`、`status`、`limit`、`offset` |
| POST | `/equipment` | 管理员 | 创建设备 |
| GET | `/equipment-bookings` | 登录用户 | 查询预约；普通用户仅本人 |
| POST | `/equipment-bookings` | 本人或管理员 | 创建预约 |
| GET | `/repair-tickets` | 登录用户 | 查询报修；普通用户仅本人 |
| POST | `/repair-tickets` | 本人或管理员 | 创建报修 |
| PATCH | `/repair-tickets/{id}` | 管理员 | 更新报修状态 |

创建设备：

```json
{
  "asset_code": "HPLC-001",
  "name": "高效液相色谱仪",
  "lab_name": "分析测试中心",
  "status": "available",
  "owner": "设备管理员"
}
```

创建预约：

```json
{
  "equipment_id": 1,
  "user_id": 2,
  "starts_at": "2026-07-10T02:00:00Z",
  "ends_at": "2026-07-10T04:00:00Z",
  "purpose": "样品检测"
}
```

后端会拒绝结束时间早于开始时间、以及同一设备时间重叠的预约。

创建报修：

```json
{
  "equipment_id": 1,
  "reported_by": 2,
  "description": "泵压异常",
  "status": "open"
}
```

更新报修状态：

```json
{
  "status": "closed"
}
```

### 安全隐患

| 方法 | 路径 | 权限 | 说明 |
| --- | --- | --- | --- |
| GET | `/hazards` | 登录用户 | 查询隐患；普通用户仅本人相关 |
| POST | `/hazards` | 本人或管理员 | 上报隐患 |
| POST | `/hazards/{id}/claim` | 本人或管理员 | 认领责任人 |
| POST | `/hazards/{id}/remediation` | 责任人或管理员 | 提交整改 |
| PATCH | `/hazards/{id}/status` | 管理员 | 更新状态 |
| POST | `/hazards/upload/issue-photo` | 登录用户 | 上传问题照片 |
| POST | `/hazards/upload/remediation-photo` | 登录用户 | 上传整改照片 |

`GET /hazards` 支持 `q`、`status`、`responsible_user_id`、`reported_by`、`limit`、`offset`。

上报隐患：

```json
{
  "title": "试剂柜标签缺失",
  "lab_name": "有机化学实验室",
  "category": "chemical",
  "description": "三号试剂柜部分瓶体缺少中文标签。",
  "reported_by": 2,
  "issue_photo_url": "/uploads/hazards/issue/example.jpg"
}
```

认领：

```json
{
  "responsible_user_id": 2
}
```

提交整改：

```json
{
  "remediation_photo_url": "/uploads/hazards/remediation/example.jpg",
  "remediation_note": "已补充标签并复核。"
}
```

管理员关闭：

```json
{
  "status": "closed"
}
```

### 文件上传

上传接口使用 `multipart/form-data`，字段名为 `file`。响应：

```json
{
  "filename": "photo.jpg",
  "content_type": "image/jpeg",
  "size": 102400,
  "url": "/uploads/hazards/issue/uuid-photo.jpg"
}
```

前端应把返回的 `url` 写入后续业务创建或整改请求。

### 统计

| 方法 | 路径 | 权限 | 说明 |
| --- | --- | --- | --- |
| GET | `/analytics/dashboard` | 登录用户 | 综合统计 |
| GET | `/analytics/regulations` | 登录用户 | 法规类型和发布机构统计 |
| GET | `/analytics/incidents` | 登录用户 | 事故分类和严重程度统计 |
| GET | `/analytics/hazards` | 登录用户 | 隐患状态和分类统计；普通用户仅本人相关 |

法规统计响应：

```json
{
  "by_type": [{ "name": "国家法规", "count": 4 }],
  "by_authority": [{ "name": "国务院", "count": 2 }]
}
```

事故统计响应：

```json
{
  "by_category": [{ "name": "chemical", "count": 2 }],
  "by_severity": [{ "name": "major", "count": 1 }]
}
```

隐患统计响应：

```json
{
  "by_status": [{ "name": "reported", "count": 4 }],
  "by_category": [{ "name": "chemical", "count": 2 }]
}
```

## 前端对接建议

### 启动顺序

1. 调用 `GET /auth/methods` 渲染登录入口。
2. 检查 URL hash 中是否有 `session=`，用于接收 SSO/OAuth 回调结果。
3. 如果已有本地 token，调用 `GET /auth/me` 校验。
4. 根据角色渲染管理端或普通用户端。

### 普通用户核心流程

1. 上传问题照片：`POST /hazards/upload/issue-photo`
2. 上报隐患：`POST /hazards`
3. 查询本人相关隐患：`GET /hazards`
4. 认领责任：`POST /hazards/{id}/claim`
5. 上传整改照片：`POST /hazards/upload/remediation-photo`
6. 提交整改：`POST /hazards/{id}/remediation`

### 管理端核心流程

1. 查询统计：`GET /analytics/dashboard`、`GET /analytics/regulations`、`GET /analytics/incidents`、`GET /analytics/hazards`
2. 查询所有隐患：`GET /hazards`
3. 分配责任人：`POST /hazards/{id}/claim`
4. 审核后关闭：`PATCH /hazards/{id}/status`
5. 维护用户、法规、事故、培训、设备基础数据

### API Client 示例

```ts
const API_BASE = "/api/v1";
let accessToken: string | null = null;

async function request<T>(path: string, init?: RequestInit): Promise<T> {
  const headers = new Headers(
    init?.body instanceof FormData
      ? undefined
      : { "Content-Type": "application/json" },
  );

  if (accessToken) {
    headers.set("Authorization", `Bearer ${accessToken}`);
  }

  const response = await fetch(`${API_BASE}${path}`, { ...init, headers });
  if (!response.ok) {
    const body = await response.json().catch(() => null);
    throw new Error(body?.detail ?? `Request failed: ${response.status}`);
  }
  return response.json() as Promise<T>;
}
```

## 后端环境变量

| 变量 | 说明 |
| --- | --- |
| `APP_HOST` | 监听地址，默认 `0.0.0.0` |
| `APP_PORT` | 监听端口，默认 `8080` |
| `DATABASE_URL` | PostgreSQL 连接串 |
| `SECRET_KEY` | JWT 签名密钥，生产必须替换 |
| `TOKEN_TTL_SECONDS` | 访问令牌有效期，默认 `3600` |
| `UPLOAD_DIR` | 上传文件目录，默认 `/app/uploads` |
| `STATIC_DIR` | 静态前端目录；整合镜像使用 |
| `SSO_ENABLED` | 是否启用 SSO |
| `SSO_LOGIN_URL` | SSO 登录入口 |
| `OAUTH_ENABLED` | 是否启用 OAuth |
| `OAUTH_LOGIN_URL` | OAuth 登录入口 |
| `FEDERATED_LOGIN_SECRET` | SSO/OAuth 回调签名密钥 |
| `WEBAUTHN_RP_ID` | Passkey RP ID |
| `WEBAUTHN_ORIGIN` | Passkey Origin |

## 命令行用户管理

首次部署创建超级管理员：

```bash
lab-safety-system users bootstrap-super-admin \
  --username admin \
  --password 'StrongAdminPass2026!' \
  --email admin@example.com \
  --display-name 超级管理员
```

后续创建用户：

```bash
lab-safety-system users create \
  --actor admin \
  --actor-password '超级管理员强密码' \
  --username researcher01 \
  --password 'StrongResearcherPass2026!' \
  --email researcher01@example.com \
  --role researcher \
  --display-name 研究员01
```

重置并生成强密码：

```bash
lab-safety-system users set-password \
  --actor admin \
  --actor-password '超级管理员强密码' \
  --username researcher01 \
  --generate-password true
```

密码策略由后端强制校验：至少 12 位，并包含小写字母、大写字母、数字和符号。
