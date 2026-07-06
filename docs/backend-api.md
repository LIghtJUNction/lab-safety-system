# 后端功能与前端对接指南

本文档描述 Rust 后端当前已经提供的能力，以及前端如何对接。后端是本项目的主工程，前端以 `frontend/` Git 子模块维护。

## 基本约定

- API 前缀：`/api/v1`
- 数据格式：除文件上传外，请求和响应均为 JSON
- 鉴权方式：受保护接口使用 `Authorization: Bearer <access_token>`
- 错误格式：`{ "detail": "错误说明" }`
- 时间格式：`DateTime<Utc>` 字段使用 ISO 8601 字符串，日期字段使用 `YYYY-MM-DD`
- 上传文件：使用 `multipart/form-data`，文件字段名为 `file`

## 后端提供的功能

- 认证登录：账号密码登录、Passkey 登录、SSO 回调、OAuth 回调
- 会话校验：获取当前登录用户信息
- 用户管理：管理员通过 API 创建和查询普通用户；超级管理员通过命令行管理用户
- 实验室安全隐患管理：隐患上报、问题照片上传、责任认领、整改照片上传、整改提交、状态流转、统计分析
- 法规条例：列表、创建、文件上传
- 事故案例：列表、创建、文件上传
- 安全培训和考核：培训列表/创建、考试结果列表/创建
- 设备和预约：设备列表/创建、预约列表/创建
- 报修工单：报修列表/创建、管理员更新状态
- 数据统计：综合仪表盘、事故统计、隐患统计
- 健康检查：容器和数据库可用性检查

## 角色与权限

当前角色有三类：

| 角色 | 来源 | 说明 |
| --- | --- | --- |
| `super_admin` | 只能通过命令行创建和维护 | 超级管理员，可以执行命令行用户管理；API 中按管理员权限访问 |
| `admin` | 命令行或管理员 API 创建 | 管理员，可以访问管理端功能 |
| `researcher` | 命令行、管理员 API、SSO/OAuth 自动创建 | 普通用户，只能访问本人相关安全任务 |

权限规则：

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

前端应根据 `GET /auth/me` 或登录响应中的 `user.role` 切换界面：

- `admin` / `super_admin`：进入管理端，显示用户、法规、事故、培训、设备、隐患、报修和统计。
- `researcher`：进入普通用户端，只显示本人隐患、整改任务、培训考核、预约和报修。

## 认证流程

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

前端登录页应先调用此接口：

- `password=true` 时显示账号密码登录。
- `sso=true` 且 `sso_login_url` 非空时显示 SSO 按钮，并跳转该 URL。
- `oauth=true` 且 `oauth_login_url` 非空时显示 OAuth 按钮，并跳转该 URL。

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

前端拿到 `access_token` 后，应保存到内存或安全的会话存储，并为后续请求添加：

```http
Authorization: Bearer jwt-token
```

### 获取当前用户

```http
GET /api/v1/auth/me
Authorization: Bearer <access_token>
```

响应字段同登录响应里的 `user`。

### SSO / OAuth 回调

后端提供两个回调地址：

- `GET /api/v1/auth/sso/callback`
- `GET /api/v1/auth/oauth/callback`

身份提供方或上游网关需要带上以下 query 参数：

| 参数 | 必填 | 说明 |
| --- | --- | --- |
| `username` | 是 | 系统内用户名 |
| `email` | 是 | 邮箱 |
| `display_name` | 否 | 显示名，缺省为 `username` |
| `role` | 否 | `admin` 或 `researcher`，缺省为 `researcher` |
| `department` | 否 | 部门 |
| `exp` | 是 | Unix 秒级过期时间 |
| `sig` | 是 | HMAC-SHA256 签名 |
| `redirect` | 否 | 登录成功后跳回的前端本地路径，必须是 `/` 开头的站内路径 |

签名原文为 7 行换行拼接：

```text
provider
username
email
display_name
role
department
exp
```

其中 `provider` 是 `sso` 或 `oauth`。签名使用 `FEDERATED_LOGIN_SECRET` 做 HMAC-SHA256，再用 base64url no padding 编码。

回调成功后，后端会自动创建或更新用户，并把会话放到前端 hash：

```text
/#session=<base64url-json>
```

前端需要在启动时解析 `location.hash`：

1. 如果存在 `session=`，base64url 解码成 `AuthSession`。
2. 保存 `access_token`。
3. 清理 URL hash，避免 token 长时间留在地址栏。
4. 根据 `session.user.role` 切换管理端或普通用户端。

联邦登录不能创建 `super_admin`。超级管理员只能通过命令行创建。

### Passkey 登录

Passkey 登录分两步：

```http
POST /api/v1/auth/passkey/login/start
Content-Type: application/json

{ "username": "researcher01" }
```

响应：

```json
{
  "challenge_id": "uuid",
  "options": {}
}
```

前端把 `options` 传给浏览器：

```ts
const credential = await navigator.credentials.get({ publicKey: options });
```

然后提交：

```http
POST /api/v1/auth/passkey/login/finish
Content-Type: application/json

{
  "challenge_id": "uuid",
  "credential": {}
}
```

响应同账号密码登录。

### Passkey 绑定

绑定 Passkey 需要用户已经登录：

```http
POST /api/v1/auth/passkey/register/start
Authorization: Bearer <access_token>
```

前端拿到 `options` 后调用：

```ts
const credential = await navigator.credentials.create({ publicKey: options });
```

然后提交：

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

查询已绑定 Passkey：

```http
GET /api/v1/auth/passkeys
Authorization: Bearer <access_token>
```

生产环境必须正确配置：

```env
WEBAUTHN_RP_ID=lab.example.com
WEBAUTHN_ORIGIN=https://lab.example.com
```

## API 目录

### 健康检查

| 方法 | 路径 | 鉴权 | 说明 |
| --- | --- | --- | --- |
| GET | `/health` | 否 | 进程健康 |
| GET | `/ready` | 否 | 数据库连通性 |

### 认证

| 方法 | 路径 | 鉴权 | 说明 |
| --- | --- | --- | --- |
| GET | `/auth/methods` | 否 | 查询可用登录方式 |
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

| 方法 | 路径 | 鉴权 | 权限 | 说明 |
| --- | --- | --- | --- | --- |
| GET | `/users` | 是 | 管理员 | 查询用户 |
| POST | `/users` | 是 | 管理员 | 创建 `admin` 或 `researcher` |

查询参数：`q`、`role`、`limit`、`offset`。

创建用户请求：

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

说明：

- API 只能创建 `admin` 和 `researcher`。
- `auth_provider=password` 时必须提供强密码。
- `auth_provider` 可为 `password`、`sso`、`oauth`。

### 法规条例

| 方法 | 路径 | 鉴权 | 权限 | 说明 |
| --- | --- | --- | --- | --- |
| GET | `/regulations` | 是 | 登录用户 | 查询法规 |
| POST | `/regulations` | 是 | 管理员 | 创建法规 |
| POST | `/regulations/upload` | 是 | 管理员 | 上传法规文件 |

查询参数：`q`、`limit`、`offset`。

创建请求：

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

| 方法 | 路径 | 鉴权 | 权限 | 说明 |
| --- | --- | --- | --- | --- |
| GET | `/incidents` | 是 | 登录用户 | 查询事故案例 |
| POST | `/incidents` | 是 | 管理员 | 创建事故案例 |
| POST | `/incidents/upload` | 是 | 管理员 | 上传事故附件 |

创建请求：

```json
{
  "title": "通风橱操作不当事故",
  "lab_name": "有机化学实验室",
  "occurred_on": "2026-05-10",
  "severity": "major",
  "category": "chemical",
  "root_cause": "未按规程开启通风设备",
  "corrective_actions": "重新培训并增加班前检查"
}
```

### 培训与考核

| 方法 | 路径 | 鉴权 | 权限 | 说明 |
| --- | --- | --- | --- | --- |
| GET | `/trainings` | 是 | 登录用户 | 查询培训 |
| POST | `/trainings` | 是 | 管理员 | 创建培训 |
| GET | `/exam-results` | 是 | 登录用户 | 查询考核结果；普通用户仅本人 |
| POST | `/exam-results` | 是 | 本人或管理员 | 创建考核结果 |

`GET /trainings` 查询参数：`status`、`limit`、`offset`。

创建培训请求：

```json
{
  "title": "化学品入门安全培训",
  "target_role": "researcher",
  "status": "published",
  "starts_on": "2026-07-01",
  "exam_required_score": 80
}
```

创建考核请求：

```json
{
  "training_id": 1,
  "user_id": 2,
  "score": 92,
  "status": "passed"
}
```

### 设备、预约与报修

| 方法 | 路径 | 鉴权 | 权限 | 说明 |
| --- | --- | --- | --- | --- |
| GET | `/equipment` | 是 | 登录用户 | 查询设备 |
| POST | `/equipment` | 是 | 管理员 | 创建设备 |
| GET | `/equipment-bookings` | 是 | 登录用户 | 查询预约；普通用户仅本人 |
| POST | `/equipment-bookings` | 是 | 本人或管理员 | 创建预约 |
| GET | `/repair-tickets` | 是 | 登录用户 | 查询报修；普通用户仅本人 |
| POST | `/repair-tickets` | 是 | 本人或管理员 | 创建报修 |
| PATCH | `/repair-tickets/{id}` | 是 | 管理员 | 更新报修状态 |

`GET /equipment` 查询参数：`q`、`status`、`limit`、`offset`。

创建设备请求：

```json
{
  "asset_code": "HPLC-001",
  "name": "高效液相色谱仪",
  "lab_name": "分析测试中心",
  "status": "available",
  "owner": "设备管理员"
}
```

创建预约请求：

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

创建报修请求：

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

| 方法 | 路径 | 鉴权 | 权限 | 说明 |
| --- | --- | --- | --- | --- |
| GET | `/hazards` | 是 | 登录用户 | 查询隐患；普通用户仅本人相关 |
| POST | `/hazards` | 是 | 本人或管理员 | 上报隐患 |
| POST | `/hazards/{id}/claim` | 是 | 本人或管理员 | 认领责任人 |
| POST | `/hazards/{id}/remediation` | 是 | 责任人或管理员 | 提交整改 |
| PATCH | `/hazards/{id}/status` | 是 | 管理员 | 更新状态 |
| POST | `/hazards/upload/issue-photo` | 是 | 登录用户 | 上传问题照片 |
| POST | `/hazards/upload/remediation-photo` | 是 | 登录用户 | 上传整改照片 |

`GET /hazards` 查询参数：

- `q`：标题或描述模糊搜索
- `status`：按状态过滤
- `responsible_user_id`：按责任人过滤
- `reported_by`：按上报人过滤
- `limit` / `offset`：分页

隐患状态由后端写入或管理员更新，当前常用值：

- `reported`：已上报，数据库默认值
- `claimed`：已认领
- `remediation_submitted`：已提交整改
- `closed`：已关闭

上报隐患请求：

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

认领请求：

```json
{
  "responsible_user_id": 2
}
```

提交整改请求：

```json
{
  "remediation_photo_url": "/uploads/hazards/remediation/example.jpg",
  "remediation_note": "已补充标签并复核。"
}
```

管理员更新状态：

```json
{
  "status": "closed"
}
```

### 文件上传

上传接口均使用：

```http
Content-Type: multipart/form-data
Authorization: Bearer <access_token>
```

字段名固定为 `file`。响应：

```json
{
  "filename": "photo.jpg",
  "content_type": "image/jpeg",
  "size": 102400,
  "url": "/uploads/hazards/issue/uuid-photo.jpg"
}
```

前端应把返回的 `url` 写入后续业务创建或整改提交请求。

### 统计

| 方法 | 路径 | 鉴权 | 权限 | 说明 |
| --- | --- | --- | --- | --- |
| GET | `/analytics/dashboard` | 是 | 登录用户 | 综合统计 |
| GET | `/analytics/incidents` | 是 | 登录用户 | 事故分类和严重程度统计 |
| GET | `/analytics/hazards` | 是 | 登录用户 | 隐患状态和分类统计；普通用户仅本人相关 |

`GET /analytics/dashboard` 响应：

```json
{
  "regulation_count": 10,
  "incident_count": 3,
  "training_count": 5,
  "equipment_count": 20,
  "open_repair_count": 2,
  "exam_pass_rate": 0.92
}
```

`GET /analytics/hazards` 响应：

```json
{
  "by_status": [{ "name": "reported", "count": 4 }],
  "by_category": [{ "name": "chemical", "count": 2 }]
}
```

## 前端对接建议

### API Client

前端建议统一封装请求函数：

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

  const response = await fetch(`${API_BASE}${path}`, {
    ...init,
    headers,
  });

  if (!response.ok) {
    const body = await response.json().catch(() => null);
    throw new Error(body?.detail ?? `Request failed: ${response.status}`);
  }

  return response.json() as Promise<T>;
}
```

### 启动顺序

1. 调用 `GET /auth/methods` 渲染登录入口。
2. 检查 URL hash 中是否有 `session=`，用于接收 SSO/OAuth 回调结果。
3. 如果已有本地会话 token，调用 `GET /auth/me` 校验。
4. 根据角色渲染管理端或普通用户端。

### 普通用户端核心流程

1. 上传问题照片：`POST /hazards/upload/issue-photo`
2. 上报隐患：`POST /hazards`
3. 查询本人相关隐患：`GET /hazards`
4. 认领责任：`POST /hazards/{id}/claim`
5. 上传整改照片：`POST /hazards/upload/remediation-photo`
6. 提交整改：`POST /hazards/{id}/remediation`

### 管理端核心流程

1. 查询统计：`GET /analytics/dashboard`、`GET /analytics/hazards`
2. 查询所有隐患：`GET /hazards`
3. 分配责任人：`POST /hazards/{id}/claim`
4. 审核后关闭：`PATCH /hazards/{id}/status`
5. 维护基础数据：法规、事故、培训、设备、用户

### 分页和过滤

支持分页的接口使用 `limit` 和 `offset`：

```http
GET /api/v1/hazards?status=reported&limit=20&offset=0
```

后端会把 `limit` 限制在 `1..100`，缺省为 `50`；`offset` 缺省为 `0`。

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

命令行用户管理是后端能力的一部分，仅超级管理员可用。

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
