<div align="center">

<img src="docs/brand/rushwind-icon.svg" alt="RushWind Admin" width="128">

# RushWind Admin

[![License](https://img.shields.io/badge/License-MIT-blue.svg)](./LICENSE)
[![Rust](https://img.shields.io/badge/Rust-1.81+-DEA584?logo=rust)](https://www.rust-lang.org/)
[![CI](https://github.com/tx7do/rushwind-admin/actions/workflows/ci.yml/badge.svg)](https://github.com/tx7do/rushwind-admin/actions/workflows/ci.yml)

[English](./README.en-US.md) | **中文** | [日本語](./README.ja-JP.md)

</div>

---

## 项目亮点

- **多前端适配**：同时提供 `Vue3 Vben`（Ant Design Vue）、`Vue3 Element Plus`、`React19 Antd` 三套前端，满足不同团队偏好
- **企业级 RBAC**：支持多租户、多角色、多部门、菜单/按钮/数据级权限控制
- **契约驱动的代码生成**：Protobuf 是唯一 API 契约——构建期确定性生成 203 条路由、441 条错误状态映射、198 个服务接口，零手写路由；前端 TypeScript 客户端三份字节相同
- **安全与等保合规**：按等保 2.0 技术要求内置 180 天审计日志留存归档、口令策略三件套、TOTP MFA、口令应用层加密、动态 RBAC 与多租户隔离，详见[安全与等保合规](#安全与等保合规)
- **生产就绪基座**：JWT RS256 鉴权、统一四字段错误信封、CORS、SSE 消息推送（规划）、异步任务调度（规划）
- **质量门禁**：fmt / clippy / test / 契约同步四道 CI 门（ubuntu + windows 矩阵），外加差分回归台架对全量路由自动 sweep 逐字节比对

## 演示地址

| 前端版本 | 演示地址 |
|---------|--------|
| Vue3 Vben | <https://vben.admin.gowind.cloud> |
| Vue3 Element Plus | <https://ele.admin.gowind.cloud> |
| React | <https://react.admin.gowind.cloud> |

- 后端 Swagger：<https://api.demo.admin.gowind.cloud/docs/>
- 默认账号密码：`admin` / `Abcd@1234`

---

## 界面预览

### 管理后台

三套前端与本仓后端零改动对接，以下界面截自本仓后端（REST :7788）与 React 版前端：

**登录** —— 图形验证码 + 租户编号（留空为平台登录），口令在应用层加密后传输

![登录](./docs/screenshots/console-login.png)

**仪表盘** —— 用户 / 角色 / 登录与操作审计实时统计，含登录趋势与占比分布

![仪表盘](./docs/screenshots/console-dashboard.png)

**用户管理** —— 组织树 + 高级查询，支持多角色、多部门、状态与主管配置

![用户管理](./docs/screenshots/console-users.png)

**菜单管理** —— 目录 / 菜单 / 按钮三类节点，权限标识与路由、组件路径一一对应

![菜单管理](./docs/screenshots/console-menus.png)

**操作审计日志** —— 操作类型、资源定位与请求 ID 全程留痕，成功失败均可追溯

![操作审计日志](./docs/screenshots/console-op-audit.png)

**在线用户** —— 会话与设备视图，支持强制下线

![在线用户](./docs/screenshots/console-online.png)

### API 文档

后端自带 API 文档服务（`server.yaml` 的 `enable_swagger` / `enable_redoc` 开关，原始规格见 `/q/openapi.yaml`），服务启动后即可访问，无需另行部署文档站点。

**Swagger UI** —— `/q/swagger-ui`：全量接口按服务分组，带鉴权入口，可直接在线调试

![Swagger UI](./docs/screenshots/swagger-ui.png)

**ReDoc** —— `/q/redoc`：三栏式接口文档，参数说明与请求 / 响应示例并列呈现

![ReDoc](./docs/screenshots/redoc.png)

---

## 技术栈

<table>
<tr><th>层级</th><th>技术</th></tr>
<tr><td><strong>后端框架</strong></td><td><code>Rust</code>（edition 2021 / MSRV 1.81） · <code>RushWind</code> 框架 · <code>axum 0.8</code> · <code>tokio</code></td></tr>
<tr><td><strong>契约链</strong></td><td><code>Protobuf</code> · <code>buf + protox</code> · <code>prost / prost-reflect / pbjson</code> · <code>rushwind-gen-http</code>（路由生成）</td></tr>
<tr><td><strong>存储层</strong></td><td><code>SeaORM</code> · <code>PostgreSQL</code> · <code>Redis</code>（接入中）</td></tr>
<tr><td><strong>认证授权</strong></td><td><code>JWT RS256</code>（rushwind-authn-jwt） · <code>RBAC</code>（规划）</td></tr>
<tr><td><strong>Vue Vben 版</strong></td><td><code>Vue 3</code> · <code>TypeScript</code> · <code>Vite</code> · <code>Ant Design Vue</code> · <code>Vben Admin</code></td></tr>
<tr><td><strong>Vue Element 版</strong></td><td><code>Vue 3</code> · <code>TypeScript</code> · <code>Vite</code> · <code>Element Plus</code>（轻量纯净版）</td></tr>
<tr><td><strong>React 版</strong></td><td><code>React 19</code> · <code>TypeScript</code> · <code>Vite</code> · <code>Zustand</code> · <code>Ant Design V6</code>（无 UMI）</td></tr>
<tr><td><strong>质量门禁</strong></td><td><code>差分回归台架</code> · <code>cargo fmt / clippy / test</code> · <code>GitHub Actions</code>（ubuntu + windows 矩阵）</td></tr>
</table>

---

## 安全与等保合规

本项目的安全能力参照《网络安全等级保护 2.0》（二级/三级）技术要求设计，面向企业高隐私私有化部署场景开箱即用：

| 等保技术要求 | 落地实现 |
|------------|---------|
| **安全审计** | 六类审计日志全覆盖：登录 / 操作 / API / 数据访问 / 权限变更 / 策略评估，记录 IP 归属地与 trace_id；留存与归档策略可调（默认库内留存 180 天，超期数据导出 JSONL 归档文件留痕） |
| **身份鉴别** | 口令复杂度（≥8 位、小写/大写/数字/符号四类取三）、历史口令复用检查（默认近 3 条）、口令有效期（默认 90 天），阈值经「参数管理」平台参数调整；TOTP 多因素认证（MFA）；图形验证码；登录失败限流（IP + 用户名双维度）；可配置登录限制策略 |
| **访问控制** | 动态 RBAC 权限引擎，角色—权限—接口映射存于数据库，权限变更即时热更新生效；菜单/按钮级权限控制，角色级行数据权限范围与字段级权限（黑名单字段自响应裁剪）；每次鉴权判定落策略评估日志可追溯 |
| **多租户隔离** | 存储层数据隔离：读查询自动注入租户过滤，写入防伪造租户、更新/删除强制租户谓词；租户请求按 `(path, method)` 经 Api 表 fail-closed 校验（缺权限点即拒绝）；套餐模块白名单与到期只读策略 |
| **数据保密性** | 登录口令应用层 AES 加密传输、bcrypt 哈希存储；JWT RS256 非对称签名；refresh token 走 HttpOnly Cookie；传输层 TLS 由部署层启用 |
| **数据备份恢复** | 定时全量备份（pg_dump，默认保留 30 份自动轮换），支持 Docker 容器 / 本地直连双模式 |
| **前端安全** | 三套前端生产构建均启用 CSP、X-Frame-Options、HSTS 等安全响应头 |

> **说明**：等保测评除技术要求外，还包含管理制度、物理环境、人员组织等非软件范畴的内容。本项目覆盖的是技术措施部分，可为私有化部署的等保测评准备提供直接支撑，但不能替代完整的等保测评流程。

---

## 当前进度

项目按 [docs/development-plan.md](./docs/development-plan.md) 的阶段推进，契约面已完成，服务实现按模块渐进落地。

**已落地**

- 契约流水线：proto 同步（MANIFEST 校验门 + 漂移检测）→ 注解描述符 → 构建期确定性生成路由 / 错误状态表 / 服务接口 / 挂载胶水
- REST :7788 装配：203 条路由、8 个免鉴权端点白名单、JWT RS256 鉴权门、CORS、统一四字段错误信封（code / reason / message / metadata）
- 编解码对齐：protojson 请求绑定与响应发射（64 位整数字符串化、presence 省略、well-known 类型语义），金样测试锁定
- 差分回归台架：203 条路由 + 89 条 HEAD 自动 sweep，豁免 4 类显式登记
- 部分系统模块的真实服务实现（用户 / 角色 / 租户 / 字典 / 认证 / MFA 等），其余端点暂返回 Unknown 桩

**进行中 / 规划**

- 存储层：RBAC 策略装载、会话吊销 checker、query 别名层
- 剩余服务模块逐个替换桩实现
- SSE（:7789）、异步任务队列、审计日志归档、脚本系统

---

## 快速开始

### 环境要求

| 工具 | 版本 |
|------|------|
| Rust | stable（workspace `rust-version = 1.81`） |
| buf | 最新版（`curl -fsSL https://buf.build/install.sh | sh`，或 [GitHub releases](https://github.com/bufbuild/buf/releases) 单二进制）；`cargo build` 时需在 PATH（注解闭包编译） |
| bash | 运行同步 / 台架脚本（Windows 推荐 Git Bash） |
| Docker | 20.0+（本地中间件 / 差分台架） |
| Node.js + pnpm | 以各前端 `package.json` 的 `engines` 为准（当前约束交集 ≥ 20.19.0），pnpm >= 10.0.0 |

### 同级克隆布局

本仓以相对路径引用兄弟仓，请将以下仓库克隆到**同一父目录**下：

```text
<parent>/
├── rushwind-admin/   # 本仓
├── rushwind/         # RushWind 框架 monorepo（必须）
└── rust-utils/       # 工具库（必须）
```

### 后端启动

```shell
cd backend
cargo run -p admin-api   # 二进制 admin-api，监听 REST :7788
```

- 启动时连接 PostgreSQL 与 Redis，配置位于 `backend/services/admin-api/assets/`（`auth.yaml` / `data.yaml` / `oss.yaml`）
- `assets/jwt_public_key.pem` 与 `auth.yaml` 内嵌密钥为**演示密钥**，生产部署必须更换
- SSE（:7789）尚未开放

### 契约同步

proto 契约由脚本从上游契约源同步进本仓，并加 MANIFEST 校验门防止手改：

```shell
bash backend/api/sync-protos.sh          # 同步 proto 并重建 MANIFEST
bash backend/api/sync-protos.sh --check  # 校验门（与 CI 一致）
```

同步源的默认路径与覆盖方式见脚本头部说明（`backend/api/sync-protos.sh`）。**不要手改** `backend/api/protos/`。

### 质量门

```shell
cd backend
cargo fmt -p proto -p auth -p admin-api -p admin-diff -- --check
cargo clippy --workspace -- -D warnings
cargo test --workspace
```

CI（ubuntu + windows 矩阵）执行同样的四道门：fmt / clippy / test / 契约同步校验，见 [.github/workflows/ci.yml](./.github/workflows/ci.yml)。

### 差分回归台架

[backend/testbed](./backend/testbed) 通过 compose 拉起中间件与 Go / Rust 双栈后端，`admin-diff` 对 203 条路由 + 89 条 HEAD 自动 sweep 并逐字节比对响应，豁免登记于 `exemptions.json`。使用方式见 [backend/testbed/README.md](./backend/testbed/README.md)。

### 前端启动

前端与后端是零改动兼容契约：API 基址指向本仓后端（REST :7788）即可，无需改动任何前端代码。三套前端均随本仓同步（同步快照 + RushWind 品牌覆写层 + 双清单校验门防手改），dev 代理默认已指向 :7788：

| 前端版本 | 目录 | 状态 | 启动命令 | 端口 |
|---------|------|------|---------|------|
| React | `frontend/admin/react` | ✅ 随仓快照 + 品牌覆写 | `pnpm dev` | 5888 |
| Vue Element | `frontend/admin/vue-element` | ✅ 随仓快照 + 品牌覆写 | `pnpm dev` | 5777 |
| Vue Vben | `frontend/admin/vue-vben` | ✅ 随仓快照 + 品牌覆写 | `pnpm dev:antd` | 5666 |

```shell
# React 版（本仓 frontend/admin/react）
cd frontend/admin/react
pnpm install
pnpm dev            # :5888，代理转发至 REST :7788

# Vue Element 版（本仓 frontend/admin/vue-element）
cd frontend/admin/vue-element
pnpm install
pnpm dev            # :5777，代理转发至 REST :7788

# Vue Vben 版（本仓 frontend/admin/vue-vben，monorepo，在仓库根执行）
cd frontend/admin/vue-vben
pnpm install
pnpm dev:antd       # :5666，代理转发至 REST :7788
```

---

## 功能列表

> 各列表页（业务数据与审计日志）均支持按当前筛选条件分页聚合导出，格式可选 CSV / XLSX（上限 1 万行）。后端各模块的实现覆盖情况见[当前进度](#当前进度)。

### 组织与权限

| 功能 | 说明 |
|------|-----|
| 用户管理 | 管理和查询用户，支持高级查询和按部门联动用户，用户可禁用/启用、设置/取消主管、重置密码、配置多角色、多部门和上级主管、一键登录指定用户等功能 |
| 租户管理 | 管理租户，新增租户后自动初始化租户部门、默认角色和管理员。支持配置套餐、禁用/启用、一键登录租户管理员功能 |
| 套餐与配额管理 | 管理租户订阅套餐及其资源配额（如模块白名单、用量上限），支持套餐与配额项的增删改查 |
| 角色管理 | 管理角色和角色分组，支持按角色联动用户，设置菜单授权、数据权限范围（五档 / 自定义组织单元集）与字段级权限（黑名单字段集），批量添加和移除员工 |
| 权限管理 | 管理权限分组、菜单、权限点，支持树形列表展示 |
| 组织管理 | 管理组织，支持树形列表展示 |
| 职位管理 | 用户职务管理，职务可作为用户的一个标签；支持 Excel 导入（客户端模板下载、逐行走既有创建接口、行级错误回报，所属组织列按组织名称精确匹配回填组织单元） |
| 菜单管理 | 配置系统菜单，操作权限，按钮权限标识等，包括目录、菜单、按钮；支持菜单同步（三端齐备，事务化清空重建或增量合并两种模式，合并模式按全路径匹配原位更新并保留既有菜单 ID 与角色授权） |

### 系统功能

| 功能 | 说明 |
|------|-----|
| 接口管理 | 管理接口，支持接口同步功能，主要用于新增权限点时选择接口，支持树形列表展示、操作日志请求参数和响应结果配置 |
| 字典管理 | 管理数据字典大类及其小类，支持按字典大类联动字典小类、服务端多列排序、数据导入和导出 |
| 任务调度 | 管理和查看任务及其任务运行日志，支持任务新增、修改、删除、启动、暂停、立即执行 |
| 文件管理 | 管理文件上传，支持文件查询、上传到 OSS 或本地、下载、复制文件地址、删除文件、图片支持查看大图功能 |
| 登录策略 | 管理登录限制策略，配置目标用户的限制类型、限制方式、限制值与限制原因 |
| 账号登录 | 支持用户名 / 邮箱 / 手机号多标识登录，可叠加图形验证码、登录策略与 TOTP 多因素认证 |
| 多因素认证（MFA） | 基于 TOTP 的多因素认证，含登录挑战、个人中心绑定管理，以及管理员救援重置用户 MFA 的解锁路径 |
| 找回密码 | 绑定邮箱验证码找回密码：验证码 10 分钟单次有效、重置成功即吊销全部会话，静默处理防用户枚举 |
| 通知渠道 | 管理通知渠道（EMAIL / SMTP），密码加密存储、列表脱敏展示，支持启用 / 停用与测试发送 |
| 服务监控 | 只读展示服务运行时指标（CPU 核数、内存、运行时长等），自动刷新 |
| 脚本系统 | 脚本级插件系统（Lua / JavaScript，数据库为事实源，管理页增改即时生效）：实体生命周期钩子（before 可否决 / after 异步）、定时任务、HTTP 出站（域名白名单 fail-closed）、试运行与执行日志 |
| 参数管理 | 平台全局系统参数的键值管理（区别于业务字典），内置参数启动时播种、禁删可改；服务侧经缓存 accessor 读取，多实例部署下参数变更经 Redis 发布订阅广播失效各实例缓存 |
| 机器凭证（AK/SK） | 租户级 AccessKey / SecretKey 管理：创建时 Secret 一次性展示，支持启停、删除与密钥轮换重置（轮换后旧 Secret 立即失效）；AK / Secret 可经令牌交换端点换取租户作用域机器 JWT（machine 角色、仅签发 access 令牌），交换端点按 IP + AK 接入尝试限流 |
| 语言管理 | 管理系统支持的多语言，配置语言名称、语言代码、本地名称、启用与默认状态 |

### 消息与日志

| 功能 | 说明 |
|------|-----|
| 消息分类 | 管理消息分类，支持 2 级自定义消息分类，用于消息管理消息分类选择 |
| 消息管理 | 管理消息，支持按发送范围（全员 / 指定用户）发送与消息撤销，全员广播走异步任务队列投递（断点恢复、幂等），可查看用户是否已读和已读时间 |
| 站内信 | 站内消息管理，支持消息详细查看、删除、标为已读、全部已读功能 |
| 登录日志 | 登录日志列表查询，记录用户登录成功和失败日志，支持 IP 归属地记录 |
| 操作日志 | 操作日志列表查询，记录用户操作正常和异常日志，支持 IP 归属地记录与资源对象定位，查看操作日志详情 |
| API 日志 | API 日志列表查询，记录 API 请求的操作者、请求路径、方法与成功状态，支持 IP 归属地记录 |
| 数据日志 | 数据访问日志列表查询，记录数据访问行为，SQL 词法脱敏，自动提取涉及表名与数据分类 |
| 权限日志 | 权限变更日志列表查询，记录权限变更的操作者、目标对象与原因，留存请求快照 |
| 策略评估日志 | 策略评估日志列表查询，记录每次鉴权判定的结果与评估上下文，支持 trace_id 关联排障 |
| Redis 缓存监控 | Redis 缓存监控，只读展示 Redis INFO、DBSIZE 与慢日志数据，不执行写操作 |

### 个人中心

| 功能 | 说明 |
|------|-----|
| 个人中心 | 个人信息展示和修改，查看最后登录信息，密码修改、邮箱绑定 / 换绑（验证码校验）等功能 |

---

## 项目结构

```text
rushwind-admin/
├── backend/
│   ├── api/                        # API 契约（唯一契约源）
│   │   ├── protos/                 # proto 契约副本（MANIFEST.sha256 校验门）
│   │   ├── third_party/            # 第三方 proto（google.api 等）
│   │   └── sync-protos.sh          # 契约同步与校验脚本
│   ├── crates/                     # 共享 crate（proto 契约生成 crate、auth 鉴权门）
│   ├── services/
│   │   └── admin-api/              # Admin 服务 crate（src/ 模块树 + assets/ 内嵌资源）
│   └── testbed/                    # 差分回归台架（compose + admin-diff sweep）
├── frontend/                       # 三套前端同步快照（sync-frontend.sh + 双清单门 + RushWind 品牌覆写）
├── docs/                           # 项目文档（binding-spec / development-plan / operator-matrix / screenshots）
└── .github/workflows/              # CI（fmt / clippy / test / 契约同步门）
```

---

## 相关项目

- **[rushwind](https://github.com/tx7do/rushwind)** —— RushWind 框架 monorepo（http / http-binding / authn / transport-axum / gen-http 等）
- **[rust-utils](https://github.com/tx7do/rust-utils)** —— Rust 工具库（`query_parser` 等）

## 社区与贡献

欢迎参与 RushWind Admin 的建设：

- [贡献指南](./CONTRIBUTING.md) —— 同级克隆布局、契约同步约定、提交规范与 PR 流程
- [安全策略](./SECURITY.md) —— 漏洞上报流程与覆盖范围

## 联系我们

- 微信个人号：`yang_lin_bo`（备注：`rushwind-admin`）
- 掘金专栏：<https://juejin.cn/column/7541283508041826367>

## 致谢

[![JetBrains](https://resources.jetbrains.com/storage/products/company/brand/logos/jb_beam.svg)](https://jb.gg/OpenSource)

感谢 JetBrains 提供的免费开源授权。
