# 01 · 架构全景：一次请求的完整路径

> 前置：无。读完你会知道：代码都在哪、一个请求从浏览器到数据库经过了什么、有哪些横切系统在起作用、哪些代码是生成的。

## 1. 仓库布局

```text
rushwind-admin/
├── backend/                    Rust 后端（Cargo workspace）
│   ├── api/                    API 契约树（唯一契约源）
│   │   ├── protos/             proto 契约副本（MANIFEST.sha256 校验门，禁手改）
│   │   ├── buf.yaml/buf.lock   buf 工作区与依赖钉定（google.api 等注解模块经 buf 依赖解析）
│   │   └── sync-protos.sh      契约同步与校验脚本
│   ├── crates/                 可被每个微服务共享的库
│   │   ├── proto/              契约 crate：构建期编译契约树 + 生成路由/错误表/服务 trait/挂载
│   │   └── auth/               鉴权门：auth_gate 与三个可注入 trait
│   ├── services/
│   │   └── admin-api/          Admin 服务 crate（src/ 模块树 + assets/ 内嵌配置）
│   └── testbed/                差分回归台架（compose + admin-diff 全量路由 sweep）
├── frontend/admin/             三套前端同步快照（react / vue-element / vue-vben）
└── docs/                       本文档体系（教程层 + 参考层，见 [docs/README.md](../README.md)）
```

两个要点：

- **`backend/crates/` 与 `backend/services/` 的分工**：crates 放"每个微服务都能共用"的库（目前是契约 crate 和鉴权门），每个微服务一个 `services/<name>` crate（目前是 admin-api）；数据层（实体、仓储、租户范围）属于服务自身，放在服务 crate 的 `src/data/` 里，不抽公共 crate。
- **proto 是唯一 API 契约**：284 条 REST 路由全部由 proto 的 `google.api.http` 注解在构建期生成，仓库里没有一条手写路由。契约树的维护方式见[第 3 章](./03-codegen-chain.md)。

前端说明：本仓随仓携带三套前端同步快照（`frontend/admin/{react,vue-element,vue-vben}`），生成的 TypeScript 客户端三份字节相同。三套前端功能同构、技术栈各异（React 19 + Ant Design V6 / Vue3 + Element Plus / Vben Admin 5.x）、共用同一套后端契约——**它们是给不同技术栈团队的三个并列选项，不是"同时要三套"的一套交付**。

## 2. 一次请求的完整路径

以"登录后的管理员在用户管理页点了一次查询"为例：

```text
浏览器
  │  GET /admin/v1/users?...（路由自带 /admin/v1 前缀）
  ▼
开发态接线：react dev server（:5888）
  │  VITE_PROXY='[["/admin", "http://127.0.0.1:7788/"]]'——vite 按 /admin 前缀
  │  转发到 7788，不做 rewrite（后端路由自带前缀），避免开发态跨域
  ▼
RushWind 装配层（rushwind-bootstrap，由 services/admin-api/assets/server.yaml 驱动）
  │  edge：gorilla 兼容的 CORS 层 + 10s 请求预算
  ▼
REST transport（axum，:7788）
  │  1. 路由匹配：构建期从注解描述符生成的路由表（284 条）
  │  2. 绑定层（逐路由最外层）：body 走 protojson（Content-Type 查表）、
  │     query 走 form 编解码——绑定失败在这里就返回 400，先于任何鉴权
  │  3. 鉴权门（仅门控路由）：JWT RS256 验签 + Redis 会话检查 → 租户检查
  │     → authz 授权评估 → 注入请求上下文
  ▼
生成的 handler（proto::gen::mounts::mount_*）
  │  解参、构造对应 Service、调用 trait 方法
  ▼
Service 层（services/admin-api/src/services/*.rs）
  │  业务逻辑：从请求上下文取操作者、调仓储、做 proto↔模型映射
  ▼
Repo 层（src/data/repos/*.rs）
  │  分页/过滤/排序组装（fetch_paged），查询谓词由 Viewer 派生
  ▼
数据层租户规则（src/data/scope.rs）
  │  平台视图全见；租户视图强制 tenant_id 谓词；系统视图旁路
  ▼
SeaORM → PostgreSQL
  ▼（响应沿原路返回：entity→proto 映射、protojson 序列化——64 位整数字符串化、
     presence 字段未设置时省略）
```

关键认知：

- **service/repo 的 CRUD 骨架高度模式化**——第 4 章你会照着一个真实小模块走一遍；
- **横切关注点不在业务代码里**：认证在鉴权门、行级隔离在 Viewer、审计在路由包的审计层——业务代码"看不见"它们，但每条路径都在其覆盖下；
- **绑定先于鉴权是线上契约**：坏 Content-Type 或畸形参数必须 400 先行，即使令牌也无效（对齐基准见 [binding-spec.md](../binding-spec.md)）；
- **DTO 与库模型是两套类型**：proto 生成的 DTO 只在 service 边界出现，库里存的是 SeaORM entity，映射靠显式函数，漏字段就是静默零值。

## 3. 另外三条通道

一个生命周期内共有四个 transport 并行服务（装配关系见 `server.yaml`，第 8 章展开）：

| 通道 | 端口 | 用途 |
|---|---|---|
| REST | :7788 | 全部管理面路由（284 条） |
| SSE | :7789 `/events` | 服务端推送（站内信、通知）；流按 userId 归属校验，连接独立鉴权 |
| 任务队列 worker | —（无监听端口） | 消费 Postgres 队列（rushwind-apalis-postgres）：广播扇出、审计归档、定时备份等异步任务 |
| cron 生产者 | —（无监听端口） | 周期投递任务：租户到期扫描（每小时）、审计归档（03:30）、sys_tasks 表驱动的用户任务 |

## 4. 横切系统地图

| 系统 | 一句话 | 实现位置 | 深读 |
|---|---|---|---|
| 认证 | JWT RS256（access + refresh HttpOnly Cookie 双 Cookie 轮换），口令应用层加密传输；认证阶段由框架 crate（rushwind-authn-gate）承载 | `crates/auth` + `services/.../token.rs`、`services/authentication/` | 第 5 章 |
| 授权（接口级） | authz 评估"该用户能否调该接口"，策略存 DB，每次评估落策略评估日志 | `crates/auth` + `services/.../authorizer.rs` | 第 5 章 |
| 租户隔离 | 数据层 Viewer 三态（平台/租户/系统）+ HTTP 层租户闸门（套餐模块白名单） | `src/data/scope.rs` + `crates/auth` | 第 6 章 |
| 数据范围 | 角色级行过滤（五档），令牌承载聚合结果（`ds`/`dss` 声明） | 仓储谓词 + 令牌载荷 | 第 6 章 |
| 字段级权限 | 角色黑名单字段（`hfs` 声明），服务端响应裁剪 + 前端隐藏 | `services/admin_portal.rs`、`role.rs` | 第 5 章 |
| 审计日志 | 六类日志全覆盖：登录（含风控打分）/操作/API/数据访问/权限变更/策略评估 | `src/audit/`（中间件层） | 第 7 章 |
| 异步任务 | Postgres 队列 + cron 生产者（系统任务 + sys_tasks 表驱动任务） | `src/server/apalis.rs` | [docs/README.md](../README.md) 工程档案 |
| SSE 推送 | 站内信/通知推送；Hub 广播 + 逐收件人投递 | `src/server/sse.rs` | [docs/README.md](../README.md) 工程档案 |
| 参数管理 | 平台全局参数键值（sys_config），缓存读取 + Redis 发布订阅多实例失效 | `src/services/config.rs` | — |
| 对象存储 | MinIO（S3 兼容），五内容桶，签名图片代理 | `src/state.rs` + `services/file.rs` | — |
| 质量门禁 | fmt / clippy / test / 契约同步四道 CI 门 + 差分回归台架 | `.github/workflows/ci.yml`、`backend/testbed` | 第 3 章 |

## 5. 生成代码与手写代码的边界

这个仓库大量依赖代码生成。**分不清这条边界，改错文件是新人最常见的翻车点：**

| 位置 | 产生方式 | 能否手改 |
|---|---|---|
| `backend/api/protos/**` | `sync-protos.sh` 从上游契约源同步（MANIFEST 门） | ❌ 改上游契约源后重新同步 |
| 契约树的注解依赖（google.api 等） | buf 工作区按 `buf.lock` 钉定（上游 lock 是唯一版本事实） | ❌ 随上游 lock 同步 |
| `crates/proto` 生成面（路由表/错误表/服务 trait/mounts/prost 类型） | `build.rs` 每次构建确定性再生 | ❌ 全部由 `backend/api/protos` 派生 |
| 前端 `src/api/generated/**` | 从契约生成的 TypeScript 客户端 | ❌ |
| `services/admin-api/src/services/**` | 手写（模式化样板） | ✅ |
| `services/admin-api/src/data/**` | 手写（实体/仓储/租户规则） | ✅ |
| `crates/auth`、`src/server/**`、`src/audit/**` | 手写（门、装配、审计层） | ✅ |

生成链路的完整机制在[第 3 章](./03-codegen-chain.md)，实战在[第 4 章](./04-first-service-module.md)。

## 6. 本章小结

- proto 契约是唯一事实源，路由/错误表/服务接口/前端客户端全部由它派生；
- 请求路径：前端代理 → edge（CORS/预算）→ 绑定层 → 鉴权门（认证→租户→授权）→ 生成 handler → service → repo → 数据层租户规则 → DB；
- 一个生命周期四个 transport：REST、SSE、任务 worker、cron 生产者；
- 先分清生成物与手写物，再动手改代码。

下一步：[02 · 从零跑起来](./02-get-it-running.md)。
