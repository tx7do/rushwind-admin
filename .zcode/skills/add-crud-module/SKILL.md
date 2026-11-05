---
name: add-crud-module
description: End-to-end guide for adding a new CRUD business module to the RushWind admin Rust backend — contract-first (upstream proto snapshot) → cargo regen of the generated HTTP surface → SeaORM entity/migration → repo → service → mount → seed → gates → smoke. Use whenever the user wants to add a new module, resource, entity, or management page; create a CRUD feature; or says things like "新增模块/新增CRUD/加一个 xxx 管理/新建管理页/新增资源/新增功能/add a module/create CRUD entity". Also applies to adding fields or endpoints to an existing entity (same contract pipeline, reduced steps). Do NOT trigger for bug fixes, refactors, or logic changes inside an existing module. Trigger even when the user only mentions the frontend page — the three frontends are byte-frozen upstream snapshots in this repo, so any page work routes back to the upstream contract source, and the backend pipeline is always the local work.
---

# 新增 CRUD 模块（RushWind admin）

本 skill 在 rushwind-admin 仓内落地一个**完整 CRUD 业务模块**：契约先行，随后按生成面对照手写 Rust 各层，最后过门禁、冒烟。全部步骤与样例引用都来自本仓的真实代码（不是文档愿景）。

## 与典型全栈仓的根本差异（先读这一段）

本仓的 **proto 契约树和三端前端都是字节冻结的同步快照**，不是本地自由编辑区：

```
上游契约源 ──sync-protos.sh──▶ backend/api/protos/（116 proto，MANIFEST.sha256 门，禁手改）
上游三端   ──sync-frontend.sh─▶ frontend/admin/{react,vue-element,vue-vben}/（双清单门，禁手改；
                               唯一允许的偏离是 brand/ 品牌覆写层）
```

因此本 skill 的主线是：**契约在上游落好 → 同步进来 → `cargo build -p proto` 重生成契约面 → 照生成面手写 Rust 各层**。前端页面代码不在本仓写（见 `references/frontend.md`）。

## 三层架构一口气

```
契约（上游先行）:  domain proto + BFF proto → sync-protos.sh（字节快照 + MANIFEST 门）
生成面（自动）  :  cargo build -p proto → 类型 / ROUTES / *Handlers trait / null 桩 / mounts
                 （build.rs 的 rerun-if-changed 挂在 api/protos，改完契约直接 build 即重生成）
手写层          :  SeaORM 实体 → migration 批次 → mapping 枚举表 → repo(repo_shell!) →
                   service(impl *Handlers) → services.rs 注册 → rest.rs 挂载 → seed 菜单/数据
收尾            :  fmt/clippy/test 门禁 → 登录冒烟 → 文档计数翻新
```

**硬规则：**
1. **契约树禁手改**。`bash backend/api/sync-protos.sh --check` 不过（exit 1）= 门禁必红。要改契约，回上游契约源改完再同步（或走 `references/backend.md` Step 1 的本地直加流，并接受其债务属性）。
2. **`docs/binding-spec.md` 是逐字节对齐权威**（对 Go 参照实现的线上行为基准）。凡本 skill 与它冲突，以它为准；凡与参照实现有行为分歧，先改 binding-spec、再动代码、后跑差分。
3. **契约面生成之前，Rust 手写层无从谈起**——实体之外的一切（Handlers trait、请求/响应类型、mount 函数）都来自 `cargo build -p proto` 的生成物。永远契约先行。

## Step 0 — 澄清需求（一次问全）

动工前向需求方确认（合并成一轮问完）：

1. **实体名三形式**：PascalCase（`Product`）、snake_case（`product`）、中文标签（`商品`）。
2. **核心字段**：`{name, type, enum?, required?}` 清单。审计字段不必列——proto 字段号分段固定：业务字段 1..20+，`created_by`=100 / `updated_by`=101，`created_at`=200 / `updated_at`=201，全部 `optional` + `json_name` 驼峰。
3. **功能域**：归入既有域（`dict`、`identity`、`message`、`notification`、`ops`、`platform`、`rbac`、`scope`、`audit`）还是新域？这决定 proto package、`data/<domain>.rs`、以及是否新建 proto 目录。
4. **上游协调**：该实体的 domain + BFF proto 在上游契约源里是否已存在？不存在则需先在上游落 proto（本仓祝福流），或走本地直加流（下次同步会被抹掉，见 backend.md Step 1 的两种路）。
5. **模块形状**：plain CRUD / 树（parent_id）/ 主从（两表联动）/ 租户域。plain 是默认；租户域 repo 不走 noun 臂（见 backend.md Step 8），形状差异都以前两者样板外推。

## Step 1 — 执行顺序（防 80% 错误的那条规则）

```
1. 契约  → backend.md Step 1–3：proto 就位（上游流或本地直加）→ buf 校验 → cargo build -p proto
2. 手写  → backend.md Step 4–11：实体/迁移/枚举/repo/service/注册/挂载/seed
3. 门禁  → backend.md Step 12：fmt → clippy → test → 冒烟
4. 收尾  → backend.md Step 13：文档计数翻新
5. 前端  → references/frontend.md：快照语义 + 三绿验收（本仓不写页面代码）
```

如果需求方坚持「只做前端」：停下解释——本仓前端是冻结快照，页面改动属于上游；本仓侧能做的只有菜单 seed 行与验收。

## Step 2 — 后端

动第一个后端文件前**通读 `references/backend.md`**。十三步概览：

```
1 契约两条路 → 2 buf 校验 → 3 cargo build -p proto 重生成 → 4 免鉴权判定（默认不动）
5 SeaORM 实体 → 6 迁移批次 → 7 枚举映射 → 8 repo → 9 service → 10 注册与挂载
11 seed → 12 门禁与冒烟 → 13 文档计数
```

**写你的版本前先读同形状的真实样板**（复制结构，改名字段）：

| 你的模块形状 | 样板（全套文件） |
|---|---|
| 平台全局表 plain CRUD | notification 域（commit fef3b71）：`data/notification.rs`、`data/repos/notification_rule.rs`、`services/notification_rule.rs`、`server/rest.rs` 挂载行、`seed/menu_data.rs` |
| 租户域（viewer 谓词） | `data/repos/internal_message_category.rs`（手写 `find_scoped/delete_scoped`）+ `services/internal_message_category.rs`（`query_by_id!` 用法） |
| 只读台账 | `services/notification_delivery.rs`（list/get 形状） |
| 存量表补列 | `migration.rs` 的 `ChannelWebhookColumns`（`execute_unprepared("ALTER TABLE ... ADD COLUMN IF NOT EXISTS ...")`） |

## Step 3 — 前端

读 **`references/frontend.md`**（短）。要点：三端是冻结快照 + 品牌覆写；generated TS 随快照同步进来，本仓没有 `make ts`；本仓侧的动作只有三端重同步、菜单 seed 行与三绿验收（差分回放 + 门禁全绿 + 页面冒烟）。

## Step 4 — 完成清单

声明完成前逐项验证：

- [ ] `bash backend/api/sync-protos.sh --check` 通过（没动过契约树则天然绿）
- [ ] `cargo build -p proto` 后生成面含新 service：`routes::ROUTES` 追加了新路由、`gen::services` 有 `<X>ServiceHandlers` trait（生成头 `Corpus:` 计数已增长）
- [ ] `cargo fmt -p proto -p auth -p admin-api -p admin-diff -- --check`、`cargo clippy --workspace --all-targets -- -D warnings`、`cargo test --workspace` 全绿
- [ ] 没有手改任何 `generated`/快照目录：`backend/api/protos/`（MANIFEST 门）、`frontend/admin/*`（双清单门）、`backend/crates/proto` 的生成物在 OUT_DIR（不进 git）
- [ ] `server/rest.rs` 的 `mount_services!` 表已加行——漏行的症状是路由存在但永远 500（null 桩在应答）
- [ ] 免鉴权白名单没动（管理端 CRUD 默认全 gated）；若真加了公开端点，`auth_free.rs` 与 `tests/whitelist.rs::EXPECTED` 两表 1:1
- [ ] 冒烟走通：`admin / Abcd@1234` + Redis 读验证码登录，带 JWT 请求新路由（配方见 backend.md Step 12）
- [ ] 菜单 seed 行已插（`seed/menu_data.rs` ROWS，parent 在前）；已有部署的旧库靠各端「菜单同步」补行——seed 的 `count == 0` 守卫只对空库生效
- [ ] 路由/端点总数变化已翻新三语 README 与 `docs/binding-spec.md` §1.2 计数
- [ ] 差分回放（如需）：`cargo run -p admin-diff -- --go http://127.0.0.1:27788 --rust http://127.0.0.1:7788`；**未经用户明确允许不得启动/触碰 docker 差分栈**（testbed README 红线）

## Step 5 — 横切坑（每个模块都会遇到，先记后读样板）

1. **`{ data: {...} }` 包裹只属于 CRUD**：Create/Update 的 proto 请求体是 `data` 包裹 + `update_mask`，服务层统一 `require_data(req.data)?` 解包（缺失 → BAD_REQUEST）。但 `body: "*"` 的自定义 RPC（如 `test-dispatch`）收的是**扁平请求体**——多包一层 `data` 会被 protojson 当未知字段丢掉，接口照样 200、字段全空。
2. **Update 的字段写入语义**：样板 service 的 update 是 `get_by_id → ActiveModel → upsert_fields → stamp_update! → update`；`upsert_fields` 是 `if let Some` 逐字段写入——**populated 才写，None 即不动，没有清空通道**，`update_mask` 在样板中不被消费。部分更新语义的最终裁决权在 `docs/binding-spec.md` 与差分回放，不要在实现里自行发明 mask 过滤。
3. **搜索谓词只有两个活性算子**：`EQ`（裸键）与 `CONTAINS`（`__contains` 后缀），全表见 `docs/operator-matrix.md` §4。全局搜索走 `__contains`；ID 类字段不进模糊搜索。
4. **枚举必须过 `mapping.rs`**：DB 存字符串字面量、wire 上是 protojson 枚举名，双向都走 `enum_map!` 表（round-trip 测试兜底）；未知枚举值在 `upsert_fields` 里落 `status_error("BAD_REQUEST", ...)`。禁止在 service 里裸字符串转换。
5. **租户域 repo 没有 noun 臂**：`repo_shell!(tenant ...)` 刻意不生成 `get_by_id/delete_by_id`——租户的 get/delete 必须带 viewer 谓词，手写（样板 `data/repos/internal_message_category.rs`）。全局表才用 `repo_shell!(global X, entity, "noun")` 的自动 404/delete。
6. **null 桩是合法里程碑，不是完成态**：`cargo build -p proto` 之后、手写 service 之前，新路由由 `nulls::null_*_service()` 应答 500/not-implemented（先例 commit 43acfeb 就是这么接受新 vintage 的）。别把「路由通」当成「模块完成」。
7. **本地直加的 proto 是债务**：`sync-protos.sh` 的 sync 模式会 `rm -rf protos/` 整树重同步——本地独有文件会被下一次同步抹掉。除非上游明确不收，否则走上游流。

## 维护约定

踩到本 skill 没写的坑，修完代码后**回填进对应文件**（SKILL.md 的横切坑或 references 的 Step/Pitfalls），并给出真实样板引用——上游同类手册的每次演进都是这么沉淀的。文档里的计数（116 proto、Corpus 211/441、53 测试）会随演进漂移，以仓库实际为准并随手翻新。
