# 09 写下游：加你的第一个业务模块

前八章讲的是"读这个基座"——从架构到部署。本章讲**写下游**：fork 本仓之后，把你的业务功能（ERP、商城、MES……）长在应用内部专属的 `src/modules/` 里，同进程骑在 admin 的鉴权门、审计层与装配生命周期上，同时与上游保持零冲突的可合并关系。

前置：[03 契约与生成链路](./03-codegen-chain.md)（生成物边界）、[04 第一个服务模块](./04-first-service-module.md)（服务实现闭环）、[06 多租户与行级隔离](./06-multi-tenant-isolation.md)（Viewer 三态）。

你将得到：一个能跑通"契约 → 服务 → 种子 → 装配 → 放行"全链路的自有业务模块。本章给出 copy-rename 的**命令级操作手册**：改哪些文件、每一处必改的字符串、守卫测试怎么跑、上游跟进时模块侧要做什么。

## fork 工作流：上游怎么跟

后端只有一个应用（`backend/apps/admin`），你的代码和上游的代码同仓不同室——所有权一条线（见 [CONTRIBUTING](../../CONTRIBUTING.md)）：

- **上游同步面**：`src/admin/`（59 个挂载服务面）+ `api/protos/`（契约快照，MANIFEST 门）——fork 只读；
- **fork 自有面**：`src/modules/`（业务模块）+ `api/fork/protos/`（模块契约树）+ `main.rs` 的装配行——上行永不触碰。

fork 的 git 形态是"双远端 + 合并"：

```bash
git clone https://github.com/<你>/rushwind-admin.git
cd rushwind-admin
git remote add upstream https://github.com/tx7do/rushwind-admin.git
```

跟进上游新版本：

```bash
git fetch upstream
git merge upstream/main      # 两个房间互不重叠，理论零冲突
cd backend && cargo check    # 编译错误就是升级清单：稳定面变了什么，这里全暴露
```

merge 冲突面收敛到 rushwind git 依赖的统一 rev 钉版——workspace（`backend/Cargo.toml`）与 app（`apps/admin/Cargo.toml`）两处 Cargo.toml 的多行依赖写同一个 rev 值（当前钉 `08fbfcd`），上游出新版时两处同升。

## 模块长什么样：template 解剖

`src/modules/template/`（代码侧）与 `api/fork/protos/notice/`（契约侧）是仓内预置的完整骨架，**两棵目录树一起 copy-rename** 即开工——没有 Cargo.toml 要改、没有 workspace 成员要登记、没有构建脚本要抄（构建由应用的 build.rs 统一驱动，你的契约树落位即被编译）：

| 位置 | 作用 | 对应教程章 |
|---|---|---|
| `api/fork/protos/<模块树>/` | 模块的 proto 契约。与 admin 树同一个 buf 工作区：一次编译、一个池、**一套分页类型**（模块的 `PagingRequest` 就是 admin 的那个，没有转码） | 03 |
| `src/modules/<module>/data.rs` | 业务实体（`<module>_` 前缀表）+ `crate::repo_shell!` 派生的租户 repo | 06 |
| `src/modules/<module>/services.rs` | Handlers trait 实现，身份/错误助手来自 `crate::admin::state` | 04 |
| `src/modules/<module>/seed.rs` | 幂等种子：菜单、权限、sys_apis 路由登记（自然键，不占固定 id） | 05 |
| `src/modules/<module>/migration.rs` | 模块自己的迁移链，与 admin 链互不干扰 | 08 |
| `src/modules/<module>/mod.rs` 的 `pack()` | 模块的 route pack：骑上 admin 边缘栈的组合子；审计面直接从本面生成的 ROUTES 表派生 | 本章 |

关键约定只有一条要背下来：**业务的一切都是纯增**——纯增目录、纯增表、纯增迁移 vintage、纯增种子；不往 admin 的 `sys_*` 表加列、不往 `rest.rs` 挂载清单塞服务、不手改生成产物。

### 契约侧三文件的分工（i_ 随域走）

`api/fork/protos/notice/service/v1/` 下三个 proto，分工是模块树的铁律：

| 文件 | 角色 |
|---|---|
| `notice.proto` | 领域面：实体 message + 请求/答复形态，**不带路由注解**。字段号约定与上游契约树一致：1–99 业务位、100+ 审计人（created_by/updated_by/deleted_by）、200+ 时间戳 |
| `i_notice.proto` | **BFF 面：带 google.api.http 注解、真正生成路由的那一个**。路由前缀是模块独立前缀 `/template/v1/…`，全部路由默认 gated（AUTH_FREE 白名单为空表） |
| `notice_error.proto` | Kratos 风格错误枚举：`(errors.code)` 把 reason 钉到 HTTP 状态，生成器发射进 `error_tables::ERROR_STATUS`，模块的 `status_error` 助手按 (package, reason) 查表回状态 |

这就是 **"i_ 面随域走"** 的形状：admin 契约的 59 个 `i_*` 投影面住上游镜像区、随同步波演进（见 [架构决策](../architecture-decisions.md) §2）；而**模块树的 i_* 投影面住在自己的域树里**、与域 message 同目录同 package，fork 全权、随模块一起 copy-rename——域搬到哪里，它的 BFF 面就到哪里，不存在"域在 fork、投影在上游"的劈叉。

## copy-rename 操作手册（命令级）

假设你的新模块叫 **`mall`**（业务域 `mall`、模块值 `MALL`、路由前缀 `/mall/v1/…`、实体 `goods`）。以下每一步都对应仓里真实存在的东西。

### 第 1 步：两棵树改名（git mv 保历史）

```bash
cd backend/apps/admin

# 代码侧
git mv src/modules/template src/modules/mall

# 契约侧（目录连 service/v1 层级一起搬）
git mv api/fork/protos/notice api/fork/protos/mall
git mv api/fork/protos/mall/service/v1/notice.proto      api/fork/protos/mall/service/v1/mall.proto
git mv api/fork/protos/mall/service/v1/i_notice.proto    api/fork/protos/mall/service/v1/i_mall.proto
git mv api/fork/protos/mall/service/v1/notice_error.proto api/fork/protos/mall/service/v1/mall_error.proto
```

proto 的改名必须**同动文件名、目录、import 路径**三件——`i_mall.proto` 里的 `import "notice/service/v1/notice.proto";` 要改成 `import "mall/service/v1/mall.proto";`，否则 buf 编译直接红。

### 第 2 步：契约侧逐处必改字符串

| 文件 | 必改 | 改成 |
|---|---|---|
| `mall.proto` | `package notice.service.v1` | `package mall.service.v1` |
| `mall.proto` | message 名（`Notice`/`ListNoticeResponse`/`CreateNoticeRequest`…） | 你的实体名（`Goods`…） |
| `i_mall.proto` | `package` + `import "mall/…"` + `service NoticeService` | `service MallService`（trait/mount 名全部随之） |
| `i_mall.proto` | 每条路由的 `"/template/v1/notices…"` | `"/mall/v1/goods…"`（模块独立前缀，与 admin 的 `/admin/v1` 平行） |
| `i_mall.proto` | rpc 的请求/答复引用（`notice.service.v1.X`） | `mall.service.v1.X` |
| `mall_error.proto` | `enum NoticeErrorReason` | `enum MallErrorReason`（reason 值 BAD_REQUEST/NOT_FOUND… 可复用） |

### 第 3 步：代码侧逐文件必改字符串

`src/modules/mall/mod.rs`：

```rust
// 审计面：路由行从本面生成的 ROUTES 表派生——改 crate::proto::gen::template → gen::mall
let rows: Vec<(&'static str, &'static str, &'static str)> =
    crate::proto::gen::mall::routes::ROUTES.iter()…

// 错误表查询的 package 字面量——与 mall_error.proto 的 package 逐字一致
.find(|(p, r, _)| *p == "mall.service.v1" && *r == reason)

// mount 调用——服务名随 proto 改
crate::proto::gen::mall::mounts::mount_mall_service(…)

// 服务实例路径
Arc::new(crate::modules::mall::services::MallService { … })
```

`data.rs`：表名 `biz_notices` → `biz_goods`（业务表一律 `biz_` 前缀，与 admin 的 `sys_` 永不撞）；`crate::repo_shell!(tenant NoticeRepo, notices)` → `repo_shell!(tenant GoodsRepo, goods)`；实体模块与字段按你的领域改。

`services.rs`：trait 名 `NoticeServiceHandlers` → `MallServiceHandlers`（来自 `crate::proto::gen::mall::services`）；proto 类型路径 `crate::proto::notice::service::v1::*` → `crate::proto::mall::service::v1::*`；`crate::modules::template::data` → `crate::modules::mall::data`。**身份助手一行不用动**——`operator_of`/`tenant_of`/`Viewer::from_ctx` 来自 `crate::admin::state`，它们是模块骑的稳定面。

`seed.rs`：`BUSINESS_MODULE: &str = "TEMPLATE"` → `"MALL"`（`sys_apis.business_module` 与套餐白名单都读这个值）；权限码 `template:manage_notice` → `mall:manage_goods`；绑定角色 `["platform:admin", "template:tenant:manager"]` → 你的角色码（第二个是 admin 种子的**租户管理员模板角色**：新租户开台时它被复制为各租户的 `tenant:manager`——绑定它等于把权限发到每个租户的管理员手里，角色码按你的域改）；菜单 `path: "/notice"` → `"/mall"`、组件 `notice/notices/index` → 你的前端视图路径；路由登记循环里的 `crate::proto::gen::template::routes::ROUTES` → `gen::mall`。

`migration.rs`：两处**必须**改——迁移名 `"m20251110_000001_module_template_init"` → `"m<日期>_<序号>_module_mall_init"`（**保持 `module_` 命名空间前缀**，与 admin 链的版本号零交集）；账本表 `"seaql_migrations_module_template"` → `"seaql_migrations_module_mall"`（每条链记自己的账本，这是真启动事故换来的回归钉）。`EntityTables` 的表引用随 data.rs 改。

### 第 4 步：build.rs 加一个 face + proto/mod.rs 加一块 include

build.rs 的 faces 数组追加一项（`root_prefix` 按**契约树目录**切片，防止你的树漏进别的 face 的路由表）：

```rust
biz_proto_build::Face {
    label: "mall",                          // face 标签
    module_file: "mall_gen.rs",             // 产物文件名（与 include 一致）
    root_prefix: Some("mall/"),             // 按 api/fork/protos/ 下的树前缀切片
    source_dir: None,                       // fork face 不取镜像区
    module_path: Some("crate::proto::gen::mall::"),
},
```

`src/proto/mod.rs` 的 `gen` 模块加一块兄弟 include（文件里的注释原话："一个 copy-renamed module adds its own sibling block here beside the build.rs face entry"）：

```rust
pub mod gen {
    include!(concat!(env!("OUT_DIR"), "/admin_gen.rs"));

    pub mod template {
        include!(concat!(env!("OUT_DIR"), "/template_gen.rs"));
    }

    // ↓ 新增这一块
    pub mod mall {
        include!(concat!(env!("OUT_DIR"), "/mall_gen.rs"));
    }
}
```

label/module_file/module_path/include 路径四处必须对齐同一个名字，手滑任何一处都是 `OUT_DIR` 文件不存在的编译错。

### 第 5 步：main.rs 四行 + server.yaml 一条

应用入口 `apps/admin/src/main.rs` 就是 fork 的装配点（上游同步永远不会重写这个文件的业务区）。每加一个业务模块，边际成本是这里加四行 + `server.yaml` 的 route_packs 一条。真实代码（template 现挂两 pack 的原样摘录，`admin::modules::template` 换成你的模块）：

```rust
use admin::config::Config;
use admin::modules::template;                     // ① use 声明（mod 树在 lib.rs 的 pub mod modules 下）
…
if state.cfg.database_migrate {
    admin::migration::run(&state.db).await?;
    template::migration::run(&state.db).await?;   // ② 迁移：admin 链之后接模块链
}
…
admin::seed::run(&state).await?;
template::seed::run(&state).await?;               // ③ 种子：admin 种子先跑（模块种子要解析它的角色）

let mut assembler = Bootstrap::from_yaml_str(SERVER_YAML)?
    .route_pack("admin-surface", rest::pack(Arc::clone(&state)))
    .route_pack("business-surface", template::pack(Arc::clone(&state)));  // ④ 装配
```

对应的 `server.yaml` route_packs 已有一条 `business-surface`（多个业务模块共用这一个 pack 名时在 pack 内并排挂载，或每模块一条 pack——template 的现状是**一条 pack 挂一个模块**）。四个位置在 main.rs 里都有注释锚——文件头直接写着"每加一个业务模块是四行：迁移链、种子、route pack，外加 `mod` 声明"，迁移块与装配行旁各有一句"新模块插这里"。

两个 pack 同进程、同 CORS、同请求预算、同一个鉴权门；各自的审计面（`AuditFace`）把各自契约的操作写进同一组审计表。

### 第 6 步：编译与守卫测试

```bash
cd backend
cargo check                      # 先编译干净
cargo test --workspace           # 再跑守卫面（见下节清单）
```

## 改完模块跑哪些测试：模板守卫面

仓里钉着 template 的三层守卫，copy-rename 时**把两个测试文件一起改名**（`tests/template_gen_surface.rs` → `tests/mall_gen_surface.rs` 等），把引用从 `gen::template` 改到 `gen::mall`——它们就是你的模块守卫：

**gen-surface 四断言**（`tests/template_gen_surface.rs`）：

1. `route_table_is_the_five_notice_routes`——路由表行数钉死（当前 5 条），每条都必须以 `/template/v1/` 开头、无 shadowed 行。**给你的模块加 rpc 时先改这里的计数**；
2. `auth_free_carries_no_module_routes_and_routes_all_gated`——模块路由一个都不许进免鉴权白名单（`notice.service.v1` 前缀检索 AUTH_FREE）；
3. `error_table_resolves_the_module_reasons`——错误表按 (package, reason) 解析出 400/404/409/500，未知 reason 回 None（调用方折成 500）；
4. `pool_carries_the_notice_service`——描述符池同时携带模块服务与 admin 服务（单池不被模块树挤掉）。

**tenancy 三断言**（`tests/template_tenancy_guards.rs`，源文本守卫——防的是"谓词在重写中被静默丢掉"这种单测抓不到的形状）：

1. `delete_resolves_through_the_tenanted_repo`——delete 必须经 `repo.get(`（带 Viewer 谓词）+ `not_found`，跨租户 id 答 404 而不是删掉别人的行；
2. `repo_get_carries_the_viewer_predicate`——repo 的 get 必须 `.filter(self.condition())`；
3. `notice_repo_is_the_tenant_shell`——`repo_shell!(tenant …)` 臂是编译期钉死（`global` 臂没有谓词，会静默全域化每个 handler）。

**迁移链 ledger 测试**（`src/modules/template/migration.rs` 内）：`module_chain_keeps_its_own_ledger_table`——账本表名必须是模块自己的（`seaql_migrations_module_template` 形），且链内迁移名一律带模块命名空间前缀。

再加全局守卫一起跑：`cargo test --workspace` 覆盖 `whitelist`（免鉴权白名单与 shadowed 集）、`golden`（脱敏金样）、`admin_error_statuses`（gate 错误码与 admin 错误表对齐）——你的模块树若有 namespace 拼写错，最先炸的就是这几个。

## 一条请求的旅程（业务路由）

`POST /template/v1/notices` 从进来到落地：

1. assembler 的 edge 块（CORS + 超时预算）——与 admin 路由完全相同的一层；
2. business-surface pack 的 bind 层：按**同一个注解描述符池**做 protojson/form 绑定（400 先于 401，与 admin 同序）；
3. gated 路由进鉴权门：JWT 验签 → Redis 会话 → 租户闸（`sys_apis` 里查这条路由的 `business_module`，fail-closed）→ 套餐模块白名单 → authz 评估；
4. `MallService::create`：`operator_of` 取身份 → 写 `biz_goods`（tenant 谓词在 repo 层）；
5. 最外层审计层按模块的审计面（从本面生成的 ROUTES 表派生）解析出 operation，写 api/operation 审计表。

与 admin 路由唯一的差别在第 2、3、5 步的**数据来源**：路由登记、审计行都是模块自己的契约产物——栈本身逐字节相同，连描述符池都是同一个。

## 放行给租户（运营两步）

1. **路由登记（自动）**：模块种子每次启动把路由表按 (path, method) upsert 进 `sys_apis`，`business_module = MALL`（`seed.rs::sync_apis`——跳过 shadowed 行、stale 的 business_module 下次启动自愈）。租户闸对未登记路由 fail-closed，这一步就是登记。
2. **套餐白名单（手动）**：管理台 → 套餐管理，给目标套餐勾上 `MALL`。平台用户（tenant 0）不受此限；租户用户未勾时 403。这是**刻意不种子化**的一步：哪个套餐能开哪个域是业务决定，不是代码决定。

## 跟进上游的完整 SOP

### 常规波次

1. `git fetch upstream && git merge upstream/main`；
2. 改 `apps/admin/Cargo.toml` 的 rushwind rev 到上游新钉的版本（如 merge 已带入则核对）——workspace 与 app 两处同升；
3. `cargo check`——编译错误即升级清单；所有权线里的稳定入口（`crate::admin::{state, audit, data}`、四个宏实名：`crate::repo_shell!`（tenant/global 两种仓壳臂，含 noun 臂的 get_by_id/delete_by_id）、`crate::query_by_id!`、`crate::stamp_update!`、`crate::set_masked!`（opt/req/opt_named/req_named 四形态，update_mask 白名单门下的字段写面）、边缘组合子签名）之外的破坏性变更，按编译器指引迁移；
4. `cargo test --workspace`——应用的守卫测试（路由表、免鉴权白名单、线契约金样、模块守卫）会直接告诉你哪条契约变了；
5. 契约基线若有变：`bash backend/apps/admin/api/sync-protos.sh --check` 应保持绿（你没碰冻结区）；
6. 起库跑应用，回归登录 + 一个 admin 面 + 一个业务面探针。

### 同步波落地时，模块侧要做什么（2026-11 修复轮的实践对照）

上游的修复列车以"port of <上游提交>"的形式 cherry-pick 落地——2026-11-19 那轮（update_mask 写面、租户回收站、渐进验证码、DB 测试面）在同布局上零语义改动落位，**模块侧的适配动作就三类**，全部有真实先例：

- **路径归一化**：上游面搬家/合层后，把模块代码里指向上游的 `use` 路径改到新址（先例：分支时代的 `crate::proto::proto::` 双段路径在单应用合层后塌缩为 `crate::proto::`，31 个文件的 use 重新指址）。模块引用上游只经 `crate::admin::{…}` 与根 re-export，面窄意味着要改的地方少；
- **根转发对齐**：上游若新增/挪动 crate 根 re-export（`data/state/crypto/fieldmask/testsupport` 这批），模块侧的 `crate::xxx` 引用跟着核一遍——这些转发存在的原因就是"（同步的）handler 与测试体继续按 `crate::xxx` 原名解析"，模块同享这条稳定线；
- **宏形态跟进**：四个实名宏的臂与形态变了就要迁移。最近的实例是 `set_masked!`——上游把字段级 update_mask 写面落地为 opt/req/opt_named/req_named 四形态（masked Some 写、masked None 显式清、未 mask 保持整提交语义）。你若在模块里写了 update_mask 风格的 update handler，跟进这套四形态；`repo_shell!` 的两臂（tenant 与 global）在回收站轮里长出了 `deleted_at IS NULL` 谓词——模块的 repo 同臂自动获得，**这类臂内语义升级你一行不用改**，但要知道行为变了（软删行从此不进读面）。

判断"这次同步要不要动模块"的最小动作就是第 3 步的 `cargo check`：稳定面没动，模块零改动直接绿。

## 已知限制（诚实清单）

- 菜单的 `module` 列在管理台 UI 侧是封闭枚举：业务模块值（如 `MALL`）在菜单筛选器显示为 UNSPECIFIED，运行时字符串开放，只影响展示；
- 业务 pack 不带 docs 面（Swagger/Redoc 挂在 admin-surface 的设置节点下）；要给自己的契约出文档，需自嵌 OpenAPI 资产并避开 `/q/*` 路径；
- `rushwind-gen-http` 对路由是 first-match 语义：模块内新增 rpc 时跑一遍守卫测试（`shadowed` 断言）确认无遮蔽——种子路由登记同样跳过 shadowed 行，被遮蔽的 rpc 不会进 `sys_apis`。

## 深读

- copy-rename 清单：`apps/admin/src/modules/template/mod.rs` 的模块文档头
- 边缘栈组合子（bind/门/审计的层序契约）：`apps/admin/src/admin/server/edge.rs` 模块文档
- 生成引擎的管线细节（单工作区多 face）：[03 契约与生成链路](./03-codegen-chain.md)
- 架构的定稿依据与演进路线（为什么单应用、业务域按需打包的两级路线、BFF 投影层规则）：[架构决策与演进路线](../architecture-decisions.md)
- 域长出进程边界之后（多服务的触发信号、rushwind-cms 参考形态、迁移映射）：[服务演进路线](../services-evolution.md)
