# 04 · 第一个服务模块

> 前置：[03 契约与生成链路](./03-codegen-chain.md)。读完你将亲手跑通一遍完整开发闭环：契约落树 → 实现 → 挂载 → 验证。
>
> 本仓的契约统一在上游契约源维护、经同步脚本进仓，所以"新增模块"的起点是**契约树里出现了新服务**（而不是本仓写 proto）。本章以契约树里真实存在的 `LanguageService`（语言管理，最小的完整模块之一）为样本，走一遍 Rust 侧的实现闭环——今后每个新模块都是同一套动作。

## 1. 契约落树，构建出骨架

同步后 `backend/api/protos/<模块>/` 里出现该服务的 proto；`cargo build` 时生成链自动产出（见[第 3 章](./03-codegen-chain.md)）：

- 类型：`proto::proto::dict::service::v1::{Language, ListLanguageResponse, ...}`
- trait：`proto::gen::services::LanguageServiceHandlers`（556 个方法之一）
- 挂载：`proto::gen::mounts::mount_language_service`

此刻服务还不可用：trait 没有实现、挂载没有被调用。漏实现/漏挂载会在守卫测试与台架 sweep 里现形。

## 2. 数据层：实体 + 仓储

**实体**——每张表一个 SeaORM 实体模块，按域分组在 `src/data/`（identity / rbac / audit / misc），形如 `src/data/rbac.rs` 里的 `sys_roles`：

```rust
#[derive(Clone, Debug, PartialEq, DeriveEntityModel)]
#[sea_orm(table_name = "sys_languages")]
pub struct Model {
    #[sea_orm(primary_key)]
    pub id: u32,
    pub language_code: String,
    pub language_name: String,
    pub native_name: Option<String>,
    pub is_default: Option<bool>,
    pub is_enabled: Option<bool>,
    pub sort_order: Option<u32>,
    pub tenant_id: Option<u32>,        // 租户表必备；全局表没有
    pub deleted_at: Option<chrono::NaiveDateTime>,   // 软删列
    // ...
}
```

新表要同时注册进 `src/migration.rs` 的 `EntityTables` 链（启动迁移按它建表）。

**仓储**——一仓一文件（`src/data/repos/language.rs`），列表分页统一走 `paging::fetch_paged`（count 先行、错误传播），查询谓词由 [`Viewer`](./06-multi-tenant-isolation.md) 派生。仓储只做数据访问，不做业务判断。

## 3. 服务层：实现生成的 trait

`src/services/language.rs`（节选，这就是全部骨架）：

```rust
pub struct LanguageService {
    pub state: Arc<AppState>,
}

#[async_trait::async_trait]
impl proto::gen::services::LanguageServiceHandlers for LanguageService {
    async fn list(
        &self,
        _ctx: rushwind_http_binding::ctx::RequestContext,
        req: PagingRequest,
    ) -> Result<ListLanguageResponse, StatusError> {
        let repo = crate::data::repos::LanguageRepo::new(&self.state.db);
        let (rows, total) = repo.paged_list(&req).await?;
        Ok(ListLanguageResponse {
            items: rows.into_iter().map(language_proto).collect(),
            total,
        })
    }
}
```

要点：

- **服务是薄层**：从 `RequestContext` 取操作者与租户、调仓储、做映射。一个服务一个 struct，只有 `state: Arc<AppState>` 一个字段；
- **entity → proto 映射是显式函数**（`language_proto`）：逐字段转换，时间戳 `Option<NaiveDateTime> → prost_types::Timestamp`。漏字段不报错，是静默零值——新增字段时两个类型都要动；
- **错误用统一信封**：仓储错误经 `state.rs` 的三个助手归一——`db_err`（数据库错误 → 500 INTERNAL）、`not_found`（查无 → 404）、`status_error`（手工构造任意 reason）；返回类型统一是 `Result<_, StatusError>`，信封形状由编码层落成四字段。

写操作（create/update/delete）多两件事：操作者戳（`created_by` / `updated_by`）取自上下文；软删走 `deleted_at` 置位，不物理删。

## 4. 挂载与注册

服务与路由的对接只有两处手写：

1. `src/services.rs`：`mod language;` + re-export `LanguageService`；
2. `src/server/rest.rs` 的 `mount_services!` 表：`(mount_language_service, LanguageService)` 一行。

挂载宏按生成器的 AUTH_FREE 分类表把每条路由放进公开子树或门控子树；绑定层永远在最外，鉴权门只包门控路由。

## 5. 数据接线：权限点、菜单、种子

模块要在管理界面可达，还差三行数据（种子或管理页操作）：

| 数据 | 表 | 作用 |
|---|---|---|
| 接口/权限点 | `sys_apis`、`sys_permissions` + 绑定 | (path 模板, method) 注册进 Api 表——租户闸门按它判定模块白名单（[第 6 章](./06-multi-tenant-isolation.md)）；角色经权限点获得调用权（[第 5 章](./05-permission-model.md)） |
| 菜单 | `sys_menus` | 目录/菜单/按钮三类节点；页面路由、组件路径、按钮权限标识 |
| 业务种子 | 各表 | 初始数据（如默认语言）走 `src/seed/` 模块 |

前端页面由菜单表驱动：快照前端的页面注册表按菜单里的组件路径渲染，所以"新模块前端页面" = 菜单行 + 前端快照里已有的页面模式，无需另写 API 层。

## 6. 验证

```bash
cd backend
cargo clippy --workspace -- -D warnings && cargo test --workspace
# 行为对齐类改动（绑定/序列化/错误信封）加跑差分台架（第 3 章）
```

手工冒烟链路：起服务 → Swagger UI 里找到新服务的路由 → 平台管理员登录拿令牌 → 走 create → list → update → delete。租户侧再用一个租户账号重复一遍，确认白名单与谓词行为符合预期。

## 7. 本章小结

- 每个新模块 = 实体 + 仓储 + 服务 trait 实现 + 两行注册 + 三行数据，骨架完全模式化；
- 映射、错误、分页、租户谓词、审计全部有统一入口，不要在服务里自建平行机制；
- 漏挂载/漏权限点不会编译报错，靠守卫测试、台架与冒烟抓。

下一步：[05 · 权限模型](./05-permission-model.md)。
