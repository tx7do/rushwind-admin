# 09 写下游：加你的第一个业务模块

前八章讲的是"读这个基座"——从架构到部署。本章讲**写下游**：fork 本仓之后，把你的业务功能（ERP、商城、MES……）作为独立模块长在 `backend/business/` 下，同进程骑在 admin 的鉴权门、审计层与装配生命周期上，同时与上游保持零冲突的可合并关系。

前置：[03 契约与生成链路](./03-codegen-chain.md)（生成物边界）、[04 第一个服务模块](./04-first-service-module.md)（服务实现闭环）、[06 多租户与行级隔离](./06-multi-tenant-isolation.md)（Viewer 三态）。

你将得到：一个能跑通"契约 → 服务 → 种子 → 装配 → 放行"全链路的自有业务模块。

## fork 工作流：上游怎么跟

fork 的 git 形态是"双远端 + 合并"，不是"子目录冻结"：

```bash
git clone https://github.com/<你>/rushwind-admin.git
cd rushwind-admin
git remote add upstream https://github.com/tx7do/rushwind-admin.git
```

跟进上游新版本：

```bash
git fetch upstream
git merge upstream/main      # 业务代码全在纯增目录里，理论零冲突
cd backend && cargo check    # 编译错误就是升级清单：上游 lib 面变了什么，这里全暴露
```

你的所有业务改动都住在**纯增位置**：`backend/business/<module>/`、`backend/apps/platform/`、以及自己的迁移/种子——上游发版不会碰这些目录，merge 冲突面收敛到 `backend/Cargo.toml` 的 rushwind rev 一行。

## 模块长什么样：module-template 解剖

`backend/business/module-template/` 是仓内预置的完整骨架，copy-rename 即开工。目录逐项的含义（改名清单见模块自带的 [README](../../backend/business/module-template/README.md)）：

| 位置 | 作用 | 对应教程章 |
|---|---|---|
| `api/` | 模块自有的 buf 工作区 + proto 契约树。**不是**主仓的 `backend/api/`（那是上游同步冻结区） | 03 |
| `build.rs` | 五行配置表调 `biz-proto-build`：buf 编译注解闭包 → prost/pbjson 类型 → 生成路由/trait/mount 面 | 03 |
| `src/data.rs` | 业务实体（`<module>_` 前缀表）+ `admin_api::repo_shell!` 派生的租户 repo | 06 |
| `src/services.rs` | Handlers trait 实现，身份/错误助手来自 `admin_api::state` | 04 |
| `src/seed.rs` | 幂等种子：菜单、权限、sys_apis 路由登记（自然键，不占固定 id） | 05 |
| `src/migration.rs` | 模块自己的迁移链，与 admin 链互不干扰 | 08 |
| `src/lib.rs` 的 `pack()` | 模块的 route pack：骑上 admin 边缘栈的组合子 | 本章 |

关键约定只有一条要背下来：**业务的一切都是纯增**——纯增目录、纯增表、纯增迁移 vintage、纯增种子；不往 admin 的 `sys_*` 表加列、不往 `rest.rs` 挂载清单塞服务、不手改生成产物。

## 装配：platform，fork 自有的入口

`backend/apps/platform/` 是 fork 从第一天就拥有的装配 binary（上游永远不会读这个目录）。它做的事 = admin binary 的全套 + 两处插入：

```rust
// 迁移：admin 链之后接模块链
if state.cfg.database_migrate {
    admin_api::migration::run(&state.db).await?;
    module_template::migration::run(&state.db).await?;
}
// 种子：admin 种子先跑（模块种子要解析它的角色），模块种子随后
admin_api::seed::run(&state).await?;
module_template::seed::run(&state).await?;

// 装配：同一个 http server 挂两个 route pack
let mut assembler = Bootstrap::from_yaml_str(SERVER_YAML)?
    .route_pack("admin-surface", rest::pack(Arc::clone(&state)))
    .route_pack("business-surface", module_template::pack(Arc::clone(&state)));
```

两个 pack 同进程、同 CORS、同请求预算、同一个鉴权门；各自的审计面（`AuditFace`）把各自契约的操作写进同一组审计表。每加一个业务模块：一行 `migration::run`、一行 `seed::run`、一行 `route_pack`、`server.yaml` 的 `route_packs` 一条——加模块的边际成本是四行。

## 一条请求的旅程（业务路由）

`POST /template/v1/notices` 从进来到落地：

1. assembler 的 edge 块（CORS + 超时预算）——与 admin 路由完全相同的一层；
2. business-surface pack 的 bind 层：按模块自己的注解描述符池做 protojson/form 绑定（400 先于 401，与 admin 同序）；
3. gated 路由进鉴权门：JWT 验签 → Redis 会话 → 租户闸（`sys_apis` 里查这条路由的 `business_module`，fail-closed）→ 套餐模块白名单 → authz 评估；
4. `NoticeService::create`：`operator_of` 取身份 → 写 `biz_notices`（tenant 谓词在 repo 层）；
5. 最外层审计层按模块的 `AUDIT_ROWS` 解析出 operation，写 api/operation 审计表。

与 admin 路由唯一的差别在第 2、3、5 步的**数据来源**：描述符池、路由登记、审计行都是模块自己的契约产物——栈本身逐字节相同。

## 放行给租户（运营两步）

1. **路由登记（自动）**：模块种子每次启动把路由表按 (path, method) upsert 进 `sys_apis`，`business_module = TEMPLATE`。租户闸对未登记路由 fail-closed，这一步就是登记。
2. **套餐白名单（手动）**：管理台 → 套餐管理，给目标套餐勾上 `TEMPLATE`。平台用户（tenant 0）不受此限；租户用户未勾时 403。

## 跟进上游的完整 SOP

1. `git fetch upstream && git merge upstream/main`；
2. 改 `backend/Cargo.toml` 与各 crate 的 rushwind rev 到上游新钉的版本（如 merge 已带入则核对）；
3. `cargo check`——编译错误即升级清单；`admin_api` 的稳定门面（见 [CONTRIBUTING](../../CONTRIBUTING.md) 的装配面宣言）之外的破坏性变更，按编译器指引迁移；
4. `cargo test --workspace` + 模块自己的守卫测试；
5. 契约基线若有变：`bash backend/api/sync-protos.sh --check` 应保持绿（你没碰冻结区）；
6. 起库跑 platform，回归登录 + 一个 admin 面 + 一个业务面探针。

## 已知限制（诚实清单）

- 菜单的 `module` 列在管理台 UI 侧是封闭枚举：业务模块值（如 `TEMPLATE`）在菜单筛选器显示为 UNSPECIFIED，运行时字符串开放，只影响展示。
- 业务 pack 不带 docs 面（Swagger/Redoc 挂在 admin-surface 的设置节点下）；要给自己的契约出文档，需自嵌 OpenAPI 资产并避开 `/q/*` 路径。
- `rushwind-gen-http` 对路由是 first-match 语义：模块内新增 rpc 时跑一遍守卫测试（`shadowed` 断言）确认无遮蔽。

## 深读

- 模块骨架逐文件说明：[`backend/business/module-template/README.md`](../../backend/business/module-template/README.md)
- 边缘栈组合子（bind/门/审计的层序契约）：`backend/services/admin-api/src/server/edge.rs` 模块文档
- 生成引擎的管线细节：[03 契约与生成链路](./03-codegen-chain.md)
