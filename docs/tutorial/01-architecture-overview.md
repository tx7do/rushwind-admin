# 01 · 架构全景：一次请求的完整路径

> 前置：无。读完你会知道：代码都在哪、一个请求从浏览器到数据库经过了什么、有哪些横切系统在起作用、哪些代码是生成的。

## 1. 仓库布局

```text
rushwind-admin/
├── backend/                    Rust 后端（Cargo workspace）
│   ├── crates/                 框架库：biz-proto-build（契约构建引擎）、auth（鉴权门）
│   └── apps/
│       └── admin/              ★ 唯一的产品应用（一个 crate，一个二进制）
│           ├── api/            应用的契约工作区（一个 buf.yaml + 一个 buf.lock）
│           │   ├── protos/     admin 契约树（MANIFEST.sha256 校验门，禁手改）
│           │   └── fork/protos/ fork 模块契约树（fork 自有，自由编辑）
│           ├── src/admin/      59 个挂载服务面 + 数据层 + 路由边 + 审计（上行同步落点）
│           ├── src/modules/    fork 业务模块（copy-rename template 起步）
│           ├── src/proto/      契约面（构建期生成：类型/池/gen 各 face）
│           └── assets/         内嵌配置（server.yaml / auth.yaml / …）
│   （testbed/ 与 apps/ 平级：差分回归台架 compose + admin-diff 全量路由 sweep）
├── frontend/admin/             三套前端同步快照（react / vue-element / vue-vben）
└── docs/                       本文档体系（教程层 + 参考层，见 [docs/README.md](../README.md)）
```

两个要点：

- **`crates/` 与 `apps/` 的分工**：crates 放框架库（契约构建引擎、鉴权门——不含任何 admin 业务语义），apps 放产品应用；数据层（实体、仓储、租户范围）属于应用自身，放在 `apps/admin/src/admin/data/` 里，不抽公共 crate。业务模块长在应用内部 `apps/admin/src/modules/`，与上游同步面同仓不同室（所有权一条线，见 [09 章](./09-write-your-first-module.md)）。
- **proto 是唯一 API 契约**：admin 面的 291 条 REST 路由绑定全部由 proto 的 `google.api.http` 注解在构建期生成（fork 面 5 条同源生成，另计），仓库里没有一条手写路由——唯一例外是 `rest.rs` 手写的图片代理路由 `GET /admin/v1/file/image`（签名图片代理，不走契约注解）。契约树的维护方式见[第 3 章](./03-codegen-chain.md)。

前端说明：本仓随仓携带三套前端同步快照（`frontend/admin/{react,vue-element,vue-vben}`），生成的 TypeScript 客户端三份字节相同。三套前端功能同构、技术栈各异（React 19 + Ant Design V6 / Vue3 + Element Plus / Vben Admin 5.x）、共用同一套后端契约——**它们是给不同技术栈团队的三个并列选项，不是"同时要三套"的一套交付**。

### 1.1 Cargo 工作区与依赖图

`backend/` 是一个 Cargo workspace，根清单 `backend/Cargo.toml` 只做四件事：声明成员、钉 Rust 基线、设全局 lint、集中管理跨 crate 依赖版本。

```toml
[workspace]
resolver = "2"
members = ["crates/*", "apps/*", "testbed/admin-diff"]

[workspace.package]
version = "0.1.0"
edition = "2021"
rust-version = "1.81"
license = "MIT"
```

成员按通配展开后是四件套，各有分工：

| 成员 | 是什么 | 深读 |
|---|---|---|
| `crates/auth` | 鉴权门：三阶段门控（会话 → 租户 → 授权）+ 失败信封，部署侧契约见 §2.2 | 第 5 章 |
| `crates/biz-proto-build` | 契约构建引擎：把 `api/` buf 工作区编译成两个面 + 每个 face 一份生成物（只被 admin 的 `build.rs` 以 build-dependencies 引用） | 第 3 章 |
| `apps/admin` | 产品应用本体：一个 crate 编出一个 `admin` 二进制，REST :7788 与 SSE :7789 同进程 | 本章其余 |
| `testbed/admin-diff` | 差分回归台架的 Rust 侧回放器（Go/Rust 双栈对打，见第 3 章 §4） | [backend/testbed/README.md](../../backend/testbed/README.md) |

workspace 根还携带三处全局约定，新成员 crate 会自动继承两处：

- **全局 clippy 基线**：`[workspace.lints.clippy]` 设 `all = "warn"` 与 `undocumented_unsafe_blocks = "warn"`；成员在自身 `Cargo.toml` 写 `[lints] workspace = true` 逐字继承（`apps/admin` 就是这么接的）。引擎 crate 顶部更是一级戒备：`crates/biz-proto-build/src/lib.rs` 开头就是 `#![forbid(unsafe_code)]`——unsafe 出现必须有块级安全论证。
- **Rust 基线 1.81**：`rust-version = "1.81"` 三处一致（workspace 与成员共享），低于它的工具链直接拒绝编译。
- **两个 profile 钉**：`[profile.release] lto = "thin"`；`[profile.dev] debug = "line-tables-only"`——后者是 Windows 专属保护：MSVC 的 PDB 有 4GB 上限，全量调试信息链接即爆 `LNK1140`，行号表保留回溯能力，类型级调试信息让给 release。

依赖形状（谁依赖谁，往下读第 3 章前只需要记前两行）：

```text
admin（apps/admin，产品应用）
├── auth                    本地 crate：鉴权门（edge.rs 只调它的 auth_gate）
├── biz-proto-build         本地 crate：[build-dependencies]，构建期把 api/ 编译成 gen 面
│     └── rushwind_proto_build / rushwind-gen-http   框架仓的引擎与生成器（同一 rev）
├── rushwind-* × 15         框架栈（Git 依赖，rev 钉版，见 §1.2）
└── rust-utils              工具库（Git 依赖，rev 钉版：Django 风格 query 解码等）
```

### 1.2 rushwind 框架：rev 钉版的 Git 依赖

本仓不把框架 vendor 进来，也不要求本地有框架 checkout——框架栈以 Git 依赖按 revision 钉死。两份 manifest 共 16 处 `rev = "08fbfcd"`：

- `backend/apps/admin/Cargo.toml` 里 14 处直接依赖（`rushwind-authn`、`rushwind-http-binding`、`rushwind-bootstrap`、`rushwind-transport-axum` 等）；
- `backend/Cargo.toml` 的 `[workspace.dependencies]` 里 2 处（`rushwind-transport-cron`、`rushwind-apalis-postgres`），应用经 `{ workspace = true }` 转接前者。

Cargo.lock 把 rev 解析成完整 commit，锁死后每次拉的都是同一份字节：

```text
git+https://github.com/tx7do/rushwind?rev=08fbfcd#08fbfcd59eb60acdf381794041e09bfbd18cbac7
```

框架版本升级是显式动作：改 rev → `cargo update -p <crate>` 重新锁 → 跑全量测试与差分台架。工具库同理，`rust-utils` 按自己的 rev 钉（带 `json` / `otp` / `crypto` / `printrender` 四个 feature）：

```toml
rust-utils = { git = "https://github.com/tx7do/rust-utils", rev = "05000a8", features = [
    "json",
    "otp",
    "crypto",
    "printrender",
] }
```

应用消费的 15 个 rushwind crate，按职责分五组：

| 组 | crate | 在应用里的落点 |
|---|---|---|
| 装配 | `rushwind-bootstrap` | `main.rs` 的 `Bootstrap::from_yaml_str(SERVER_YAML)` 生命周期装配（见第 2 章 §3.2） |
| 传输 | `rushwind-transport` / `-axum` / `-sse` / `-cron` + `rushwind-apalis-postgres` | 四个 transport 的传输底座与 Postgres 任务队列 |
| HTTP 契约语义 | `rushwind-http-binding` | 绑定层 `bind_run`、胶水 `glue::handle`、错误信封 `envelope`、请求上下文 `ctx`、`RouteWire` |
| 认证 | `rushwind-authn` / `-jwt` | `Authenticator` trait + RS256 验签/签发引擎 |
| 存储 | `rushwind-storage-seaorm-support` / `-seaorm-migration` | 分页输入 `PagingInput`、`db_err` 映射、实体派生迁移 `EntityTables` |
| 脱敏 | `rushwind-redact` | 静态脱敏计划 `RedactPlan`（crate 根 `redact_plan()` 构建） |
| 对象存储 | `rushwind-oss` / `-oss-s3` / `-oss-local` | `ObjectStorage` trait、S3/MinIO 引擎、本地磁盘镜像引擎 |

版本对齐约定写在 workspace 依赖注释里：proto 类型链（prost 0.14 / pbjson 0.9）"版本与 rushwind 工作区保持一致"——应用侧不要单方面抬这些 crate 的版本，否则和框架内编译出的类型对不上。权威规格（各 crate 的完整 API 面）见框架仓，本教程只在用到的地方点名。

## 2. 一次请求的完整路径

以"登录后的管理员在用户管理页点了一次查询"为例：

```text
浏览器
  │  GET /admin/v1/users?...（路由自带 /admin/v1 前缀）
  ▼
开发态接线：react dev server（:15888，.env.development 的 VITE_SERVER_PORT）
  │  VITE_PROXY='[["/admin", "http://127.0.0.1:7788/"]]'——vite 按 /admin 前缀
  │  转发到 7788，不做 rewrite（后端路由自带前缀），避免开发态跨域
  ▼
RushWind 装配层（rushwind-bootstrap，由 apps/admin/assets/server.yaml 驱动）
  │  edge：gorilla 兼容的 CORS 层 + 10s 请求预算
  ▼
REST transport（axum，:7788）
  │  1. 路由匹配：构建期从注解描述符生成的路由表（291 条）
  │  2. 绑定层（逐路由最外层）：body 走 protojson（Content-Type 查表）、
  │     query 走 form 编解码——绑定失败在这里就返回 400，先于任何鉴权
  │  3. 鉴权门（仅门控路由）：JWT RS256 验签 + Redis 会话检查 → 租户检查
  │     → authz 授权评估 → 注入请求上下文
  ▼
生成的 handler（proto::gen::mounts::mount_*）
  │  解参、构造对应 Service、调用 trait 方法
  ▼
Service 层（apps/admin/src/admin/services/*.rs）
  │  业务逻辑：从请求上下文取操作者、调仓储、做 proto↔模型映射
  ▼
Repo 层（apps/admin/src/admin/data/repos/*.rs）
  │  分页/过滤/排序组装（fetch_paged），查询谓词由 Viewer 派生
  ▼
数据层租户规则（apps/admin/src/admin/data/scope.rs）
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

### 2.1 逐站注解：路由边的层序与真实函数名

上面的第 2、3 站都发生在"路由边"（route-pack edge，`apps/admin/src/admin/server/edge.rs`）。它是每个路由包（admin 面、业务模块面）共同骑的底座，逐路由的组合逻辑集中在 `make_wrap`：

```rust
// apps/admin/src/admin/server/edge.rs（节选，注释有删）
/// The per-route layer composition: the auth gate inside (gated routes
/// only), the bind layer outermost — the wire order, not a preference.
pub fn make_wrap(
    state: &Arc<AppState>,
    pool: &'static DescriptorPool,
    subtypes: &'static [&'static str],
) -> impl Fn(MethodRouter, &RouteWire, bool) -> MethodRouter + Send + Sync {
    // …… authenticator / checker / tenant_checker / authorizer 四个 Arc 在闭包外建好
    move |mr: MethodRouter, wire: &RouteWire, gated: bool| -> MethodRouter {
        let mut out = mr;
        if gated {
            // …… 组装 auth_gate 中间件
            out = out.layer(gate);          // ① 先挂：门在内层
        }
        // …… 组装 bind_run 中间件
        out = out.layer(bind);              // ② 后挂：绑定层在最外层
        out
    }
}
```

三个必懂的点：

- **axum 的层序语义**：`.layer()` 后挂的包在外层。`make_wrap` 先挂鉴权门、后挂绑定层，于是每个门控路由从外到内是 `bind → auth_gate → handler`，公开路由只有 `bind → handler`。这不是实现细节而是线上契约——edge.rs 模块注释原文："codec/binding failures answer 400 ahead of any 401"。绑定失败（坏 Content-Type、畸形 query）在最外层就返回 400，请求根本到不了鉴权门；对齐基准见 [binding-spec.md](../binding-spec.md)。
- **两个参数从哪来**：`pool` 是本路由包的契约描述符池（`crate::pool()`，见 §2.5），绑定层拿它把路由的输入类型解析成可绑定叶子；`subtypes` 是 `rest.rs` 顶部的 `REGISTERED_SUBTYPES = &["json", "proto", "x-www-form-urlencoded"]`——绑定层处理 Content-Type 的白名单（兼容规格 §2.1 的登记面）。
- **`Wrap` 是生成代码的反向握手**：`pub type Wrap = dyn Fn(MethodRouter, &RouteWire, bool) -> MethodRouter;`——生成的 mount 对每条路由调一次 `wrap(method_router, &wire, gated)`，把"该路由是公开还是门控"（第三参）与该路由的 wire 描述（`RouteWire`：operation id、输入/输出类型全名、body 策略、路径变量）交回手写侧组合。手写侧不知道任何路由，生成侧不知道任何中间件，握手就是这一个函数。

路由包的整机组装在 `edge_router`，顺序固定：

```rust
// apps/admin/src/admin/server/edge.rs（节选）
let mut router_pub = axum::Router::new();
let mut router_gate = axum::Router::new();
for mount in pack.mounts {                       // ① 逐服务双桶挂载
    (router_pub, router_gate) = mount(router_pub, router_gate, &wrap);
}
let mut app = router_pub.merge(router_gate);     // ② 两桶合并
if let Some(docs) = pack.docs { app = app.merge(docs_router); }      // ③ 文档面（/q/*）
for extra in pack.extras { app = app.merge(extra); }                 // ④ extras（图片代理）
if let Some(audit) = pack.audit { app = app.layer(audit_layer); }    // ⑤ 审计层最外
```

为什么审计层最后挂：`.layer()` 后挂在外层，审计中间件因此**在响应路径上最后经手**——它看到的是最终状态码，审计记录才能如实区分 2xx/4xx/5xx。`extras`（图片代理这类手写公开路由）在审计层之内、契约路由之外合并。CORS 与 10s 请求预算不在这张图里：它们骑装配器 per-server 的 edge 段（`server.yaml` 的 `edge:` 块），把该服务器上的所有路由包一视同仁包住（见第 2 章 §3.1 的 server.yaml 注解）。

鉴权门的 Redis 侧适配器也在这个文件：`RedisTokenChecker` 把应用侧的会话存储（`AppState.tokens`，第 5 章展开）适配成框架的 `AccessTokenChecker` trait——门的会话阶段因此只需要两个动词：白名单校验 `is_valid_access_token(uid, jti, token)` 与黑名单校验 `is_blocked_access_token(jti)`。

### 2.2 鉴权门内部：三阶段与失败信封

门控路由到达 `crates/auth/src/lib.rs` 的 `auth_gate` 后，按固定顺序过三阶段（wire 序，每阶段失败立即渲染信封返回）：

| 阶段 | 输入 | 做什么 | 失败形态 |
|---|---|---|---|
| ① 会话（框架侧） | bearer 令牌 + 全部请求头 | `rushwind_authn_gate::authenticate_and_check_session`：JWT RS256 验签（`Authenticator`）→ Redis 白名单/黑名单检查（`AccessTokenChecker`） | 401 `UNAUTHORIZED`，消息文本由框架失败类固定 |
| ② 租户 | claims 的 `tid` + 路由模板 + 动词 | `TenantAccessChecker::check_tenant_access(tenant_id, path, method)`：租户存在/启用/到期只读/套餐模块白名单 | 403 `FORBIDDEN`，消息为检查自身的文本 |
| ③ 授权 | 用户/租户/角色/动词/路由/IP/trace | `AuthorizationEvaluator::authorize(AuthzEvaluation)`：逐角色评估，第一个放行的角色生效；评估留痕落策略评估日志 | 403 `FORBIDDEN`，同上 |

阶段②只对租户成员跑（`tid > 0`）；平台管理员（`tid = 0`）默认跳过——除非在**代租户操作**：携带 `X-Tenant-Id` 头的平台管理员，门会对所选租户跑完整租户阶段，通过后把 claims 里的 `tid` 覆写为生效租户，而令牌本体不动——审计里的 uid/tid 仍是真实操作者。头解析刻意宽松：正整数十进制生效，缺失/空白/非数字/零/负数一律不构成代租户（垃圾值不得变成另一个身份，`parse_impersonated_tenant` 有单测钉住全部形态）。

三阶段全部通过后，claims 整包塞进 `req.extensions`——这就是"注入请求上下文"：胶水层（`rushwind_http_binding::glue`）随后把它装进 `RequestContext`，service 层再从中取操作者。业务代码全程看不到门。

失败信封的两个状态码在 `crates/auth` 里钉成字面量（`UNAUTHORIZED_STATUS = 401` / `FORBIDDEN_STATUS = 403`），注释原话："the message texts AND these numbers are the wire contract (the differential harness byte-compares the 401 envelope)"。框架 crate 因此不反向依赖契约树；`apps/admin/tests/admin_error_statuses.rs` 用错误状态表断言同一对锚（`UNAUTHORIZED → 401`、`FORBIDDEN → 403`），契约侧改锚会在这里红，而不是悄悄漂移。

### 2.3 mount_service!：挂载表的逐行宏机制

服务与路由的唯一对接点是 `rest.rs` 的 `mount_service!` 表。宏本体：

```rust
// apps/admin/src/admin/server/rest.rs（注释节选）
/// The uniform service mount: every service follows the same
/// router-threading, service-construction, and wrap handshake. The
/// state is an explicit parameter — a module-level macro cannot
/// capture the function's local by name.
macro_rules! mount_service {
    ($state:expr, $mount:ident, $service:ident) => {{
        let state = std::sync::Arc::clone($state);
        // The wrap parameter is annotated so the closure is
        // higher-ranked over the reference lifetime — an unannotated
        // closure specializes to one lifetime and fails the
        // MountItem's for<'a> bound.
        Box::new(
            move |router_pub: axum::Router,
                  router_gate: axum::Router,
                  wrap: &super::edge::Wrap|
                  -> (axum::Router, axum::Router) {
                crate::proto::gen::mounts::$mount(
                    router_pub,
                    router_gate,
                    std::sync::Arc::new($service {
                        state: std::sync::Arc::clone(&state),
                    }),
                    wrap,
                )
            },
        ) as super::edge::MountItem
    }};
}
```

逐段拆：

- **入参三件**：`$state`（应用状态 Arc）、`$mount`（生成面 `proto::gen::mounts` 里的挂载函数名）、`$service`（手写 service 结构体名）。宏展开时把 service 实例化（只带一个 `state` 字段）并 `Arc` 化，交给生成的挂载函数。
- **出参是 `MountItem`**：`Box<dyn Fn(Router, Router, &Wrap) -> (Router, Router) + Send + Sync>`——一个"吃双路由桶、吐双路由桶"的闭包。`edge_router` 的挂载循环（§2.1 ①）逐项调用它。
- **`wrap` 参数必须显式标注类型**：不标注的闭包会把引用特化到某一个生命周期，过不了 `MountItem` 的 `for<'a>` 高阶约束——这是宏注释里专门写明的坑，改这段代码的人最容易踩。
- **为什么是宏而不是手写 59 个函数**：模块级宏无法按名捕获调用处的局部变量，所以 `$state` 必须显式传参——宏注释原话。宏保证了 59 行只差三个标识符，"新服务一行"的机械性由它兜底。

挂载表的调用面就是 59 行同构代码（`mount_items()` 返回的 `Vec<MountItem>`）：

```rust
fn mount_items(state: &Arc<AppState>) -> Vec<super::edge::MountItem> {
    vec![
        mount_service!(state, mount_access_key_service, AccessKeyService),
        mount_service!(state, mount_ai_agent_service, AiAgentService),
        // ……共 59 行，每服务一行，最后：
        mount_service!(state, mount_user_service, UserService),
    ]
}
```

每行的路由级分类（进 `router_pub` 还是 `router_gate`）不在这张表里决定——生成的挂载函数按白名单表（§2.4）把每条绑定分桶，表只负责"哪个服务用哪个实现实例化"。

### 2.4 免鉴权白名单：8 端点与双向钉测试

哪些路由不设门，由一张单源表决定：`apps/admin/src/auth_free.rs`（以 `include!` 并入 crate 根）：

```rust
pub const AUTH_FREE: &[(&str, &str)] = &[
    ("admin.service.v1.AuthenticationService", "Login"),
    ("admin.service.v1.AuthenticationService", "GenerateCaptcha"),
    ("admin.service.v1.AuthenticationService", "VerifyCaptcha"),
    ("admin.service.v1.AuthenticationService", "RefreshToken"),
    ("admin.service.v1.MfaService", "VerifyMFAChallenge"),
    ("admin.service.v1.AccessKeyService", "IssueToken"),
    ("admin.service.v1.AuthenticationService", "ForgotPassword"),
    ("admin.service.v1.AuthenticationService", "ResetPasswordByCode"),
];
```

8 个 `(服务全名, proto 方法名)` 对——登录、验证码两件、刷新令牌、MFA 挑战校验、访问密钥签发、忘记/重置密码。这张表的消费者有三个：

1. **构建脚本**（`build.rs` 顶部 `include!` 同一文件）→ 生成器按它把每条路由绑定分类进公开桶或门控桶（§2.3 的双桶）；
2. **crate 根再导出**（`lib.rs` `include!` + `proto/mod.rs` 里 `pub use crate::AUTH_FREE`）→ 守卫测试与差分台架从契约面取用；
3. **守卫测试**（`apps/admin/tests/whitelist.rs`）——双向钉：

| 钉的方向 | 断言 | 防的事故 |
|---|---|---|
| 白名单 → 路由表 | 每个 AUTH_FREE 对仍存在于 `ROUTES` | 删掉某 proto 方法后公开集悄悄缩水（门控面扩大，与参照系失去对齐） |
| 路由表 → 白名单 | 白名单之外没有任何绑定被分类为公开 | 表手误把受保护端点写进白名单（安全事故） |
| 绑定计数 | 公开绑定数 == 8 | 白名单操作的 `additional_bindings` 漂移 |
| 影子集 | `shadowed` 绑定恒等（当前恰 1 条：索引 54，`GET /admin/v1/apis/walk-route`） | 注册顺序或路径形状漂移悄悄改变 first-match 吸收结果 |

同一个测试还把"影子路由"钉死：先注册的同方法模式路由会把后注册的字面量路径吸收（gorilla 语义的 first-match mux），后者永不可达，生成器标记 `shadowed` 并让 mount 跳过注册——契约漂移增删影子必须在这个测试里显式落地。

### 2.5 crate 根：re-export 全清单与进程级单例

`apps/admin/src/lib.rs` 的模块声明只有三行——`pub mod admin`（上游同步面）、`pub mod proto`（契约面）、`pub mod modules`（fork 业务模块），注释原话："Three rooms, one ownership line"。其余是两类手写粘合：

**根级 re-export 全清单**（模板与 main.rs 消费这些入口）：

```rust
pub use admin::{config, crypto, data, migration, paging, seed, server, state};

pub(crate) use admin::fieldmask;
#[cfg(test)]
pub(crate) use admin::testsupport;

include!("auth_free.rs");
```

- 八个 `pub use` 把装配面提到根上：`main.rs` 与模块宏（`repo_shell!` 等宏体里写的是 `crate::data`、`crate::state`）因此读的是一个平面入口，同步来的 handler/test 正文也逐字按 `crate::` 路径书写；
- `fieldmask`（字段级更新闸门）与 `testsupport`（测试脚手架）刻意 `pub(crate)`：只服务 crate 内的同步正文与测试面，不进公开 API；
- `auth_free.rs` 在根上 include，`build.rs` 也 include 同一文件——白名单单源，构建期与运行期永远看同一份。

**两个进程级单例**，也是生成代码的"旋钮表达式"落点：

```rust
pub static DESCRIPTOR_BYTES: &[u8] =
    include_bytes!(concat!(env!("OUT_DIR"), "/annotated_descriptor.bin"));

pub fn pool() -> &'static DescriptorPool { /* OnceLock，进程首次调用时解码一次 */ }
pub fn redact_plan() -> &'static rushwind_redact::RedactPlan { /* OnceLock，fail-closed 构建 */ }
```

为什么它们必须在 crate 根：生成挂载的代码里写死了 `crate::pool()` 与 `crate::redact_plan()`（`build.rs` 的 `pool_expr` / `redact_plan_expr` 旋钮定名）——池是绑定层解析路由绑定、胶水序列化响应的依据；脱敏计划在每次响应序列化前按契约注解裁剪，遇到不认识的选项形状拒绝构建（fail-closed），与"注解源拒绝静默降级"同语义。

`proto` 模块（`src/proto/mod.rs`）把这些拼起来：`include!(OUT_DIR/proto_include.rs)`（prost 类型面）、`pub mod gen`（admin face，`include! admin_gen.rs`）、`pub mod gen::template`（fork face，`include! template_gen.rs`）、手写访问器 `tables::error_status`（reason → HTTP 状态查表）、以及 `PagingRequest → PagingInput` 的 trait impl（孤儿规则钉在本 crate——类型是本 crate 生成的，impl 只能在这）。第 3 章逐面展开。

## 3. 另外三条通道

一个生命周期内共有四个 transport 并行服务（装配关系见 `server.yaml`，第 8 章展开）：

| 通道 | 端口 | 用途 |
|---|---|---|
| REST | :7788 | 全部管理面路由（291 条） |
| SSE | :7789 `/events` | 服务端推送（站内信、通知）；流按 userId 归属校验，连接独立鉴权 |
| 任务队列 worker | —（无监听端口） | 消费 Postgres 队列（rushwind-apalis-postgres）：广播扇出、审计归档、定时备份等异步任务 |
| cron 生产者 | —（无监听端口） | 周期投递任务：租户到期扫描（每小时）、审计归档（03:30）、sys_tasks 表驱动的用户任务 |

四条通道在 `main.rs` 里逐一装配：REST 由 `rest::pack`（admin 面）+ `template::pack`（业务面）两个路由包骑同一台 http server；SSE 的 `factory` 把 `SseApp`（Hub + `AdminGate`）钉在 `:7789/events`——连接鉴权独立于 REST 的门，令牌校验同引擎同存储，流只投给令牌所属的 userId；任务 worker 与 cron 生产者是 factory transport，注册名分别为 `admin-tasks` 与 `cron`。三条 cron 的真实排程在 `server/apalis.rs`：`tenant_expiry_scan = "0 * * * *"`（每小时整点）、`audit_log_archive = "30 3 * * *"`（每日 03:30）、`sys_tasks_periodic = "* * * * *"`（每分钟扫 `sys_tasks` 表）。

## 4. 横切系统地图

| 系统 | 一句话 | 实现位置 | 深读 |
|---|---|---|---|
| 认证 | JWT RS256（access + refresh HttpOnly Cookie 双 Cookie 轮换），口令应用层加密传输；认证阶段由框架 crate（rushwind-authn-gate）承载 | `crates/auth` + `services/.../token.rs`、`services/authentication/` | 第 5 章 |
| 授权（接口级） | authz 评估"该用户能否调该接口"，策略存 DB，每次评估落策略评估日志 | `crates/auth` + `services/.../authorizer.rs` | 第 5 章 |
| 租户隔离 | 数据层 Viewer 三态（平台/租户/系统）+ HTTP 层租户闸门（套餐模块白名单） | `apps/admin/src/admin/data/scope.rs` + `crates/auth` | 第 6 章 |
| 数据范围 | 角色级行过滤（五档），令牌承载聚合结果（`ds`/`dss` 声明） | 仓储谓词 + 令牌载荷 | 第 6 章 |
| 字段级权限 | 角色黑名单字段（`hfs` 声明），服务端响应裁剪 + 前端隐藏 | `services/admin_portal.rs`、`role.rs` | 第 5 章 |
| 审计日志 | 六类日志全覆盖：登录（含风控打分）/操作/API/数据访问/权限变更/策略评估 | `apps/admin/src/admin/audit/`（中间件层） | 第 7 章 |
| 异步任务 | Postgres 队列 + cron 生产者（系统任务 + sys_tasks 表驱动任务） | `apps/admin/src/admin/server/apalis.rs` | [docs/README.md](../README.md) 工程档案 |
| SSE 推送 | 站内信/通知推送；Hub 广播 + 逐收件人投递 | `apps/admin/src/admin/server/sse.rs` | [docs/README.md](../README.md) 工程档案 |
| 参数管理 | 平台全局参数键值（sys_config），缓存读取 + Redis 发布订阅多实例失效 | `apps/admin/src/admin/services/config.rs` | — |
| 对象存储 | MinIO（S3 兼容），五内容桶，签名图片代理 | `apps/admin/src/admin/state.rs` + `services/file.rs` | — |
| 质量门禁 | fmt / clippy / test / 契约同步四道 CI 门 + 差分回归台架 | `.github/workflows/ci.yml`、`backend/testbed` | 第 3 章 |

两个地图上容易误读的行，展开半页：

- **对象存储是可选面**：`AppState.oss` 是 `Option<OssBuckets>`——`oss.yaml` 有 MinIO 段才启用，五内容桶（`images`/`videos`/`audios`/`docs`/`files`，`OssBuckets::NAMES`）按 MIME 分桶建引擎，桶创建是 best-effort（已持有的桶返回冲突即忽略）；没有配置时文件服务落到本地磁盘镜像引擎（`rushwind-oss-local`，根目录 `./data/files/<bucket>`），元数据语义与 MinIO 面一致。第 2 章的本地起服流程依赖这一点：不起 MinIO 也能完整跑通。
- **授权与租户闸门的失败文本**：门的两条 403 消息来自检查器自身（租户阶段/授权阶段各自给出精确文本），与 401 的框架固定文本分开——登录页与前端按 reason 分文案渲染，这也是 reason 字段必须是稳定契约的原因之一。

## 5. 生成代码与手写代码的边界

这个仓库大量依赖代码生成。**分不清这条边界，改错文件是新人最常见的翻车点：**

| 位置 | 产生方式 | 能否手改 |
|---|---|---|
| `backend/apps/admin/api/protos/**` | `sync-protos.sh` 从上游契约源同步（MANIFEST 门） | ❌ 改上游契约源后重新同步 |
| 契约树的注解依赖（google.api 等） | buf 工作区按 `buf.lock` 钉定（上游 lock 是唯一版本事实） | ❌ 随上游 lock 同步 |
| `apps/admin/src/proto` 生成面（路由表/错误表/服务 trait/mounts/prost 类型） | `build.rs` 每次构建确定性再生 | ❌ 全部由 `api/` 工作区（admin 树 + fork 树）派生 |
| 前端 `src/api/generated/**` | 从契约生成的 TypeScript 客户端 | ❌ |
| `apps/admin/src/admin/services/**` | 手写（模式化样板） | ✅ |
| `apps/admin/src/admin/data/**` | 手写（实体/仓储/租户规则） | ✅ |
| `crates/auth`、`apps/admin/src/admin/server/**`、`apps/admin/src/admin/audit/**` | 手写（门、装配、审计层） | ✅ |

生成链路的完整机制在[第 3 章](./03-codegen-chain.md)，实战在[第 4 章](./04-first-service-module.md)。

### 5.1 两个 gen face 各自产出什么

构建在 `OUT_DIR` 里落两个 gen face 文件——admin face 与 fork face 同构，产物四类齐全。admin face（`target/.../out/admin_gen.rs`）文件头四行是它自己的自述：

```rust
// @generated by rushwind-gen-http from the embedding deployment's contract tree. DO NOT EDIT.
// Route, binding-plan, service-trait and error-table surface for the proto-HTTP annotation semantics
// semantics; the deployment's compatibility spec pins the wire contract.
// Corpus: 291 route bindings, 491 error-status entries.
```

**"Corpus: 291 route bindings, 491 error-status entries"** 是这份生成物的量词：291 = `ROUTES` 表的路由绑定条数（含 `additional_bindings`，即一条 rpc 可带多条 HTTP 绑定）；491 = `ERROR_STATUS` 表的 `(proto 包, reason 名) → HTTP 状态` 三元组条数（来自契约树各 `*_error.proto` 的 `(errors.code)` 注解，超出 100..=599 的状态值在生成期丢弃）。fork face（`template_gen.rs`）同构，Corpus 行是"5 route bindings, 6 error-status entries"。文件内部五个模块，各自的角色与消费者：

| 生成模块 | 内容 | 谁消费 |
|---|---|---|
| `error_tables::ERROR_STATUS` | `(包, reason, 状态)` 三元组表 × 491 | 手写访问器 `proto::tables::error_status` → `state::status_error` / 门失败信封 |
| `routes::{RouteSpec, BindLeaf, LeafKind, is_verb_on_resource, ROUTES}` | 291 条 `RouteSpec`（方法/路径/operation id/服务全名/输入输出类型全名/路径变量/body 策略/`shadowed` 标记），每条带逐字段**绑定计划** `query_leaves: &[BindLeaf]`（`path_json`/`path_proto` 双拼写、`LeafKind` 标量分组、oneof 归属、repeated/map 标记）+ `is_verb_on_resource` 守卫 | 绑定层 `bind_run`（按计划解 query/body/path）、守卫测试、差分台架 |
| `services::XxxServiceHandlers` | 59 个服务 trait、共 286 个 async 方法（每个注解 rpc 一个，async-trait 保 dyn 兼容） | service 层实现（`impl UserServiceHandlers for UserService`） |
| `nulls::null_xxx_service` | 每服务的占位实现（每个方法答 Unknown 兜底） | 编译完整性面/测试兜底 |
| `mounts::mount_xxx_service` | 59 个挂载函数：双路由桶线程，逐路由调 `wrap(...)` 组层、`glue::handle` 接 handler | `rest.rs` 的 `mount_service!` 表（§2.3） |

fork face（`gen::template`）是同构切片：5 条路由全部落在 `/template/v1/` 模块前缀下，6 条错误映射（notice 包），5 个 trait 方法；`apps/admin/tests/template_gen_surface.rs` 把这三个数字与前缀钉成守卫（路由数变了必须显式改测试），并断言模块路由绝不泄漏进部署白名单 `AUTH_FREE`。

**读生成物头部的习惯**值得养成：Corpus 行的数字就是路由面与错误面的体检表——契约同步后跑一次构建，两处数字变了就说明面变了，结合 `git diff api/MANIFEST.sha256` 能立刻定位变化来自哪次同步。生成物的逐面机制（双闭包编译、切片、模块路径重写）在[第 3 章](./03-codegen-chain.md)展开。

## 6. 本章小结

- proto 契约是唯一事实源，路由/错误表/服务接口/前端客户端全部由它派生；
- 请求路径：前端代理 → edge（CORS/预算）→ 绑定层 → 鉴权门（认证→租户→授权）→ 生成 handler → service → repo → 数据层租户规则 → DB；
- 绑定层外、门内的层序是线上契约（400 先于 401）；审计层挂最外（看得到最终状态码）；挂载对接只有一张 59 行的 `mount_service!` 表；
- 一个生命周期四个 transport：REST、SSE、任务 worker、cron 生产者；
- 先分清生成物与手写物，再动手改代码——admin face 与 fork face 两个生成文件各有 Corpus 头部可核对。

下一步：[02 · 从零跑起来](./02-get-it-running.md)。
