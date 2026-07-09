# 后端功能与前端对接指南

本文档描述 Rust 后端当前提供的 API（多实验室 + 实验室内角色模型）。主仓库是后端工程，前端以 `frontend/` 目录维护。

## 基本约定

- API 前缀：`/api/v1`
- 数据格式：除文件上传外，请求和响应均为 JSON。
- 鉴权方式：受保护接口使用 `Authorization: Bearer <access_token>`。
- 错误格式：`{ "detail": "错误说明" }`（403 表示权限不足，404 资源不存在，400 参数错误）。
- 时间格式：`DateTime<Utc>` 使用 ISO 8601 字符串，日期字段使用 `YYYY-MM-DD`。
- 上传文件：使用 `multipart/form-data`，文件字段名固定为 `file`。
- 列表接口统一支持分页：`limit`、`offset`；搜索 `q`；多数支持 `lab_id` 过滤。

## 角色模型

系统按“多实验室 + 实验室内角色”设计。

| 角色值         | 中文含义             | 作用范围   | 说明 |
|----------------|----------------------|------------|------|
| `system_admin` | 实验室管理系统管理员 | 全局       | 唯一账号，维护系统、实验室、用户和全局数据。只能通过 CLI 初始化。 |
| `lab_admin`    | 实验室管理员         | 单个实验室 | 管理本实验室成员、设备、事故、维修、隐患闭环。 |
| `lab_member`   | 实验室成员           | 单个实验室 | 可上报隐患、预约设备、提交维修、处理自己负责的整改。 |
| `visitor`      | 访客                 | 单个实验室 | 最小权限，主要用于只读访问。 |

实现方式：

- `users.role` 保存**全局角色**（只有 `system_admin` 拥有跨实验室能力）。
- `lab_users.lab_role` 保存用户在**某个实验室**里的角色（`lab_admin` / `lab_member` / `visitor`）。
- 同一个用户可同时属于多个实验室，拥有不同 `lab_role`。
- 后端兼容旧的 `super_admin`，但新系统使用 `system_admin`。

**关键接口组合（前端必须先调）：**

1. `GET /api/v1/auth/me` → 获取全局 `role`
2. `GET /api/v1/auth/my-labs` → 获取可访问实验室 + **每个实验室的 role**

前端用「全局 role + 当前选中 lab 的 role」来决定页面、菜单、按钮和能力。

## 登录与当前用户

```http
GET /api/v1/auth/methods
```

```http
POST /api/v1/auth/password-login
Content-Type: application/json

{
  "username": "admin",
  "password": "StrongPassword123!"
}
```

响应示例（AuthSession）：

```json
{
  "access_token": "...",
  "token_type": "bearer",
  "expires_in": 3600,
  "user": {
    "id": 1,
    "username": "admin",
    "display_name": "实验室管理系统管理员",
    "email": "admin@example.com",
    "role": "system_admin",
    "auth_provider": "password"
  }
}
```

```http
GET /api/v1/auth/me
Authorization: Bearer <access_token>
```

返回 `AuthUser`（同上 user 字段）。

SSO / OAuth 回调：

- `GET /api/v1/auth/sso/callback`
- `GET /api/v1/auth/oauth/callback`

## 获取我的实验室与角色（核心！）

登录成功后**必须立即调用**：

```http
GET /api/v1/auth/my-labs
Authorization: Bearer <access_token>
```

响应：`LabMembership[]`

```json
[
  {
    "lab_id": 1,
    "lab_name": "有机化学实验室",
    "role": "lab_admin"
  },
  {
    "lab_id": 2,
    "lab_name": "物理实验室",
    "role": "lab_member"
  }
]
```

**system_admin 特殊行为**：返回系统中**所有实验室**，每个的 `role` 都是 `"system_admin"`。

前端用法：

- 保存 `labMemberships`
- 计算 `currentLabRole = (user.role === 'system_admin') ? 'system_admin' : memberships.find(m => m.lab_id === selectedLabId)?.role`
- 没有选中实验室或无权限时 role 为 null

## 实验室管理（仅 system_admin 可创建，lab_admin 可查看自己）

| 方法 | 路径                    | 权限                          | 说明 |
|------|-------------------------|-------------------------------|------|
| GET  | `/api/v1/labs`          | 登录用户                      | `system_admin` 返回全部；其他人只返回自己有成员关系的实验室。支持 `?q=` `?status=` |
| POST | `/api/v1/labs`          | `system_admin`                | 创建实验室 |
| GET  | `/api/v1/labs/{id}`     | 有该实验室访问权的用户        | 详情 |
| PATCH| `/api/v1/labs/{id}`     | `system_admin` 或该 lab 的 `lab_admin` | 更新 |
| GET  | `/api/v1/labs/{id}/users` | 有该实验室访问权的用户     | 实验室成员列表（含全局角色） |
| POST | `/api/v1/labs/{id}/users` | `system_admin` 或该 lab 的 `lab_admin` | 分配/更新实验室内角色 |
| DELETE | `/api/v1/labs/{id}/users/{user_id}` | `system_admin` 或该 lab 的 `lab_admin` | 移除实验室成员关系 |

### Lab 对象（响应）

```json
{
  "id": 1,
  "code": "LAB-CHEM-001",
  "name": "有机化学实验室",
  "location": "实验楼A-302",
  "department": "化学学院",
  "manager_user_id": 3,
  "contact": "chem-lab@example.com",
  "status": "active",
  "description": "有机合成与试剂管理实验室",
  "created_at": "2026-01-15T08:00:00Z"
}
```

状态值：`active`、`inactive`、`maintenance`。

### 创建实验室（POST /api/v1/labs）

```json
{
  "code": "LAB-PHYS-002",
  "name": "物理实验室",
  "location": "实验楼B-105",
  "department": "物理学院",
  "manager_user_id": 5,
  "contact": "phys@example.com",
  "status": "active",
  "description": "力学与光学实验"
}
```

### 更新实验室（PATCH /api/v1/labs/1）

支持部分字段：

```json
{
  "name": "有机化学与高分子实验室",
  "status": "maintenance",
  "description": "暂停使用中"
}
```

### 实验室成员列表与分配

GET `/api/v1/labs/{id}/users` 返回：

```json
[
  {
    "id": 12,
    "lab_id": 1,
    "user_id": 5,
    "lab_role": "lab_admin",
    "username": "labadmin05",
    "display_name": "张管理员",
    "email": "z@example.com",
    "global_role": "lab_member",
    "created_at": "2026-03-01T10:00:00Z"
  }
]
```

注意：**global_role**（users.role）与 **lab_role**（lab_users.lab_role）要**分开显示**。

分配/修改角色（POST）：

```json
{
  "user_id": 7,
  "lab_role": "lab_member"
}
```

合法 lab_role：`lab_admin`、`lab_member`、`visitor`。

## 用户管理

| 方法 | 路径 | 权限 | 说明 |
|------|------|------|------|
| GET | `/api/v1/users` | `system_admin` 或任意实验室的 `lab_admin` | 查询用户，支持 `?q=` `?role=` `?lab_id=` 分页；`lab_admin` 只返回自己管理实验室内的成员 |
| POST | `/api/v1/users` | `system_admin` | 创建普通用户，role 只能是 `lab_member` 或 `visitor` |
| PATCH | `/api/v1/users/{id}` | `system_admin` | 更新普通用户资料、全局角色和启用状态 |

创建示例（仅全局管理员可用）：

```json
{
  "username": "member01",
  "display_name": "实验室成员01",
  "email": "m01@example.com",
  "role": "lab_member",
  "auth_provider": "password",
  "department": "化学学院",
  "password": "StrongMember123!"
}
```

更新示例：

```json
{
  "display_name": "实验室访客01",
  "email": "visitor01@example.com",
  "role": "visitor",
  "department": "化学学院",
  "is_active": false
}
```

`PATCH /users/{id}` 只能把普通用户角色设置为 `lab_member` 或 `visitor`。系统管理员账号的角色和启用状态必须通过 CLI 管理，防止破坏唯一系统管理员约束。

## 安全隐患（必须绑定实验室）

**关键变更**：隐患现在强制使用 `lab_id`（而非自由 lab_name）。

| 方法   | 路径                              | 权限（示例）                                      | 说明 |
|--------|-----------------------------------|---------------------------------------------------|------|
| GET    | `/api/v1/hazards?lab_id=1`        | 登录 + 有实验室访问权                             | 支持 `lab_id`、`q`、`status`、`responsible_user_id` 等过滤 |
| POST   | `/api/v1/hazards`                 | `system_admin` 或该实验室 `lab_admin`/`lab_member` | 上报隐患（必须带 lab_id） |
| POST   | `/api/v1/hazards/{id}/claim`      | 同上 + 本人或 lab_admin                           | 认领责任人 |
| POST   | `/api/v1/hazards/{id}/remediation`| 责任人 / lab_admin / system_admin                 | 提交整改 |
| PATCH  | `/api/v1/hazards/{id}/status`     | lab_admin 或 system_admin                         | 状态流转 |
| POST   | `/api/v1/hazards/upload/issue-photo`      | 登录用户 | 上传问题照片 |
| POST   | `/api/v1/hazards/upload/remediation-photo`| 登录用户 | 上传整改照片 |

### 创建隐患（必须传 lab_id）

```json
{
  "lab_id": 1,
  "title": "试剂柜标签缺失",
  "category": "chemical",
  "description": "三号试剂柜部分瓶体缺少中文标签。",
  "reported_by": 7,
  "issue_photo_url": "/uploads/hazards/issue/xxx.png"
}
```

**响应**（SafetyHazard）包含 `lab_id` + `lab_name`：

```json
{
  "id": 42,
  "lab_id": 1,
  "lab_name": "有机化学实验室",
  "title": "...",
  "category": "chemical",
  "description": "...",
  "status": "open",
  "reported_by": 7,
  "responsible_user_id": null,
  "issue_photo_url": "/uploads/...",
  "remediation_photo_url": null,
  "remediation_note": null,
  "created_at": "..."
}
```

列表查询推荐始终带 `?lab_id=当前选中实验室ID`。

### 隐患状态机（前后端统一）

| 状态 | 含义 | 如何进入 |
|------|------|----------|
| `open` | 已上报、待认领 | `POST /hazards` 创建默认；历史 `reported` 已迁移/别名为 `open` |
| `claimed` | 已认领责任人 | `POST /hazards/{id}/claim` |
| `remediation_submitted` | 已提交整改 | `POST /hazards/{id}/remediation` |
| `closed` | 已闭环 | `PATCH /hazards/{id}/status`（lab_admin / system_admin） |

`PATCH .../status` 仅允许上述集合；传入遗留值 `reported` 时会规范化为 `open`。

## 事故案例、设备、预约、维修

这些资源也支持 `lab_id` 过滤与权限收口：

- `GET/POST /api/v1/incidents`（支持 `?lab_id=`）
- `GET/POST /api/v1/equipment`（支持 `?lab_id=`）
- `GET/POST /api/v1/equipment-bookings`
- `GET/POST/PATCH /api/v1/repair-tickets`

创建时优先传 `lab_id`，后端会解析并校验权限。非 system_admin 时必须有对应实验室访问权。

示例（incident）：

```json
{
  "lab_id": 1,
  "title": "通风橱操作不当事故",
  "occurred_on": "2026-05-10",
  "severity": "major",
  "category": "chemical",
  "root_cause": "...",
  "corrective_actions": "..."
}
```

## 法规条例与培训（全局）

- 法规：`GET/POST /api/v1/regulations`、`POST /regulations/upload`（仅 system_admin 创建）
- 培训：`GET/POST /api/v1/trainings`、`GET/POST /api/v1/exam-results`（创建培训仅 system_admin）

## 统计分析

- `GET /api/v1/analytics/dashboard`
- `GET /api/v1/analytics/hazards`
- `GET /api/v1/analytics/incidents`
- `GET /api/v1/analytics/regulations`

统计接口支持 `?lab_id=当前选中实验室ID`。传入 `lab_id` 时会先校验当前用户是否可访问该实验室；不传时，`system_admin` 查看全部，非系统管理员按实验室成员关系和本人相关数据过滤可见范围。

## 前端对接核心流程与建议

### 推荐初始化流程（登录后）

```ts
const me = await api.me();
const memberships = await api.myLabMemberships();

const isSystemAdmin = me.role === 'system_admin';

// 选择实验室：记住上次、本地存储、或默认第一个
let selectedLabId = localStorage.getItem('selectedLabId') ? Number(...) : (memberships[0]?.lab_id ?? null);

if (isSystemAdmin && !selectedLabId) {
  const allLabs = await api.labs();
  // system_admin 可以看到全部实验室
}

// 计算当前角色
const currentLabRole = isSystemAdmin
  ? 'system_admin'
  : memberships.find(m => m.lab_id === selectedLabId)?.role ?? null;
```

### 权限判断（推荐实现，与后端一致）

```ts
const canManageSystem = (user: AuthUser) => user.role === 'system_admin';

const canManageLab = (user: AuthUser, labId: number | null, memberships: LabMembership[]) => {
  if (!user || !labId) return false;
  if (user.role === 'system_admin') return true;
  return memberships.some(m => m.lab_id === labId && m.role === 'lab_admin');
};

const canCreateHazard = (user: AuthUser, labId: number | null, memberships: LabMembership[]) => {
  if (!user || !labId) return false;
  if (user.role === 'system_admin') return true;
  return memberships.some(m => m.lab_id === labId && ['lab_admin', 'lab_member'].includes(m.role));
};

const canClaimOrRemediate = (user: AuthUser, labId: number | null, memberships: LabMembership[]) =>
  canCreateHazard(user, labId, memberships); // 简化示例，实际以后端 403 为准

const canViewLab = (user: AuthUser, labId: number | null, memberships: LabMembership[]) => {
  if (!user || !labId) return false;
  if (user.role === 'system_admin') return true;
  return memberships.some(m => m.lab_id === labId);
};
```

### 页面与数据调用规则

- 所有隐患、设备、事故、维修列表**必须**带当前 `lab_id`（`api.hazards(q, selectedLabId)`）。
- 创建隐患/设备/事故时**必须**传 `lab_id: selectedLabId`。
- system_admin 可访问“实验室管理”页面（GET/POST labs + labs/{id}/users）。
- 普通角色**不要**直接进入系统级页面。
- 按钮可隐藏，但遇到 403 必须提示“无权限操作”（不要假装成功）。
- 实验室切换后需要重新拉取对应 lab 的数据（hazards, members, stats 等）。
- 全局资源（法规、培训）system_admin 创建，普通用户只读。

### 动态导航与页面标题

根据 `currentLabRole` 决定可见菜单：

- `system_admin`：实验室管理、用户管理、全局概览、所有实验室数据
- `lab_admin`：本实验室概览、成员管理、隐患管理、设备、统计
- `lab_member`：上报隐患、我的整改、设备预约、培训
- `visitor`：只读列表

切换实验室时更新 URL（如 `/labs/${labId}/overview`）并持久化 `selectedLabId`。

## 错误处理建议

- 401/403：清除 token，提示重新登录或“无权限”。
- 404：资源不存在或无该实验室访问权。
- 后端返回 `detail` 字段，直接展示给用户。
- 网络错误：提示检查后端地址（前端支持登录页高级配置 apiBase）。

## 命令行用户与实验室管理

首次部署创建系统管理员：

```bash
lab-safety-system users bootstrap-super-admin --generate-password true
```

创建普通用户（需用 system_admin 身份）：

```bash
lab-safety-system users create \
  --actor admin \
  --actor-password '...' \
  --username member01 \
  --password '...' \
  --email ... \
  --role lab_member \
  --display-name "..."
```

实验室目前主要通过前端或直接 SQL/后端接口管理（system_admin）。

## 登录页轮播设置（自定义标题/副标题）

存于后端，全局生效。

```http
GET /api/v1/settings/login-carousel
```
（公开接口，登录页直接调用）

```http
PATCH /api/v1/settings/login-carousel
Authorization: Bearer <token>
Content-Type: application/json

{
  "zh": [{ "stat": "...", "title": "...", "body": "..." }, ...],
  "en": [...]
}
```

- 只有 `system_admin` 能 PATCH。
- 无记录时后端返回内置默认。
- 前端“全局配置”页提供编辑界面。

---

**总结**：前端必须围绕以下链路重构：

**当前用户 (me.role) → 可访问实验室列表 (my-labs) → 当前选中实验室 (selectedLabId) → 该实验室角色 (currentLabRole) → 带 lab_id 的接口调用 + 权限控制**

后端已按实验室和角色收口权限，前端必须严格传递 `lab_id` 并以 403 为最终权限依据。文档中的示例均为当前实际后端行为。
