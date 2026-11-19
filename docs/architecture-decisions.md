# 架构决策与演进路线（architecture-decisions.md）

> **定位**：本仓架构层面"已定稿什么、已否决什么、什么条件下演进到什么"的唯一权威记录。它回答三类问题：要不要拆模块、契约（含 BFF 投影面）住哪里、哪些改动受工程约定约束。
> **什么时候读**：动仓库布局前；评估"业务域要不要打包成独立 lib"前；做全量上游同步前；合并横跨多分支的长寿命工作前。

---

## 0. 现状快照（2026-11-19）

- **形态**：单应用 `backend/apps/admin`——上游同步面（`src/admin` + `api/protos/`）、下游自有面（`src/modules` + `api/fork/protos/` + `main.rs` 四行装配/模块）一条所有权线，见 [CONTRIBUTING](../CONTRIBUTING.md)。
- **验证基线**：`cargo test --workspace` 296 绿（纯逻辑单测 + sqlite 内存 DB 测试面 + wire/golden/binder/envelope/whitelist + 模板守卫 + admin-diff 语义面）；`cargo fmt --check` 绿；`clippy -D warnings` 绿；`sync-protos.sh --check` 的 MANIFEST 面通过。
- **已知未跑**：boot smoke 与 admin-diff 实机差分（需起 Postgres/Redis 的部署栈）。

---

## 1. 现行形态：单应用（已定稿，否决回潮）

### 1.1 形状

```
backend/
├── apps/admin/                  # 唯一产品 crate：一池一类型一装配
│   ├── src/admin/               # 上游同步面（59 个挂载服务面、数据层、路由边、审计、迁移种子）——fork 只读
│   ├── src/modules/template/    # 下游自有面骨架（copy-rename 起步；i_notice BFF 随域走）
│   ├── src/proto/               # 契约生成面（api/ 树 → 一个 buf 工作区 → 一池一类型）
│   ├── api/
│   │   ├── protos/              # 上游镜像区（含 BFF 投影层，见 §2；由 MANIFEST.sha256 钉死）
│   │   ├── MANIFEST.sha256      # 镜像区 checksum 门（sync-protos.sh --check 的验收面）
│   │   ├── buf.yaml / buf.lock  # buf 工作区（admin 树 + fork 树一池一类型）
│   │   ├── fork/protos/         # fork 自有契约区（notice 三件套：notice/i_notice/notice_error）
│   │   └── sync-protos.sh       # 同步/校验门（HERE 相对，随树迁移）
│   └── main.rs                  # 装配：每模块四行（mod 声明、迁移链、种子、route pack）
├── crates/auth                  # 鉴权门（无契约反向依赖，401/403 锚为字面量 + 等价守卫）
└── crates/biz-proto-build       # 契约构建引擎（extra_type_roots / Face.source_dir 旋钮）
```

### 1.2 跨线消费 = 下游稳定契约（机械清单）

模块只允许经以下入口骑基座（签名与层序语义按稳定契约维护，改动须 PR 显式声明）：

- `crate::admin::{state, audit, data}`（状态助手、审计 face、repo_shell 实体/种子实体）与 `crate::admin::server::edge::Wrap`（模块 pack 的闭包签名）
- 四个 `#[macro_export]` 宏：`crate::repo_shell!` / `crate::stamp_update!` / `crate::query_by_id!` / `crate::set_masked!`
- 路由边组合子：`EdgePack` / `edge_router` / `pack_with`（`server/edge.rs`，双桶挂载循环）与 `crate::pool()`（描述符池句柄，装进 `EdgePack.pool`）
- crate 根 re-export：`config` / `crypto` / `data` / `migration` / `paging` / `seed` / `server` / `state`（apalis 不在根清单——住 `admin::server::apalis`）+ 根模块 `proto`
- 模块签名直接触及的框架面：`rushwind_bootstrap::{RouteInput, RouteSurface, BootstrapError}`、`rushwind_http_binding`（envelope/ctx）、`rushwind_storage_seaorm_migration`（EntityTables/MigratorTrait）、`sea_orm`

**围栏现状**：这条线目前靠约定 + 模板守卫测试（编译失败即稳定面被破坏）维护，尚无"模块目录只许 import 白名单路径"的源码围栏测试。**建议补**（一个源码扫描测试），未落。

### 1.3 已否决：恢复旧四桶布局（`business/` 独立 crate 回潮）

2026-11-19 评估结论：**不恢复**。三条结构性理由：

| 旧布局的缝 | 尸检 | 恢复的代价 |
|---|---|---|
| PagingRequest 转码 | 双 crate 各自 buf 池 → 模块信封 ≠ admin 信封，边界手写转码 | 模块独立 crate ⇒ 双池不可避免，转码必复活 |
| 手抄 AUDIT_ROWS 字面量 | 模块自带审计行拷贝 + 防漂移镜像守卫 | 单池后从 face 生成 ROUTES 派生，字面量无处存在 |
| AuditFace clone-at-pack-build | face 是 Arc 共享句柄 | 已死 |
| `$crate` 四连导出 | 跨 crate 宏展开的路径地狱 | 现 crate 根 re-export，branch 代码逐字落位（2026-11-19 修复轮实证） |

**实证**（2026-11-19）：branch 侧一整条功能列车（update_mask 写面 5600+ 行、租户回收站、渐进验证码、全套 DB 测试）在新布局上零语义改动落位，适配仅路径归一化与 crate 根 re-export——单应用承载上游同源功能没有结构性摩擦。

**重开条件**：仅当产品形态要求（模块独立售卖/独立授权/闭源模块配开源底座），且届时正确形状是 §3.3 的 `admin-contract` 叶子抽取，**不是**四桶回潮。

---

## 2. 契约区划与 BFF 投影层（i_*.proto）

### 2.1 双区制（已存在，勿动）

| 区 | 位置 | 所有权 | 校验 |
|---|---|---|---|
| **上游镜像区** | `api/protos/` | 随同步波次落位，手改禁止 | `MANIFEST.sha256`（sync-protos.sh --check） |
| **fork 自有区** | `api/fork/protos/` | fork 全权（域业务契约 + 域 BFF） | 无 checksum 门（git 即门） |

一池一类型：两区同入一个 buf 工作区，`biz-proto-build` 编出唯一描述符池；admin face 取镜像区，域 face 取各自树（`Face.source_dir`）。`redact.v1` 注解与 `method_skip` 覆全池，不感知分区。

### 2.2 BFF 投影层的定性（2026-11-20 修订）

`api/protos/admin/service/v1/i_*.proto`（**59 面**，i_access_key 至 i_user_profile，另有模块树的 i_notice）是 **admin 契约的 BFF 投影层**：逐服务镜像领域面（redaction / method_skip / 嵌套信封），随同步波次演进。

**上游镜像仓同样携带这 59 面**（2026-11-20 实测：57 面逐字节同文，i_tenant 仅空白行差，i_ai_content 上游领先一条 body 注解——正常 vintage 漂移）。因此定性由"fork 创作"修订为：**投影面属同步语料的一部分，变更入口在上游**（镜像区手改本就被 MANIFEST 门禁止）。`i_` 前缀升格为官方规则：**`i_` 前缀 = admin 契约的 BFF 投影面**。

**它们留在镜像区**（定稿）：搬去 fork 区会拆散投影面与领域面的共演节奏，且引发路由注册表重划、审计 skip 派生改源、差分白名单重整——行为零收益的纯 churn；上游既已携带，镜像区是它们唯一合法的位置。

### 2.3 同步安全性（2026-11-20 修订）

曾评估 sync 模式的 `rm -rf protos/` + 上游全量拷贝会"无声抹除 59 个投影面"（依据当时"上游零 i_* 文件"的过时事实）。**上游既已整体携带投影层，全量同步会原样带回 59 面——该风险不复存在**，原定的 stash/restore 脚本补丁撤销。

需要正视的两点现状：

1. **脚本默认上游路径已失效**：上游仓布局已迁至 `backend/api/protos`，`sync-protos.sh` 的默认 `SRC` 停在旧路径——全量同步前需以 `GOWIND_ADMIN_API_DIR` 指认现路径（脚本修正随下一轮同步工程一并落）；
2. **退役政策不变**：上游对某投影面的删除/改名随同步波生效（与任何上游面同待遇），守卫网负责炸响——`template_gen_surface`（路由表/auth-free/错误表）、`whitelist`+`admin-diff semantic`（语料双向钉）、`golden` 的 method_skip；仓内不对镜像区做投机性修改。

另：当前上游 `i_ai_content.proto` 领先本仓一条 body 注解——待随下次同步吸收的正常漂移。

---

## 3. 业务域按需打包路线（分级定稿）

"二开人员把一个业务域打包成像 lib 一样的模块，按需打包"——按"按需"的层级分两条路线：

### 3.1 部署级按需（现架构直接支持，路线已定稿）

**语义**：某个部署的二进制要不要编进某个业务域。

**做法**：模块对（`src/modules/<domain>/` + `api/fork/protos/<domain>/`）之上加 feature 门——`[features] mall = []`，`main.rs` 四行装配按 `#[cfg(feature = "mall")]` 门控。不开 feature → 模块代码不编译、服务面不挂。

**代价与边界**：proto 池仍编译全部 fork 树（描述符死重，无运行时代价）；BFF 投影层不挂载（服务面同开关）。若未来真要池级按需，走分域描述符集 + boot 时合并（见 §3.3 第 4 条），现阶段不做。

### 3.2 分发过渡形态（零代码，现即可用）

二开交付以**模块源码包**为准：模板 pair 拷贝改名 + 域 `api/fork/protos/<domain>/` 子树 + main.rs 四行清单。模板的 copy-rename 演练（README 清单：proto 改名必须同动文件名/目录/import 路径）已验证该交付形态可走通。

### 3.3 分发级按需（触发后才做，目标形状已定稿）

**语义**：业务域作为独立 lib 产物，跨仓/跨团队分发，底座与二开各管各的版本。

**目标形状（三 crate 分层）**：

```
admin-contract        叶子：api 树 + biz-proto-build 生成面 + mount traits + 信封类型
      ↑                       ↑
admin-support         modules/mall 等域 crate（各自独立仓亦可）
（repo_shell! 家族/审计 face/state 类型）   ↑
      ↑                       │
apps/admin            二进制：features 选域，逐域四行装配
```

**四条关键设计决定**（每条对应旧布局的一条死缝，勿在设计时重新打开）：

1. **契约必须是叶子 crate**——域 crate 与 app 都依赖它，依赖图无环；类型只有一份，信封转码缝不复活。
2. **域 crate 物理看不见 app 内部**——只能 import `admin-support` + `admin-contract`；围栏由 Cargo 机制天然保证（现架构 §1.2 的约定围栏届时退役）。
3. **`#[macro_export]` 宏族搬进 admin-support**——`$crate` 跨 crate 展开回归，但有界（唯一支撑 crate）且被模板 copy-rename 演练守卫钉死。
4. **buf 工作区仍是一棵树**（admin 镜像区 + fork 各域子树）；`extra_type_roots` / `Face.source_dir` 旋钮即为该形状而设。池级真按需 = 各域描述符集 + boot 时合并 FileDescriptorSet（包不相交、信封文件从域生成中排除由契约 crate 提供）——仅在 §3.1 的"死重不可接受"被实证后才启用。

**迁移成本**：`crate::proto::` → `admin_contract::` 量级的 86 文件机械手术（引用 `crate::proto` 的文件实测数）+ 宏搬迁 + 全部守卫适配（与 2026-11 大爆炸重构同量级）。

**触发器**：出现第一个真实的跨仓二开消费者。在触发前，§3.1/§3.2 已覆盖全部现实需求。域进一步长出**进程边界**（独立伸缩/部署/团队/非 Rust 消费方）时，演进到多服务——四级阶梯与参考形态见 [服务演进路线](./services-evolution.md)（rushwind-cms 即第一级多服务的现成实现）。

---

## 4. 工程约定

1. **长寿命分支不得携带仓库布局变更**——布局迁移必须独立短周期轮次落地，落定后其余工作再续。
2. **重构轮与功能轮永不并行**——同一时段主线只允许一种性质的波次。
3. **合并跨目录挪移的分支时，modify-vs-delete 禁止按"目录已删除"整体裁决**——必须逐文件确认语义已迁移至新路径，未迁移项显式登记为"放弃"并留档。
4. **全量上游同步前必跑差分回归**（sync-protos.sh 头部既有此建议），同步后对照 §2.3 的投影面退役政策。

---

## 5. 决策日志

| 日期 | 决策 | 状态 |
|---|---|---|
| 2026-11（会话二十一） | 四桶布局坍缩为单应用 `apps/admin`；四条死缝以单池/单装配消除 | ✅ 定稿（c46015e） |
| 2026-11-19 | 否决恢复 `business/` 四桶布局；单应用 + 围栏为终局形态（重开条件见 §1.3） | ✅ 定稿 |
| 2026-11-19 | `i_` 前缀 = admin 契约的 BFF 投影面（官方命名规则）；投影面留镜像区 | ✅ 定稿 |
| 2026-11-19 | sync-protos.sh 加 i_* stash/restore + 上游胜出退役政策 | ❌ 2026-11-20 撤销（上游已携带投影层，全量同步原样带回 59 面；退役政策保留，见 §2.3） |
| 2026-11-19 | 源码围栏测试（模块目录 import 白名单） | ⏳ 建议，待落 |
| 2026-11-19 | 部署级按需（feature 门） | 📋 路线定稿，实施按需求 |
| — | 分发级按需（三 crate 分层抽取） | 📋 路线定稿，等触发器（首个跨仓二开消费者） |
| 2026-11-19 | 多服务演进路线独立成篇：L1 单体模块 → L2 feature 门 → L3 lib 三 crate → L4 多服务 → L5 规模化，rushwind-cms 为 L4 参考形态 | ✅ 定稿（见 [services-evolution.md](./services-evolution.md)） |
| 2026-11-20 | 文档审计轮：15 文档 50+ 处事实回填（291/296、491、602/286、59、61、283 等全部实测重钉）+ binding-spec 增 §2.5 update_mask 应用语义 | ✅ 完成（3e7e85b） |
| 2026-11-20 | 本文档第三路审计勘误：apalis 移出根 re-export 清单、§1.2 补 5 项模板实际消费的稳定入口、i_* 定性修订（上游已携带投影层，"fork 创作"表述作废）、迁移成本 203→86 文件 | ✅ 完成 |
