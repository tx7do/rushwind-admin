# Rust 后端：新增 CRUD 资源全流程

本文覆盖 rushwind-admin 后端（Rust workspace：`backend/crates/{proto,auth}` + `backend/services/admin-api` + `backend/testbed/admin-diff`）新增一个 CRUD 实体的完整流程。路径均相对仓库根。

**契约面与上游的关系必须先想清楚**：`backend/api/protos/` 是上游契约源的字节同步快照（116 个 proto，`MANIFEST.sha256` 门禁），buf 工作区头注释明令「快照禁手改，发现的问题回上游修」。Step 1 的两条路就是围绕这个事实展开的。

## 你要创建或编辑的文件

| 文件 | 动作 | 说明 |
|---|---|---|
| `backend/api/protos/<domain>/service/v1/<entity>.proto` | 新建/同步 | domain 层契约（祝福流来自上游） |
| `backend/api/protos/admin/service/v1/i_<entity>.proto` | 新建/同步 | BFF 层契约（http 注解在这层） |
| `backend/api/MANIFEST.sha256` | 重建/补行 | 字节清单门，禁手改既有行 |
| `backend/services/admin-api/src/data/<domain>.rs` | 新建/追加 | SeaORM 实体 |
| `backend/services/admin-api/src/data.rs` | +2 行 | `mod <domain>;` + `pub use` |
| `backend/services/admin-api/src/migration.rs` | 追加批次 | 新表一个迁移号 |
| `backend/services/admin-api/src/mapping.rs` | 追加表 | enum 列的 DB 字面量 ↔ wire 枚举 |
| `backend/services/admin-api/src/data/repos/<entity>.rs` | 新建 | `repo_shell!` 一行 + 特有谓词 |
| `backend/services/admin-api/src/data/repos/mod.rs` | +2 行 | `mod` + `pub use` |
| `backend/services/admin-api/src/services/<entity>.rs` | 新建 | 行映射 + `upsert_fields` + `impl *Handlers` |
| `backend/services/admin-api/src/services.rs` | +2 行 | `mod` + `pub use` |
| `backend/services/admin-api/src/server/rest.rs` | +2 处 | import + `mount_services!` 表行 |
| `backend/services/admin-api/src/seed/menu_data.rs` | 追加行 | 菜单 seed（parent 在前） |
| `backend/services/admin-api/src/seed/mod.rs` | 可选 | 默认数据种子 |

**无需动**：`backend/crates/proto/src/auth_free.rs`（管理端 CRUD 默认全 gated）、任何生成物（OUT_DIR 不进 git）、Makefile（本仓没有——全是裸 cargo 命令 + bash 脚本）。

## Step 1 — 契约面（两条路）

### 祝福流：上游先落，同步进来

契约源新增/修改 domain + BFF 两个 proto 后，本仓整树重同步：

```bash
# Linux 上默认源是 Windows 路径（/d/GoProject/...），必须用 env 覆盖；
# 本机克隆在 ~/Github/go-wind-admin 时：
GOWIND_ADMIN_API_DIR="$HOME/Github/go-wind-admin/backend/api/protos" \
  bash backend/api/sync-protos.sh
```

sync 模式会 `rm -rf protos/` 整树复制（BOM 剥除归一化）并重建 `MANIFEST.sha256`（`export LC_ALL=C` 钉死排序）。diff 即显式接受新 vintage。

### 本地直加流（债务，见 SKILL.md 坑 7）

上游暂时不收时，手写同样两个 proto 进 `backend/api/protos/`，再按同款归一化补清单行：

```bash
cd backend/api
printf '%s  %s\n' "$(sha256sum protos/<domain>/service/v1/<entity>.proto | cut -d' ' -f1)" \
  "<domain>/service/v1/<entity>.proto" >> MANIFEST.sha256
# BFF 行同理；然后 LC_ALL=C sort 校验顺序与既有清单一致
```

下次上游同步会把本地独有文件抹掉——除非上游明确不收，否则别走这条。

### proto 形状（照抄结构，改名字段）

**domain 层** `protos/<domain>/service/v1/<entity>.proto`，package `<domain>.service.v1`。样板：`protos/notification/service/v1/notification_rule.proto`。

- 实体 message：字段全部 `optional` + `json_name = "camelCase"`；字段号分段——业务字段 1..20+，`created_by`=100 / `updated_by`=101，`created_at`=200 / `updated_at`=201。
- 域内 service：rpc 用**裸动词**——`List(pagination.PagingRequest) returns (List<Entity>Response)`、`Get`、`Create`、`Update`、`Delete`（自定义动作如 `TestDispatch`）；**不带 google.api.http 注解**。
- Create/Update 请求：`{ data: <Entity> = 1; }` 包裹 + `update_mask`（google.protobuf.FieldMask）。分页信封来自 BSR 依赖 `buf.build/tx7do/pagination`。

**BFF 层** `protos/admin/service/v1/i_<entity>.proto`，package `admin.service.v1`，import domain proto。样板：`protos/admin/service/v1/i_notification_rule.proto`。

- service 同名，rpc 命名 `<Verb><Entity>`（`ListNotificationRule` / `GetNotificationRule` / ...），每条 rpc 带 `option (google.api.http)`：
  - 列表 `get: "/admin/v1/<kebab-plural>"`；单查 `get: ".../{id}"`；创建 `post: "..." body: "*"`；更新 `put: ".../{id}" body: "*"`；删除 `delete: ".../{id}"`；自定义动作 `post: ".../{id}/<action>"`（如 `test-dispatch`）。
- 错误面在 `protos/admin/service/v1/admin_error.proto`（`(errors.code)` 注解 → reason→HTTP 状态表）。

## Step 2 — buf 校验

```bash
cd backend/api
buf lint                            # 仅报告——Kratos 式命名整体违反 STANDARD，
                                    # 问题回上游修，本仓不手改快照
buf build -o /tmp/api-image.binpb   # 编译全闭包（含注解闭包）；首次按 buf.lock 拉六个 BSR 模块
buf breaking --against "../../.git#branch=main,subdir=backend/api"
                                    # 接受新 vintage 前的演进检查（FILE 规则：仅删除/改型算破坏）
```

**禁止 `buf format`**——会砸字节清单门。buf 在本机 `~/.local/bin/buf`（或 `~/.cargo/bin/buf`，以 `command -v buf` 为准）。

## Step 3 — 重生成 Rust 契约面

```bash
cd backend && cargo build -p proto
```

build.rs 的 `rerun-if-changed` 挂在 `backend/api/protos`、`buf.yaml`、`buf.lock`、`auth_free.rs`——**改完契约直接 build 即重生成，没有手工生成命令**。生成物落 OUT_DIR（`backend/target/debug/build/proto-<hash>/out/`），经 `crates/proto/src/lib.rs` 暴露：

| 生成面 | 内容 | 你将在手写层引用它为 |
|---|---|---|
| 类型 | prost struct + pbjson protojson serde，每 package 一模块 | `proto::proto::<domain>::service::v1::{Entity, ListEntityResponse, ...}` |
| `routes::ROUTES` | 全部 `RouteSpec`（生成头 `Corpus:` 计数，当前 211 条路由 / 441 条错误状态） | 只读；守卫测试钉 |
| `error_tables::ERROR_STATUS` | `(package, reason, status)` 表 | 经 `crate::proto::error_status` 间接用 |
| `services::*Handlers` | 每 BFF service 一个 async trait，方法为 rpc 名 snake_case | `impl proto::gen::services::ProductServiceHandlers` |
| `nulls::null_*_service()` | null 桩构造器（应答 500/not-implemented） | 中间态的合法挂载物 |
| `mounts::mount_*_service()` | 按免鉴权白名单分 public/gate 两个 Router 的挂载函数 | `mount_services!` 表行 |

生成头注释（`admin_gen.rs` 顶部 `// @generated by rushwind-gen-http ...`）是路由/错误计数的权威出处。此步完成后即使什么都不写，`cargo build -p admin-api` 也应通过——新路由由 null 桩应答。

## Step 4 — 免鉴权判定（默认不动）

管理端 CRUD 默认全 gated，**不要碰** `backend/crates/proto/src/auth_free.rs`。只有真正的公开端点（如登录、验证码）才加 `(service_fq, RpcName)` 对，且必须同步更新 `backend/crates/proto/tests/whitelist.rs` 的 `EXPECTED`——两表 1:1 双向钉死，漏一边测试红。

## Step 5 — SeaORM 实体

新建 `backend/services/admin-api/src/data/<domain>.rs`（既有域就追加），样板：`data/notification.rs`。

```rust
pub mod sys_products {
    use sea_orm::entity::prelude::*;
    #[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel)]
    #[sea_orm(table_name = "sys_products")]
    pub struct Model {
        pub id: u32,
        // 业务字段全部 Option<...>，与 proto 的 optional 对齐
        pub name: Option<String>,
        /// enum(DRAFT,PUBLISHED) default DRAFT.   // enum 列必须这样注释
        pub status: Option<String>,
        pub created_by: Option<u32>,
        pub updated_by: Option<u32>,
        pub created_at: Option<DateTime>,
        pub updated_at: Option<DateTime>,
    }
    #[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
    pub enum Relation {}
    impl ActiveModelBehavior for ActiveModel {}
}
```

注册进 `data.rs`：`mod product;` + `pub use product::sys_products;`。

## Step 6 — 迁移批次

`backend/services/admin-api/src/migration.rs` 的 `Migrator::migrations()` **追加**新批次（init 批次只读不改）。样板：notification 批次 `EntityTables::new("m20250923_000001_notification", DatabaseBackend::Postgres).table::<data::sys_notification_rules::Entity>()...`。

- 迁移号：`m<YYYYMMDD>_<序号>_<名>`。
- 既有存量表补列不走 `EntityTables`：照 `ChannelWebhookColumns` 样板（`execute_unprepared("ALTER TABLE sys_notification_channels ADD COLUMN IF NOT EXISTS webhook_url, ...")`）。

## Step 7 — 枚举映射

有 enum 列就在 `backend/services/admin-api/src/mapping.rs` 加 `enum_map!` 表：DB 字面量 → wire 枚举是 `notification_event_type_of`，反向 `+ rev` 生成 `_str`。文件内已有 2 个 round-trip 测试兜底，新表自动被覆盖的写法以现有表为准。

## Step 8 — repo

新建 `backend/services/admin-api/src/data/repos/<entity>.rs`，注册进 `data/repos/mod.rs`（`mod` + `pub use`）。

**平台全局表**（绝大多数）——首行宏，随后只写特有谓词：

```rust
repo_shell!(global ProductRepo, entity, "product");

impl ProductRepo {
    // 特有谓词，样板 data/repos/notification_rule.rs 的 find_by_event_type
}
```

global noun 臂自动生成 `get_by_id`（404 = `"<noun> not found"`，走 `state::not_found`）与 `delete_by_id`。宏三臂（`tenant` / `global` / `global noun`）定义在 `data/repos/mod.rs` 顶部，臂语义注释就在宏上方。

**租户域**——用 `repo_shell!(tenant ProductRepo, entity);`，且**刻意没有 noun 臂**：租户的 get/delete 必须带 viewer 谓词，`find_scoped`/`delete_scoped`/`paged_list` 手写，样板 `data/repos/internal_message_category.rs`（走 `crate::paging::fetch_paged`，`Viewer` 来自 `data/scope.rs`）。

## Step 9 — service

新建 `backend/services/admin-api/src/services/<entity>.rs`，样板：`services/notification_rule.rs`。四件套：

1. **行映射** `fn rule_proto(Model) -> proto Entity`：enum 列经 `mapping::*_of` 转 wire 枚举，时间经 `state::naive_to_ts`。
2. **反向映射** `fn upsert_fields(&mut ActiveModel, &Entity)`：`if let Some` 逐字段（populated 才写，None 即不动）；enum 反查 `mapping::*_str`，未知值 `status_error("BAD_REQUEST", ...)`。
3. **服务结构**：`pub struct ProductService { pub state: Arc<AppState> }`。
4. ** Handlers 实现**：

```rust
#[async_trait::async_trait]
impl proto::gen::services::ProductServiceHandlers for ProductService {
    async fn create_product(&self, ctx: RequestContext, req: CreateProductRequest)
        -> Result<Product, StatusError> {
        let payload = operator_of(&ctx)?;
        let data = require_data(req.data)?;
        // ... ActiveModel 组装 + insert
    }
}
```

**helper 一览**（全部来自 `crate::state` / `crate::macros`，别自己造）：

| helper | 用途 |
|---|---|
| `require_data(req.data)?` | CRUD 的 `data` 包裹解包，缺失 → BAD_REQUEST |
| `operator_of(&ctx)?` | 操作者（审计字段 100/101 来源） |
| `tenant_of(&ctx)` | 租户 id（租户域 list 显式参数） |
| `not_found(what)` / `status_error(reason, msg)` / `internal_error` / `db_err` | 错误面（信封四字段规范见 `docs/binding-spec.md` §4） |
| `stamp_update!` | 更新时盖 `updated_by/updated_at` |
| `query_by_id!(req.query_by, GetQueryBy)` | Get/Delete 的 query_by 解包；`GetQueryBy` 是生成类型嵌套 enum 的别名，样板 `services/internal_message_category.rs` |

Update 的完整形状（唯一性校验、错误序）：照抄 `notification_rule.rs::update_notification_rule`。

## Step 10 — 注册与挂载

- `services.rs`：`mod product;` + `pub use product::ProductService;`（41 个服务一表，加一行）。
- `server/rest.rs`：use 区加 `ProductService`，`mount_services!` 表加行：

```rust
(mount_product_service, ProductService),
```

宏展开即 `proto::gen::mounts::mount_product_service(router_pub, router_gate, Arc::new(ProductService { state }), &wrap)`。**漏行的症状：路由存在（ROUTES 里有）、但永远 500——null 桩在应答。**

## Step 11 — seed

**菜单**：`seed/menu_data.rs` 的 `ROWS: [MenuSeedRow; N]` 追加行。字段：`{id, parent, menu_type, path, name, component, module, title, order}`。铁则：**parent 行在前**（同批插入顺序即父先子后）；id 不与既有 34 行冲突；component 是前端快照内的页面路径（如 `notification/rule/index`）。样板行：id 74（`/notification` CATALOG）→ 68/73（channels/rules 页）。seed 由 `seed/mod.rs::run()` 在启动期执行，`count == 0` 守卫意味着**只有空库生效**——已有部署靠各端「菜单同步」补行。

**默认数据**（可选）：需要预置行就在 `seed/mod.rs` 加 `seed_products`（`insert_seed!` 宏）并挂进 `run()`。登录种子账号 `admin / Abcd@1234`（`DEFAULT_USER_PASSWORD`）不要动。

## Step 12 — 门禁与冒烟

CI（`.github/workflows/ci.yml`，ubuntu+windows 矩阵）六门即本地命令序列：

```bash
cd backend
bash api/sync-protos.sh --check                                   # 契约清单门
bash ../frontend/admin/sync-frontend.sh react --check             # 三端快照门（各跑一次）
bash ../frontend/admin/sync-frontend.sh vue-element --check
bash ../frontend/admin/sync-frontend.sh vue-vben --check
buf lint                                                          # 仅报告（CI 上 continue-on-error）
cargo fmt -p proto -p auth -p admin-api -p admin-diff -- --check  # scoped，不波及框架 path-deps
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace                                            # 当前 53 个：proto 19 + admin-api 21 + admin-diff 13
```

**冒烟**（本地依赖：`backend/testbed/docker-compose.yml`，Postgres 密码 `*Abcd123456`、**库名必须 `gwa`** 与 DSN 对齐、Redis 同密码）：

```bash
RUSHWIND_DATABASE_SOURCE="host=127.0.0.1 port=5432 user=postgres password=*Abcd123456 dbname=gwa sslmode=disable" \
RUSHWIND_REDIS_ADDR=127.0.0.1:6379 RUSHWIND_REDIS_PASSWORD='*Abcd123456' \
  cargo run -p admin-api            # :7788
```

1. `GET /admin/v1/captcha` 拿 `captchaId`；
2. Redis 直读答案：`docker exec <redis> redis-cli -a '*Abcd123456' GET "admin:captcha:<captchaId>"`（key 形状见 `captcha.rs::captcha_key`，验证即删）；
3. `POST /admin/v1/login`（`admin / Abcd@1234` + 验证码）拿 JWT；
4. 带 `Authorization` 请求新路由，验 list/get/create/update/delete 全链路。

## Step 13 — 文档计数翻新

路由/端点总数变化时（先例 commit 43acfeb）：

- 三语 README：`README.md` / `README.en-US.md` / `README.ja-JP.md` 的路由计数；
- `docs/binding-spec.md` §1.2 的计数；
- `backend/testbed/README.md`（语料描述）。

数字以生成头 `Corpus:` 为权威出处，别手算。

## Backend-specific pitfalls

1. **快照禁手改**：`backend/api/protos/` 任何手改都会让 `sync-protos.sh --check` exit 1，门禁必红。
2. **Linux 上 sync 必须带 `GOWIND_ADMIN_API_DIR`**：脚本默认源是 Windows 路径 `/d/GoProject/...`，不带 env 直接跑会报 source not found。
3. **禁止 `buf format`**：同理砸字节清单门。lint 只报告不修。
4. **租户域别图省事用 global noun 臂**：租户表用错臂会生成不带 viewer 谓词的 get/delete——跨租户越权读取。
5. **枚举裸字符串 = 迟早红**：DB 字面量与 wire 枚举名的翻译只在 `mapping.rs`；service 里直接 `to_string()`/`from_str()` 绕过 round-trip 测试的保护。
6. **`upsert_fields` 忘写新字段**：proto 加了字段、实体加了列、但 `upsert_fields` 没加 `if let Some` 分支——字段永远写不进去，接口 200 但数据不落。改字段先对照 Step 9 四件套逐一检查。
7. **`AUTH_FREE` 与 `EXPECTED` 漂移**：只改 `auth_free.rs` 不改 `tests/whitelist.rs`，测试必红；两表 1:1。
8. **别拿 `cargo build` 当门禁**：build 过 ≠ fmt/clippy/test 过。声明完成前跑全 Step 12 序列。
