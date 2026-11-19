# 04 · 第一个服务模块

> 前置：[03 契约与生成链路](./03-codegen-chain.md)。读完你将亲手跑通一遍完整开发闭环：契约落树 → 实现 → 挂载 → 验证。
>
> 本仓的契约统一在上游契约源维护、经同步脚本进仓，所以"新增模块"的起点是**契约树里出现了新服务**（而不是本仓写 proto）。本章以契约树里真实存在的 `LanguageService`（语言管理，最小的完整模块之一）为样本，走一遍 Rust 侧的实现闭环——今后每个新模块都是同一套动作。
>
> 权威规格指针：绑定语义见 [binding-spec.md](../binding-spec.md)，租户谓词见 [第 6 章](./06-multi-tenant-isolation.md)，权限点见 [第 5 章](./05-permission-model.md)。本章把"跟着做要碰到的细节"直接写在正文里，规格层负责裁定争议。

## 1. 契约落树，构建出骨架

同步后 `backend/apps/admin/api/protos/<模块>/` 里出现该服务的 proto。一个服务在契约树里是**两个文件**，各管一头：

- **能力面** `api/protos/dict/service/v1/language.proto`：定义消息与 rpc（`List / Count / Get / Create / BatchCreate / Update / Delete` 七个），以及 `Language` 消息本身的字段表——**每个字段都带 `optional`**（presence 跟踪），`id` 是 `uint64`，时间戳是 `google.protobuf.Timestamp`；
- **装配面** `api/protos/admin/service/v1/i_language.proto`：admin 面要暴露哪些 rpc，每个 rpc 挂 `google.api.http` 注解给出 HTTP 形态：

```proto
service LanguageService {
  rpc List (pagination.PagingRequest) returns (dict.service.v1.ListLanguageResponse) {
    option (google.api.http) = { get: "/admin/v1/dict/langs" };
  }
  rpc Get (dict.service.v1.GetLanguageRequest) returns (dict.service.v1.Language) {
    option (google.api.http) = { get: "/admin/v1/dict/langs/{id}" };
  }
  rpc Create (dict.service.v1.CreateLanguageRequest) returns (google.protobuf.Empty) {
    option (google.api.http) = { post: "/admin/v1/dict/langs" body: "*" };
  }
  rpc Update (dict.service.v1.UpdateLanguageRequest) returns (google.protobuf.Empty) {
    option (google.api.http) = { put: "/admin/v1/dict/langs/{id}" body: "*" };
  }
  rpc Delete (dict.service.v1.DeleteLanguageRequest) returns (google.protobuf.Empty) {
    option (google.api.http) = { delete: "/admin/v1/dict/langs" };
  }
  rpc BatchCreate (dict.service.v1.BatchCreateLanguagesRequest) returns (google.protobuf.Empty) {
    option (google.api.http) = { post: "/admin/v1/dict/langs/batch" body: "*" };
  }
}
```

（节选自 `i_language.proto`，共 6 条路由。能力面的 `Count` 没进装配面，就没有 HTTP 形态——装配面是路由的最终裁决。）

`cargo build` 时生成链自动产出（见[第 3 章](./03-codegen-chain.md)）：

- 类型：`crate::proto::dict::service::v1::{Language, ListLanguageResponse, ...}`
- trait：`crate::proto::gen::services::LanguageServiceHandlers`（286 个方法之一）
- 挂载：`crate::proto::gen::mounts::mount_language_service`

此刻服务还不可用：trait 没有实现、挂载没有被调用。漏实现/漏挂载会在守卫测试与台架 sweep 里现形。

**路由与查询参数的绑定计划也是生成的**。生成器为每条路由产出 `RouteSpec`：方法、路径模板、请求/响应类型全名、body 模式，以及 query 叶子表。`List` 的入参是 `pagination.PagingRequest`，它的 query 叶子（生成代码 `admin_gen.rs`）：

```text
page → U32（json_name: page）
page_size → U32（json_name: pageSize）
offset → U64、limit → U32、token → Str
no_paging → Bool（json_name: noPaging）
query / filter → Str（filtering_type oneof 的两个成员）
order_by → Str、sorting → repeated、field_mask → FieldMask
```

所以 `GET /admin/v1/dict/langs?page=1&page_size=10` 与 `?page=1&pageSize=10` 是同一件事——proto 名与 camel json_name 两种拼法都解析（归一规则见 [binding-spec §2.2](../binding-spec.md)）。

## 2. 数据层：实体 + 仓储

### 2.1 实体

每张表一个 SeaORM 实体模块，按域分组在 `apps/admin/src/admin/data/` 下的十二个域文件里（ai / audit / bi / dict / identity / message / monitor / notification / ops / platform / print / rbac）。`src/admin/data/dict.rs` 里的 `sys_languages` 全文如下（这就是一张表实体的全部内容）：

```rust
pub mod sys_languages {
    use sea_orm::entity::prelude::*;

    #[derive(Clone, Debug, PartialEq, DeriveEntityModel)]
    #[sea_orm(table_name = "sys_languages")]
    pub struct Model {
        #[sea_orm(primary_key)]
        pub id: i64,
        pub language_code: String,
        pub language_name: String,
        pub native_name: Option<String>,
        #[sea_orm(default_value = false)]
        pub is_default: Option<bool>,
        #[sea_orm(default_value = true)]
        pub is_enabled: Option<bool>,
        #[sea_orm(default_value = 0)]
        pub sort_order: Option<u32>,
        pub created_by: Option<i64>,
        pub updated_by: Option<i64>,
        pub deleted_by: Option<i64>,
        pub created_at: Option<chrono::NaiveDateTime>,
        pub updated_at: Option<chrono::NaiveDateTime>,
        pub deleted_at: Option<chrono::NaiveDateTime>,
    }

    #[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
    pub enum Relation {}

    impl ActiveModelBehavior for ActiveModel {}
}
```

读法要点：

- 主键统一 `i64`（proto 侧对应 `uint64`，映射函数里 `as u64` 换乘，见 §3.2）；
- 业务列是否可空与 proto 的 `optional` 一一对应；`#[sea_orm(default_value = ...)]` 是建库 DDL 的默认值注记；
- 审计三对列（`created_by/at`、`updated_by/at`、`deleted_by/at`）是统一模板——服务层通过 `stamp_update!` 等助手盖章，不手搓；
- `deleted_at` 是软删列。`sys_languages` 是平台全局表，没有 `tenant_id` 列；租户表（如 `sys_roles`）才带该列，由 Viewer 盖章（[第 6 章](./06-multi-tenant-isolation.md)）。

新表要同时注册进 `apps/admin/src/admin/migration.rs` 的 `EntityTables` 链（启动迁移按它建表）；测试内存库走同一清单（`SqliteTestMigrator`，见 §5.1）。

### 2.2 仓储：一仓一文件

仓储是一仓一文件（`apps/admin/src/admin/data/repos/language.rs`）。语言是全局表，所以用 `repo_shell!` 的 **global 臂 + 名词臂**，一行展开出整个仓储骨架：

```rust
repo_shell!(global LanguageRepo, entity, "language");
```

这一行宏展开后得到（展开体在 `apps/admin/src/admin/data/repos/mod.rs`，此处按 global 名词臂逐段对照）：

- **struct + 构造**：`pub struct LanguageRepo<'a> { pub db: &'a sea_orm::DatabaseConnection }`，`LanguageRepo::new(&db)`——注意 global 臂**没有 viewer 字段**（没有租户谓词可派生）；
- **软删谓词**：`fn condition()` 生成 `deleted_at IS NULL`——软删行在所有列表读面隐身，这是宏替你写的，不是每个仓储自觉；
- **两个列表信封**：`list()`（不分页全量）与 `paged_list(&PagingRequest)`（返回 `(rows, total)` 二元组）；
- **两个 id 寻址方法**（名词臂独有）：`get_by_id(id)`——查无或软删时返回 `not_found("language")`（404，文案 `"language not found"`，名词就是宏第三个参数）；`delete_by_id(id)`——删掉后 `Ok(())`，"行不存在算不算错"由调用方决定。

租户臂（`repo_shell!(tenant XxxRepo, entity)`）的差异在 [第 6 章](./06-multi-tenant-isolation.md)：struct 多一个 `viewer` 字段，`condition()` 在 `deleted_at IS NULL` 之上再叠 `tenant_id = tid`。**没有"租户名词臂"**——租户表的 get/delete 必须带着 viewer 谓词，所以那些方法留在各仓储文件的手写 `impl` 里。

repo 专属方法写在同一文件的 `impl LanguageRepo<'a>` 块内。language.rs 只加了两个：

```rust
impl<'a> LanguageRepo<'a> {
    /// The id behind a language-code lookup (Get's query_by=code).
    pub async fn find_id_by_code(&self, code: &str) -> Result<Option<u64>, StatusError> {
        entity::Entity::find()
            .filter(entity::Column::LanguageCode.eq(code))
            .one(self.db)
            .await
            .map_err(db_err)
            .map(|row| row.map(|row| row.id as u64))
    }

    /// One insert, shared by create and batch_create.
    pub async fn insert(&self, row: entity::ActiveModel) -> Result<entity::Model, StatusError> {
        ActiveModelTrait::insert(row, self.db).await.map_err(db_err)
    }
}
```

两条纪律：谓词写 `filter(...)` 不裸写 SQL 片段；错误一律 `map_err(db_err)` 归一，仓储只做数据访问，不做业务判断。

### 2.3 分页管线：PagingRequest → Params → SQL

`paged_list` 把请求原样交给 `crate::paging::fetch_paged`——这是框架管线（`rushwind-storage-seaorm-support`）的部署桥。三段：

1. **类型换乘**（`apps/admin/src/admin/proto/mod.rs`）：生成类型不能在框架 crate 里实现 trait（孤儿规则），所以 `PagingInput for PagingRequest` 写在本仓：

   ```rust
   impl rushwind_storage_seaorm_support::paging::PagingInput
       for crate::proto::pagination::PagingRequest
   {
       fn paging_params(&self) -> ...Params {
           ... // query/order_by/sorting（direction == 1 即 DESC）/page/page_size/
               // offset/limit/no_paging（unwrap_or(false)）逐项搬进 Params
       }
   }
   ```

   `PagingRequest` 的完整字段集：`page`（默认 1）、`page_size`（默认 10）、`offset`/`limit`（游标式二选一）、`token`（游标）、`no_paging`、`order_by`、`sorting`、`field_mask`、`filtering_type`（oneof：`query` 字符串或 `filter`）。

2. **谓词与排序**（框架 `apply_filters_and_order`）：`filtering_type` 里的 `query` 字符串按解析器拆成 (field, op, value) 三元组，按**列类型分类**（`column_kind`：数值/布尔/文本）绑定成 SQL 谓词——数值列收 `"5"` 会转数、文本列恒按文本比，类型错配直接不出条件；排序缺省回落到 `id`，保证分页稳定。

3. **切片与计数**（框架 `fetch_paged`）：

   ```rust
   let total = if params.no_paging {
       None
   } else {
       Some(assembled.clone().count(db).await.map_err(db_err)?)   // count 先行
   };
   let rows = apply_slicing(assembled, &params).all(db).await.map_err(db_err)?;
   let total = total.unwrap_or(rows.len() as u64);                 // no_paging 折叠
   ```

   切片规则：`page`/`page_size` 下限钳到 1；`offset`/`limit` 携带任一即走游标式；`no_paging=true` 时 `(0, u64::MAX)` 且不 `limit`——repo 测试里 `no_paging` 返回全部 12 行、total 折叠为行数，钉的就是这段。

仓储调用点只写 `repo.paged_list(&req).await?`，请求对象**原样透传**，不拆不装。

## 3. 服务层：实现生成的 trait

### 3.1 骨架与映射函数

`apps/admin/src/admin/services/language.rs` 全文不到 170 行（含测试），先是映射函数与服务结构：

```rust
fn language_proto(r: crate::admin::data::sys_languages::Model) -> Language {
    Language {
        id: Some(r.id as u64),
        language_code: Some(r.language_code),
        language_name: Some(r.language_name),
        native_name: r.native_name,
        is_default: r.is_default,
        is_enabled: r.is_enabled,
        sort_order: r.sort_order,
        created_by: r.created_by.map(|v| v as u64),
        updated_by: r.updated_by.map(|v| v as u64),
        deleted_by: r.deleted_by.map(|v| v as u64),
        created_at: r.created_at.and_then(crate::admin::state::naive_to_ts),
        updated_at: r.updated_at.and_then(crate::admin::state::naive_to_ts),
        deleted_at: r.deleted_at.and_then(crate::admin::state::naive_to_ts),
    }
}

pub struct LanguageService {
    pub state: Arc<AppState>,
}
```

要点：

- **服务是薄层**：一个服务一个 struct，只有 `state: Arc<AppState>` 一个字段（配置、连接池、Redis、JWT 引擎、令牌库都挂在 state 上）；
- **entity → proto 映射是显式函数**：逐字段转换，`i64 → u64` 换乘、时间戳 `Option<NaiveDateTime> → prost_types::Timestamp`（`naive_to_ts`）。漏字段不报错，是静默零值——新增字段时两个类型都要动（这是模式化骨架最大的坑）；
- **id 的 64 位换乘**：数据库 `i64`，wire `uint64`。protojson 把 64 位整数编码为**字符串**（见 §4），所以前端拿到的 id 永远是 `"1"` 这样的字符串，直接参与下一次请求的 `{id}` 路径变量即可。

### 3.2 六个 handler 逐个看

trait 的实现全是同一个节奏：取上下文 → 构造 repo → 调用 → 映射返回。以下逐段（均在 `impl LanguageServiceHandlers for LanguageService` 内）：

**list**——最薄的一个：

```rust
async fn list(&self, _ctx: RequestContext, req: PagingRequest)
    -> Result<ListLanguageResponse, StatusError>
{
    let repo = crate::admin::data::repos::LanguageRepo::new(&self.state.db);
    let (rows, total) = repo.paged_list(&req).await?;
    Ok(ListLanguageResponse {
        items: rows.into_iter().map(language_proto).collect(),
        total,
    })
}
```

`_ctx` 下划线：list 不需要操作者身份。`PagingRequest` 由绑定层从 query 装好后递进来，handler 不碰原始 query 串。

**get**——双寻路 + 错误分型：

```rust
async fn get(&self, _ctx: RequestContext, req: GetLanguageRequest)
    -> Result<Language, StatusError>
{
    let repo = crate::admin::data::repos::LanguageRepo::new(&self.state.db);
    let id = match req.query_by {
        Some(get_language_request::QueryBy::Id(id)) => id,
        Some(get_language_request::QueryBy::Code(code)) => {
            repo.find_id_by_code(&code).await?
                .ok_or_else(|| not_found("language"))?
        }
        None => return Err(status_error("BAD_REQUEST", "query_by required")),
    };
    Ok(language_proto(repo.get_by_id(id as i64).await?))
}
```

`query_by` 是 oneof：按 id 或按 code。按 code 先查 id 再走 `get_by_id`——两段都可能 404，文案统一走 `not_found("language")`。oneof 缺失（比如 query 串什么都没带）直接 `BAD_REQUEST`。

**create**——操作者戳 + 默认值兜底：

```rust
async fn create(&self, ctx: RequestContext, req: CreateLanguageRequest)
    -> Result<Empty, StatusError>
{
    let payload = operator_of(&ctx)?;
    let data = crate::admin::state::require_data(req.data)?;
    crate::admin::data::repos::LanguageRepo::new(&self.state.db)
        .insert(crate::admin::data::sys_languages::ActiveModel {
            language_code: Set(data.language_code.unwrap_or_default()),
            language_name: Set(data.language_name.unwrap_or_default()),
            native_name: Set(data.native_name),
            is_default: Set(data.is_default.or(Some(false))),
            is_enabled: Set(data.is_enabled.or(Some(true))),
            sort_order: Set(data.sort_order.or(Some(0))),
            created_by: Set(Some(payload.user_id)),
            created_at: Set(Some(crate::admin::data::now())),
            updated_at: Set(Some(crate::admin::data::now())),
            ..Default::default()
        })
        .await?;
    Ok(Empty {})
}
```

三个标准动作：`operator_of(&ctx)?` 从 claims 解出操作者（解不出 → 401 `missing identity`）；`require_data` 缺载荷 → 400 `data required`；`created_by`/`created_at`/`updated_at` 服务端盖章。返回 `google.protobuf.Empty`——创建不回读实体，前端随后自己 list/get。

**batch_create**——同一插入体循环复用：

```rust
for data in req.items {
    repo.insert(... /* 同 create 的 ActiveModel */).await?;
}
```

差别一处：`created_by: Set(operator_of(&ctx).ok().map(|p| p.user_id))`——批量面操作者缺省不报错（`.ok()` 吞掉），逐条照插。批量没有事务包裹，中途失败即前面的已落库（契约如此，别在这里自作主张加事务）。

**update**——先取行、再盖字段、最后统一戳：

```rust
async fn update(&self, ctx: RequestContext, req: UpdateLanguageRequest)
    -> Result<Empty, StatusError>
{
    let payload = operator_of(&ctx)?;
    let repo = crate::admin::data::repos::LanguageRepo::new(&self.state.db);
    let mut a: crate::admin::data::sys_languages::ActiveModel =
        repo.get_by_id(req.id as i64).await?.into();      // 查无 → 404
    if let Some(data) = &req.data {
        let mask = crate::fieldmask::FieldMask::from(req.update_mask.as_ref());
        Self::apply_fields(&mut a, data, &mask);          // 白名单门（§6）
    }
    crate::stamp_update!(a, payload.user_id);             // updated_by/at 一起动
    a.update(&self.state.db).await.map_err(db_err)?;
    Ok(Empty {})
}
```

update 的次序是刻意安排的：`get_by_id` 在前——不存在的 id 第一跳就 404；`apply_fields` 只动掩码放行的字段；`stamp_update!` 宏无条件把 `updated_by` 挪到操作者、`updated_at` 落到 now（两者总是成对，这是宏存在的意义）。

**delete**——id-only 的 oneof 抽取：

```rust
async fn delete(&self, _ctx: RequestContext, req: DeleteLanguageRequest)
    -> Result<Empty, StatusError>
{
    let id = crate::query_by_id!(
        req.query_by,
        crate::proto::dict::service::v1::delete_language_request::QueryBy
    );
    crate::admin::data::repos::LanguageRepo::new(&self.state.db)
        .delete_by_id(id as i64)
        .await?;
    Ok(Empty {})
}
```

`query_by_id!` 宏把 oneof 拆到 `Id` 臂，其余形态统一回 `BAD_REQUEST "query_by required"`。注意删除的两种语义在本仓并存：语言这类字典表走名词臂的 `delete_by_id`——SeaORM `delete` 语句**物理删行**（repo 测试钉住：删后 `get_by_id` 404、`find_id_by_code` 也查不到，行是真没了）；而租户、岗位这类带"停车"语义的表在仓储方法里盖 `deleted_at` / `deleted_by` 走软删进回收站（[第 6 章](./06-multi-tenant-isolation.md)）。软删谓词 `deleted_at IS NULL` 由 `repo_shell!` 铺在全部读面上，两种删除在"读面消失"这一效果上等价。

### 3.3 错误映射：信封的三条来路

服务返回 `Result<_, StatusError>`，信封在编码层落成 wire 上的四字段 JSON。三条来路：

| 助手（`state.rs`） | 产生点 | wire 形态 |
|---|---|---|
| `db_err`（框架桥 re-export） | 仓储所有 `map_err(db_err)` | 500，reason 空串（Unknown 分支），`message` 带 `db: …` 前缀 |
| `not_found("language")` | 查无/软删命中 | 404，`NOT_FOUND`，`"language not found"` |
| `status_error(reason, msg)` | 手工构造任意 reason | reason 经 `error_status` 静态表查 HTTP 码，查不到兜底 500 |

`not_found("language")` 在 wire 上的完整字节（金样级形状，字段序 = errors.proto 字段序 1..4）：

```json
{"code":404,"reason":"NOT_FOUND","message":"language not found","metadata":{}}
```

reason → HTTP 码的映射表来自契约树里的 `admin_error.proto` 注解（`NOT_FOUND = 400 [(errors.code) = 404]`），生成器抽成静态表——**服务代码里永远只写 reason 字符串，不写数字**。前端消费 reason 做 i18n 键、HTTP 401 触发刷新/重放，message 除登录页外不直接展示（[binding-spec §4](../binding-spec.md)）。

## 4. 一个请求的端到端追踪

以刚启动、种子已落、平台管理员已登录的部署为例，逐站走 `GET /admin/v1/dict/langs?page=1&page_size=10`（路由出自 `i_language.proto` 的 List 注解）。

1. **路由命中**：请求进 axum 路由树。生成器把 LanguageService 的 6 条路由全装进门控子树（语言没有进 `AUTH_FREE` 白名单——白名单 8 条全是登录/验证码/刷新/救援类，见 [第 5 章](./05-permission-model.md)）。每条路由的 handler 闭包携带 `RouteWire`（operation_id、输入/输出类型全名、路径变量名、body 模式）。
2. **绑定层（最外层）**：query 串按 §1 的叶子表装进 `PagingRequest`——`page` → `Some(1)`，`page_size` → `Some(10)`，其余字段缺席。畸形值（`page=abc`）在这里就吃 `400 CODEC` 信封，**先于 401**：绑定先于鉴权是 wire 契约。
3. **鉴权门**：`Authorization: Bearer …` 走 RS256 验签 + Redis 会话检查（[第 5 章](./05-permission-model.md)），claims 注入 `RequestContext`；租户闸门与 authz 评估随后跑（语言属 dict 模块，租户套餐要含它才放行——[第 6 章](./06-multi-tenant-isolation.md)）。
4. **glue 生命周期**：本路由无路径变量，跳过 vars 绑定；`DynamicMessage` 静态换乘成 `PagingRequest`；组装 `RequestContext`，调用 `LanguageService::list`。
5. **handler**：`LanguageRepo::new(&state.db).paged_list(&req)`——`fetch_paged` 先 `count`（谓词 `deleted_at IS NULL`，无租户谓词），再切片查询。种子 7 行 < 默认页宽 10，一次全出。
6. **映射与编码**：7 行 `Model` → `language_proto` → `ListLanguageResponse`；编码器按 protojson **EmitUnpopulated** 语义序列化（构建于契约 pool 之上，响应先过静态脱敏计划——语言消息没有脱敏注解，原样通过）。

响应体（种子库的真实形状；时间戳值为示意）：

```json
{
  "items": [
    {
      "id": "1",
      "languageCode": "zh-CN",
      "languageName": "简体中文",
      "nativeName": "简体中文",
      "isDefault": true,
      "isEnabled": true,
      "sortOrder": 0,
      "createdAt": "2026-01-01T08:00:00Z",
      "updatedAt": "2026-01-01T08:00:00Z"
    },
    "… 其余 6 行（zh-TW / en-US / ja-JP / ko-KR / es-ES / fr-FR）…"
  ],
  "total": "7"
}
```

三个值得记住的形状规则（golden 测试逐字节钉住，`apps/admin/tests/golden.rs`）：

- **presence 优先于 EmitUnpopulated**：`Language` 的每个字段都是 proto3 `optional`，未设置的 presence 字段**保持省略**——种子行没写 `created_by`，响应里就没有 `createdBy` 键（哪怕开了 EmitUnpopulated）；零值 `Language` 序列化就是 `{}`；
- **普通标量与 repeated 落默认**：`ListLanguageResponse.total` 是非 optional 的 `uint64`，空列表响应也要输出 `"items":[],"total":"0"`（repeated 空出 `[]`）；
- **64 位整数是字符串**：`id`、`total`、`created_by` 一律 `"7"` 形态；32 位标量（`sort_order`）保持数字。

手工冒烟同款命令（开发部署的 HTTP 端口来自 `server.yaml` 的 `bind: ":7788"`）：

```bash
curl -s "http://127.0.0.1:7788/admin/v1/dict/langs?page=1&page_size=10" \
  -H "Authorization: Bearer <access_token>"
```

## 5. 写 handler 测试：完整示范

### 5.1 测试装配面：testsupport

handler 测试不起 HTTP 服务、不连 Postgres/Redis。`apps/admin/src/admin/testsupport.rs` 提供最小 `AppState`（仅测试构建可见）：

- **db**：`sqlite::memory:`——每次连接是独立库，测试间天然隔离；schema 由 `SqliteTestMigrator` 建全（与生产同一实体清单，backend=Sqlite）；
- **redis**：绑定 `127.0.0.1:0` 的进程内 mini-redis——纯 DB service 从不触它，但 `AppState.redis` 必须有一个活连接才能构造；
- **jwt/authenticator**：占位引擎（测试面从不校验 token；登录链路测试要走通铸造时用 HS256 测试钥）；
- **CI 零服务依赖**：`cargo test --workspace` 不需要 postgres/redis 在场。

```rust
pub(crate) async fn test_state() -> Arc<AppState> { /* mem_db + schema + mini-redis + 占位认证 */ }

/// 带 sub/uid/tid 声明包的请求上下文（键名对齐 UserTokenPayload::from_claims；
/// tid = 0 即平台视角）。
pub(crate) fn ctx(tenant_id: i64, user_id: i64) -> RequestContext { ... }
```

`ctx(7, 1)` = "租户 7 的用户 1 在发请求"，claims 只有 `sub`/`uid`/`tid` 三键——够 `operator_of` 用。测试不经过鉴权门，直接调 handler 方法。

### 5.2 一个完整测试逐行读

`services/language.rs` 里的真实测试（节选主干，注释保留）：

```rust
/// 全局面 CRUD：create 不落租户（global 表）、按 code 寻路、
/// batch_create 批量进、update 改名。
#[tokio::test]
async fn global_crud_and_code_lookup() {
    let state = test_state().await;          // ① 内存库 + schema + mini-redis
    let svc = LanguageService { state: state.clone() };

    svc.create(
        ctx(7, 1),                            // ② 租户 7 的用户 1——全局表不在乎 tid
        CreateLanguageRequest { data: Some(data("zh-CN", "简体中文")) },
    ).await.unwrap();

    svc.batch_create(
        ctx(0, 1),                            // 平台视角再来两条
        BatchCreateLanguagesRequest { items: vec![data("en-US", "English"), data("ja-JP", "日本語")] },
    ).await.unwrap();

    // 全局表：任意租户视角都见全部行。
    let listed = svc.list(ctx(7, 1), PagingRequest::default()).await.unwrap();
    assert_eq!(listed.total, 3);              // ③ 处理器返回静态类型，直接断言字段
    let again = svc.list(ctx(9, 2), PagingRequest::default()).await.unwrap();
    assert_eq!(again.total, 3);               // 换个租户看，还是 3 行

    let got = svc.get(ctx(0, 1), GetLanguageRequest {
        query_by: Some(QueryBy::Code("ja-JP".into())), ..Default::default()
    }).await.unwrap();
    assert_eq!(got.language_name, Some("日本語".into()));
    let id = got.id.unwrap();                 // ④ u64，与 update 的 req.id 同型

    svc.update(ctx(0, 1), UpdateLanguageRequest {
        id,
        data: Some(Language { language_name: Some("日本語（改）".into()), ..Default::default() }),
        ..Default::default()
    }).await.unwrap();
    // …再 get 回读，断言 language_name == "日本語（改）" 且 updated_by == Some(1)
}
```

模板化解读：① `test_state()` 每个测试独享一套内存库；② `ctx(tid, uid)` 直接造上下文，测试是**单元级**的（不经过绑定/鉴权层——那些各有 binder 测试与守卫测试钉）；③ 断言打在静态 proto 类型上，无 JSON 解析；④ 错误路径测试用 `unwrap_err()` 拿 `StatusError` 对 `reason`/`message` 双断言（如 `get_error_mappings`：未知 code → `("NOT_FOUND", "language not found")`；缺 query_by → `("BAD_REQUEST", "query_by required")`）。

repo 层测试更矮一层，直接对内存库（`mem_db()` + `schema()`）建 repo 实例——`repos/language.rs` 里的 `paged_list_defaults_and_no_paging` 钉住"默认页宽 10 / total 12 / no_paging 折叠 total 为行数"三个分页不变量。

### 5.3 新 handler 的测试该写哪些

对照既有实物，一个新服务的测试面至少覆盖：一条 CRUD 主干（如 `global_crud_and_code_lookup`）、一条错误映射（如 `get_error_mappings`）、掩码门两条（见 §6 的 `mask_gates_unlisted_scalar_fields` / `masked_none_clears_nullable_column`）；租户表再加一条跨租户用例（[第 6 章](./06-multi-tenant-isolation.md)清单第 5 条）。

## 6. 写面的字段闸：update_mask 与 set_masked!

### 6.1 语义三态

契约的 Update 消息都带 `update_mask`（AIP-134 字段级更新语义；[binding-spec §2.5](../binding-spec.md) 是权威规格）。本仓落在 `apps/admin/src/admin/fieldmask.rs` + `set_masked!` 宏：

- **无掩码** → 全量提交语义：载荷里 Some 的都写、None 的不动列（存量行为一字不差）；
- **有掩码** → 白名单门：只有掩码选中的字段允许动；掩码选中 + 载荷 Some → 写值；掩码选中 + 载荷 None → **显式清列**（可空列的清值通道）；
- **present-but-empty 掩码**（`updateMask=` 空串）→ 什么都不许动（AIP-134 严格读法，不是"全放行"）。

路径归一：protojson 送 snake 原名、前端可能送 camel 表单键，`FieldMask` 内部去下划线小写对表——`hide_in_menu` 与 `hideInMenu` 命中同一列。

### 6.2 set_masked! 四形态

宏定义在 `apps/admin/src/admin/macros.rs`，四个形态按"列可空性 × 命名"组合：

```rust
set_masked!(req mask, a, language_name, data.language_name, |v: &String| v.clone());
// req：NOT NULL 列。掩码选中且载荷 Some → Set；载荷 None 永不写（无可清语义）。

set_masked!(opt mask, a, native_name, data.native_name, |v: &String| Some(v.clone()));
// opt：可空列。掩码选中：Some → Set(Some)；None → Set(None) 显式清列。
//      无掩码：None 不动列（全量提交语义不回退）。

// opt_named / req_named：列名与 wire 名不一致时显式给名（如 proto `type` → 列 type_column）。
```

language 的 `apply_fields` 用了全部五个字段位（`req` 一条 + `opt` 四条），`language_code` 不在列表——它是标准代码锚，**更新面不开放**：载荷里带 `languageCode` 也动不了这一列（白名单门只在列出的字段上放行）。

### 6.3 门的行为钉在测试里

`mask_gates_unlisted_scalar_fields`：掩码只选 `language_name`，载荷多带 `native_name: Some(" evil ")` 与 `is_enabled: Some(false)`——回读断言只有 `language_name` 变了，另两列原值不动。`masked_none_clears_nullable_column`：无掩码时载荷 None 不清列（全量提交不回退）；掩码选中 `native_name` 且载荷 None 时列被显式清空，且 **NOT NULL 列（`language_name`）永远不被掩码清掉**。

两形态对照表（写新 update 时照抄）：

| 场景 | 无掩码 | 掩码选中 | 掩码未选中 |
|---|---|---|---|
| `opt` 列 + 载荷 Some | 写 | 写 | 不动 |
| `opt` 列 + 载荷 None | 不动 | **清列** | 不动 |
| `req` 列 + 载荷 Some | 写 | 写 | 不动 |
| `req` 列 + 载荷 None | 不动 | 不动 | 不动 |

create 没有 update_mask 参数——create 是**无掩码即全量提交**的世界：载荷 Some 全写，缺省列走 `or(Some(默认))` 兜底（见 §3.2 create 的 `is_enabled.or(Some(true))`）。

## 7. 挂载与注册

服务与路由的对接只有两处手写：

1. `apps/admin/src/admin/services.rs`：`mod language;` + re-export `LanguageService`；
2. `apps/admin/src/admin/server/rest.rs` 的 `mount_service!` 表：`(mount_language_service, LanguageService)` 一行。

`mount_service!` 是 rest.rs 内部的统一装配宏：把服务实现包成 `Arc<dyn LanguageServiceHandlers>` 递给生成器的 `mount_language_service`，后者按生成时的 AUTH_FREE 分类表把每条路由放进公开子树（`router_pub`）或门控子树（`router_gate`）——语言的 6 条全部进门控子树。绑定层永远在最外，鉴权门只包门控路由；公开/门控的双向守卫在 `apps/admin/tests/whitelist.rs`（白名单多一条、少一条都红）。

## 8. 数据接线：权限点、菜单、种子

模块要在管理界面可达，还差三行数据（种子或管理页操作）：

| 数据 | 表 | 作用 |
|---|---|---|
| 接口/权限点 | `sys_apis`、`sys_permissions` + 绑定 | (path 模板, method) 注册进 Api 表——租户闸门按它判定模块白名单（[第 6 章](./06-multi-tenant-isolation.md)，fail-closed：漏注册租户侧一律 403）；角色经权限点获得调用权（[第 5 章](./05-permission-model.md)） |
| 菜单 | `sys_menus` | 目录/菜单/按钮三类节点；页面路由、组件路径、按钮权限标识 |
| 业务种子 | 各表 | 初始数据走 `apps/admin/src/admin/seed/` 模块 |

种子代码的形状（`seed/mod.rs` 的 `seed_languages`）：

```rust
async fn seed_languages(state: &Arc<AppState>) -> Result<(), String> {
    if langs::Entity::find().count(&state.db).await.unwrap_or(1) > 0 {
        return Ok(());                        // 幂等：已有数据即跳过
    }
    let rows = [
        ("zh-CN", "简体中文", "简体中文", true, 0u32),
        ("zh-TW", "繁體中文", "繁體中文", false, 0),
        ("en-US", "English", "English", false, 1),
        /* ja-JP / ko-KR / es-ES / fr-FR … */
    ];
    for (code, name, native, is_default, sort) in rows { insert_seed!(...) }
    Ok(())
}
```

七种语言全部幂等播种（启动种子测试断言 `sys_languages` 恰好 7 行且重跑种子不翻倍）。

前端页面由菜单表驱动：快照前端的页面注册表按菜单里的组件路径渲染，所以"新模块前端页面" = 菜单行 + 前端快照里已有的页面模式，无需另写 API 层。

## 9. 验证

```bash
cd backend
cargo clippy --workspace -- -D warnings && cargo test --workspace
# 行为对齐类改动（绑定/序列化/错误信封）加跑差分台架（第 3 章）
```

单测一个模块时可以缩窄：`cargo test -p admin language`（跑 language 相关的全部单测）。CI 不需要 postgres/redis 在场（内存库 + mini-redis，见 §5.1）。

手工冒烟链路：起服务 → Swagger UI 里找到新服务的路由 → 平台管理员登录拿令牌 → 走 create → list → update → delete。租户侧再用一个租户账号重复一遍，确认白名单与谓词行为符合预期。

## 10. 本章小结

- 每个新模块 = 实体 + 仓储 + 服务 trait 实现 + 两行注册 + 三行数据，骨架完全模式化；
- `repo_shell!` 一行展开 struct + 软删谓词 + 两个列表信封（+ 名词臂的两个 id 寻址方法）；分页请求原样透传 `fetch_paged`；
- 映射、错误、分页、租户谓词、审计全部有统一入口，不要在服务里自建平行机制；
- 响应形状三规则（presence 省略、repeated 落 `[]`、64 位字符串化）由 golden 测试钉死，别在前端猜；
- update_mask 白名单门是写面标配：`set_masked!` 四形态 + 无掩码全量提交语义；
- 漏挂载/漏权限点不会编译报错，靠守卫测试、台架与冒烟抓。

下一步：[05 · 权限模型](./05-permission-model.md)。
