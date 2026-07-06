# 后端功能与前端对接指南

本文档描述 Rust 后端当前提供的 API。主仓库是后端工程，前端以 `frontend/` Git 子模块维护。

## 基本约定

- API 前缀：`/api/v1`
- 数据格式：除文件上传外，请求和响应均为 JSON。
- 鉴权方式：受保护接口使用 `Authorization: Bearer <access_token>`。
- 错误格式：`{ "detail": "错误说明" }`。
- 时间格式：`DateTime<Utc>` 使用 ISO 8601 字符串，日期字段使用 `YYYY-MM-DD`。
- 上传文件：使用 `multipart/form-data`，文件字段名固定为 `file`。

## 角色模型

系统按“多实验室 + 实验室内角色”设计。

| 角色值 | 中文含义 | 作用范围 | 说明 |
| --- | --- | --- | --- |
| `system_admin` | 实验室管理系统管理员 | 全局 | 唯一账号，维护系统、实验室、用户和全局数据。只能通过 CLI 初始化。 |
| `lab_admin` | 实验室管理员 | 单个实验室 | 管理自己实验室的成员、设备、事故、维修和隐患闭环。 |
| `lab_member` | 实验室成员 | 单个实验室 | 可上报隐患、预约设备、提交维修和处理自己负责的整改。 |
| `visitor` | 访客 | 单个实验室 | 最小权限，主要用于只读访问。 |

实现方式：

- `users.role` 保存全局角色。系统管理员是 `system_admin`；普通账号默认是 `lab_member` 或 `visitor`。
- `lab_users.lab_role` 保存用户在某个实验室里的角色，可为 `lab_admin`、`lab_member`、`visitor`。
- 同一个用户可以在多个实验室拥有不同角色。
- 后端兼容旧库中的 `super_admin` 读取，但新数据应使用 `system_admin`。

前端对接流程（推荐）：

1. 登录后立即调用 `GET /api/v1/auth/me` 获取全局 `role`。
2. 调用 `GET /api/v1/auth/my-labs` 获取用户可访问的实验室列表及在每个实验室的 `lab_role`。
3. 前端保存“当前选中实验室”。
4. 根据「全局 role + 当前实验室 lab_role」动态渲染菜单、按钮和操作权限。
5. `system_admin` 可看到实验室管理、用户管理等系统入口，并可操作任意实验室数据。
6. 普通角色严格限定在本实验室内。

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

```http
GET /api/v1/auth/me
Authorization: Bearer <access_token>
```

SSO / OAuth 回调地址：

- `GET /api/v1/auth/sso/callback`
- `GET /api/v1/auth/oauth/callback`

上游身份系统传入 `username`、`email`、`display_name`、`role`、`department`、`exp`、`sig`。`role` 只能是 `lab_member` 或 `visitor`，不能通过联邦登录创建系统管理员。

## 用户管理

| 方法 | 路径 | 权限 | 说明 |
| --- | --- | --- | --- |
| `GET` | `/users` | `system_admin`，或任意实验室 `lab_admin` | 查询用户，支持 `q`、`role`、`limit`、`offset` |
| `POST` | `/users` | `system_admin` | 创建普通账号，角色只能是 `lab_member` 或 `visitor` |

```json
{
  "username": "member01",
  "display_name": "实验室成员01",
  "email": "member01@example.com",
  "role": "lab_member",
  "auth_provider": "password",
  "department": "化学学院",
  "password": "StrongMember123!"
}
```

## 实验室管理

| 方法 | 路径 | 权限 | 说明 |
| --- | --- | --- | --- |
| `GET` | `/labs` | 登录用户 | `system_admin` 返回全部；其他用户只返回自己有关联角色的实验室。支持 `q`、`status`、`limit`、`offset` |
| `POST` | `/labs` | `system_admin` | 创建实验室 |
| `GET` | `/labs/{id}` | 实验室可访问用户 | 查询实验室详情 |
| `PATCH` | `/labs/{id}` | `system_admin` 或该实验室 `lab_admin` | 更新实验室 |
| `GET` | `/labs/{id}/users` | 实验室可访问用户 | 查询实验室成员关系 |
| `POST` | `/labs/{id}/users` | `system_admin` 或该实验室 `lab_admin` | 分配或更新实验室内角色 |

```json
{
  "code": "LAB-CHEM-001",
  "name": "有机化学实验室",
  "location": "实验楼A-302",
  "department": "化学学院",
  "manager_user_id": 2,
  "contact": "lab@example.com",
  "status": "active",
  "description": "有机合成和试剂暂存实验室"
}
```

实验室状态：`active`、`inactive`、`maintenance`。

分配实验室角色：

```json
{
  "user_id": 2,
  "lab_role": "lab_admin"
}
```

## 安全隐患

| 方法 | 路径 | 权限 | 说明 |
| --- | --- | --- | --- |
| `GET` | `/hazards` | 登录用户 | 支持 `lab_id`、`q`、`status`、`responsible_user_id`、`reported_by`、`limit`、`offset` |
| `POST` | `/hazards` | `system_admin`、该实验室 `lab_admin`、该实验室 `lab_member` | 上报隐患 |
| `POST` | `/hazards/{id}/claim` | `system_admin`、该实验室 `lab_admin`、本人认领 | 认领责任人 |
| `POST` | `/hazards/{id}/remediation` | `system_admin`、该实验室 `lab_admin`、责任人 | 提交整改 |
| `PATCH` | `/hazards/{id}/status` | `system_admin` 或该实验室 `lab_admin` | 更新状态 |
| `POST` | `/hazards/upload/issue-photo` | 登录用户 | 上传问题照片 |
| `POST` | `/hazards/upload/remediation-photo` | 登录用户 | 上传整改照片 |

```json
{
  "title": "试剂柜标签缺失",
  "lab_id": 1,
  "category": "chemical",
  "description": "三号试剂柜部分瓶体缺少中文标签。",
  "reported_by": 2,
  "issue_photo_url": "/uploads/hazards/issue/example.jpg"
}
```

## 事故案例

| 方法 | 路径 | 权限 | 说明 |
| --- | --- | --- | --- |
| `GET` | `/incidents` | 登录用户 | 支持 `lab_id`、`q`、`limit`、`offset`；非系统管理员只返回自己可访问实验室的数据。 |
| `POST` | `/incidents` | `system_admin` 或该实验室 `lab_admin` | 创建事故案例。 |
| `POST` | `/incidents/upload` | `system_admin` | 上传事故附件。 |

```json
{
  "title": "通风橱操作不当事故",
  "lab_id": 1,
  "occurred_on": "2026-05-10",
  "severity": "major",
  "category": "chemical",
  "root_cause": "未按规程开启通风设备",
  "corrective_actions": "重新培训并增加班前检查",
  "file_url": "/uploads/incidents/example.txt"
}
```

## 设备、预约和维修

| 方法 | 路径 | 权限 | 说明 |
| --- | --- | --- | --- |
| `GET` | `/equipment` | 登录用户 | 支持 `lab_id`、`q`、`status`、`limit`、`offset`；非系统管理员只返回可访问实验室的设备。 |
| `POST` | `/equipment` | `system_admin` 或该实验室 `lab_admin` | 创建设备。 |
| `GET` | `/equipment-bookings` | 登录用户 | 支持 `lab_id`；系统管理员返回全部，其他用户返回本人或可访问实验室的预约。 |
| `POST` | `/equipment-bookings` | 该实验室 `lab_admin` 或 `lab_member` | 创建设备预约，访客不可预约。 |
| `GET` | `/repair-tickets` | 登录用户 | 支持 `lab_id`；系统管理员返回全部，其他用户返回本人或可访问实验室的维修单。 |
| `POST` | `/repair-tickets` | 该实验室 `lab_admin` 或 `lab_member` | 提交维修单，访客不可提交。 |
| `PATCH` | `/repair-tickets/{id}` | `system_admin` 或该实验室 `lab_admin` | 更新维修单状态。 |

```json
{
  "asset_code": "HPLC-001",
  "name": "高效液相色谱仪",
  "lab_id": 1,
  "status": "available",
  "owner": "设备管理员"
}
```

## 法规和培训

- 法规条例仍是全局资料：`GET/POST /regulations`，`POST /regulations/upload`。
- 培训考核仍是全局资料：`GET/POST /trainings`，`GET/POST /exam-results`。

## 前端对接建议

1. 登录后调用 `GET /auth/me` 获取全局角色。
2. 调用 `GET /labs` 获取当前用户可访问的实验室列表。
3. 进入业务页面前，让用户选择当前实验室，并把 `lab_id` 放入隐患、事故、设备、预约和维修查询。
4. 调用 `GET /labs/{id}/users` 判断当前用户在实验室里的 `lab_role`。
5. 按角色渲染界面：
   - `system_admin`：系统维护、实验室管理、用户管理、所有实验室数据。
   - `lab_admin`：本实验室信息、成员、设备、事故、维修和隐患闭环。
   - `lab_member`：本实验室隐患上报、预约设备、提交维修和处理自己负责的整改。
   - `visitor`：只读或最小操作入口。

## 命令行用户管理

首次部署创建唯一系统管理员：

```bash
lab-safety-system users bootstrap-super-admin --generate-password true
```

创建普通用户：

```bash
lab-safety-system users create \
  --actor admin \
  --actor-password '系统管理员强密码' \
  --username member01 \
  --password 'MemberStrong123!' \
  --email member01@example.com \
  --role lab_member \
  --display-name 实验室成员01
```

重置密码：

```bash
lab-safety-system users set-password \
  --actor admin \
  --actor-password '系统管理员强密码' \
  --username member01 \
  --generate-password true
```
