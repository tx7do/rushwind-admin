# 06 · 多租户与行级隔离

> 前置：[05 权限模型](./05-permission-model.md)。读完你会理解："行级谁的数据"——HTTP 闸门、数据层谓词、数据范围与套餐联动，以及新表接入隔离的清单。
>
> 权威规格指针：闸门失败信封见 [binding-spec.md](../binding-spec.md) 与 [第 4 章 §3.3](./04-first-service-module.md)；claims 载体见 [第 5 章 §2.2](./05-permission-model.md)。

## 1. 两道闸，各管一层

租户隔离不是一个点，是叠在两个层面的闸：

```text
HTTP 层（crates/auth::TenantAccessChecker）——"这个租户能不能用这个接口"
    │  按 (路径模板, method) 判定，403 信封拒绝
    ▼
数据层（apps/admin/src/admin/data/scope.rs::Viewer）——"这行数据是不是你的"
    │  每条 SQL 的谓词强制，查询/写入/删除全覆盖
    ▼
PostgreSQL
```

两层都过才有数据。只懂其中一层就会把"接口 403"和"列表为空"混为一谈：接口 403 是 HTTP 闸拒绝（租户没这个模块的套餐），列表为空是数据层谓词把别家的行滤掉了——同一次请求只撞其一。

## 2. HTTP 闸门：七步判定

租户登录（`tid > 0`）的每个请求按序过七步，任何一步失败即 403（`FORBIDDEN` 信封，message 由 check 自己带）。实现在 `apps/admin/src/admin/authorizer.rs` 的 `TenantAccessChecker::check_tenant_access`，逐步对照：

| 步 | 判定 | 失败消息 | 代码锚 |
|---|---|---|---|
| 1 | 租户行存在 | `access denied` | `find_by_id(tenant_id).ok_or_else(denied)?` |
| 2 | 状态 = ON | `tenant is not active` | `status.as_deref() != Some("ON")` |
| 3 | 到期且套餐为只读策略 → **写方法拒绝**（GET/HEAD/OPTIONS 放行） | `tenant is read-only due to expiry` | `expired && expiry_policy == "READONLY" && !matches!(method, "GET" \| "HEAD" \| "OPTIONS")` |
| 4 | (路径模板, method) 在 Api 表注册（**fail-closed**：未注册即拒） | `access denied` | `sys_apis` 按 `path + method.to_uppercase()` 查，落空即拒 |
| 5 | 路由所属业务模块 ≠ UNSPECIFIED | `module not allowed` | `business_module` 空 / `MODULE_UNSPECIFIED` 即拒 |
| 6 | 租户有套餐 | `no subscription plan` | `plan_id == 0` |
| 7 | 套餐模块白名单包含该模块 | `module not allowed` | `sys_plan_modules` 按 `(plan_id, module)` 查 |

三个要点：

- **fail-closed 是基调**：第 4 步意味着新接口忘了注册 Api 表，租户侧一律 403——宁可拒不可漏。配套机制在启动种子：`sync_apis` 把打进二进制的 OpenAPI 端点清单按 (path, method) MERGE 进 `sys_apis`（失败不阻断启动，fail-open 留痕，重跑 SyncApis RPC 即自愈）——新契约部署后若不同步，租户侧全端点 403；
- **到期只读是"写拒读通"**：过期租户还能登录看数据，但任何写方法被第 3 步拦下。第 3 步的判定是 `expired_at < now` **直接算**，不依赖租户状态列——所以它在 cron 落状态之前也生效；
- 第 7 步还有一个前置分支：套餐**一条白名单都没有**时返回专属文案 `tenant plan has no whitelisted modules`——"module not allowed" 会让租户管理员去追一个任何配置都满足不了的模块名，这条文案告诉他问题在套餐本身；
- 平台管理员（`tid = 0`）整段跳过——平台域不受租户闸门约束（`auth_gate` 里 `if tenant_id > 0` 才进这段）。

**到期与 cron 的一小时窗**：系统 cron `tenant_expiry_scan`（每小时整点）把 `status = ON && expired_at <= now` 的租户行置为 `EXPIRED`。cron 跑过之后，租户在第 2 步就吃到 `tenant is not active`（全方法拒，含 GET）；cron 还没跑到的窗口里，租户走第 3 步的"写拒读通"。两个阶段之间是扫描周期，不是矛盾——判定链对两种状态都安全。

套餐侧数据模型：`sys_plans`（套餐，含 `expiry_policy` 到期策略）→ `sys_plan_modules`（模块白名单）+ `sys_plan_quotas`（用量上限）；租户到期由上述 cron 扫描落状态。

## 3. 数据层：Viewer 三态

`apps/admin/src/admin/data/scope.rs` 是**唯一**的租户判定点。文件全文不到 90 行，三态判定收在两个方法里：

```rust
pub enum ViewerKind {
    Noop,    // 无凭据（公开路由）
    User,    // 已验证用户：tenant 0 = 平台
    System,  // 旁路租户（种子、内部作业）
}

/// 租户强制——行必须被约束时 Some(tenant)，
/// 平台/系统/noop 的宽读为 None。
pub fn tenant_scope(&self) -> Option<i64> {
    match self.kind {
        ViewerKind::User if self.tenant_id > 0 => Some(self.tenant_id),
        _ => None,
    }
}

/// 创建盖章：租户 viewer 把自己的租户强按到新行上。
pub fn stamp_tenant(&self, requested: Option<i64>) -> Option<i64> {
    match self.kind {
        ViewerKind::User if self.tenant_id > 0 => Some(self.tenant_id),
        _ => requested.or(Some(0)),
    }
}
```

| Viewer | 语义 |
|---|---|
| `Noop` | 无凭据（公开路由），只配白名单面 |
| `User (tid = 0)` | 平台视图：不加租户谓词，全租户行可见 |
| `User (tid > 0)` | 租户视图：查询/更新/删除强制 `tenant_id = tid`，创建强制盖章，跨租户写拒绝 |
| `System` | 系统视图：旁路全部租户谓词（启动种子、令牌清理等内部作业专用） |

`Viewer::from_ctx(&ctx)` 从请求 claims 解出（claims 缺 → Noop）——代租户场景下 claims 的 `tid` 已被门覆写成生效租户（§9），所以 Viewer 天然按所选租户行事。

由这个单点派生出的行为（不需要各仓储自觉）：

- 租户用户的任何 list/get/update/delete 都看不到、摸不到别家行——按 id 直查也逃不掉（谓词叠在 `find_by_id` 上）；
- 创建时 `tenant_id` 由服务端盖章（`stamp_tenant` 直接返回自己的 tid，请求里伪造的租户字段无效）；
- 子树/级联操作（如组织单元删除）在**语句级**携带谓词——混合租户的子树不会被越权删除。

## 4. repo_shell!：仓储骨架的租户形态

`apps/admin/src/admin/data/repos/mod.rs` 的 `repo_shell!` 宏是谓词纪律的落点。两个臂的展开体逐段对照（[第 4 章 §2.2](./04-first-service-module.md) 讲过 global 臂的骨架，这里看租户维度）：

### 4.1 tenant 臂

```rust
pub struct $name<'a> {
    pub db: &'a sea_orm::DatabaseConnection,
    pub viewer: $crate::data::Viewer,          // ← 比 global 臂多这一个字段
}

fn condition(&self) -> sea_orm::Condition {
    match self.viewer.tenant_scope() {
        Some(tid) => sea_orm::Condition::all()
            .add($entity::Column::TenantId.eq(tid))          // 租户谓词
            .add($entity::Column::DeletedAt.is_null()),      // 软删谓词
        None => sea_orm::Condition::all()
            .add($entity::Column::DeletedAt.is_null()),      // 平台/系统：只剩软删
    }
}
```

`list()` 与 `paged_list()` 把 `condition()` 叠进每次查询——**租户成员的每次列表天然只看自己租户的行**，平台/系统视角自动放宽到全租户（仍滤软删行）。

### 4.2 global 臂（与名词臂）

global 臂没有 viewer 字段、没有租户谓词（平台全局表如 `sys_languages`），软删谓词照铺。名词臂（`global $name, $entity, "noun"`）追加两个 id 寻址方法：

- `get_by_id(id)`：`find_by_id` + `DeletedAt.is_null()` 谓词——**查无或软删统一 404**，文案 `"<noun> not found"`；
- `delete_by_id(id)`：SeaORM 物理删，"行不存在算不算错"由调用方决定。

**故意没有"租户名词臂"**：租户表的 get/delete 必须携带 viewer 谓词，宏替你生成反而会诱导绕过——所以这些方法留在各仓储文件的手写 `impl` 里。手写样板见 `repos/role.rs`（viewer-scoped 仓储的实物）：

```rust
pub async fn get_by_id(&self, id: i64) -> Result<roles::Model, StatusError> {
    let mut query = roles::Entity::find_by_id(id);
    if let Some(tid) = self.viewer.tenant_scope() {
        query = roles::Entity::find_by_id(id).filter(roles::Column::TenantId.eq(tid));
    }
    query.one(self.db).await.map_err(db_err)?
        .ok_or_else(|| status_error("NOT_FOUND", "role not found"))
}
```

租户视角查别家角色 id → 谓词滤掉 → `role not found`（404 而非 403：不向调用方泄露"别家有这行"）。

另一类仓储是**显式参数化**的（模块文档点名的双习语之二）：`UserRepo` 等的方法带 `tenant_id` 参数，由**服务**指名范围（平台管理员列另一租户的用户）——viewer 管默认，参数管代查，两者不混。

### 4.3 deleted_at IS NULL 的三面覆盖

软删谓词在三个读面上都由骨架铺好：列表面（tenant/global 两臂的 `condition()`）、id 寻址读面（名词臂 `get_by_id`）、回收站对照面（租户 repo 的 `paged_deleted` 用 `is_not_null()` 反相——在营面与回收站互不可见，§8）。写面没有软删谓词——update 走 `get_by_id` 拿行，软删行第一跳就 404。

## 5. 数据范围：角色级的行过滤

租户内部还有一层"哪些组织单元的数据可见"——角色的 `data_scope` 五档（`sys_roles.data_scope` 列的枚举全集，缺省 ALL），登录时聚合进令牌（`dss` 多角色聚合数组 + `dsu` 单元 id 串，见[第 5 章 §2.2](./05-permission-model.md)）：

| 档 | 语义 |
|---|---|
| ALL | 全租户 |
| SELF | 仅本人 |
| UNIT_ONLY | 本组织单元 |
| UNIT_AND_CHILD | 本单元及子树 |
| SELECTED_UNITS | 自定义组织单元集（`dsu` 携带） |

聚合规则（`services/authentication/authority.rs`）：`dss` 收各角色的档名（去重、序稳定）；`dsu` 只对 `UNIT_*` 档的角色收 `sys_role_org_units` 的单元目标（并集、逗号串）。

与 Viewer 的关系：**Viewer 先圈租户，数据范围在租户内再圈组织**——两层谓词叠加，不是二选一。注意当前的消费现状：五档聚合与声明下发已就绪（`dsu` 在令牌里可直接取用），仓储侧按档生成 SQL 谓词的消费面仍在接入——接新行级过滤时以 `tenant_scope()` 之外的 `dss`/`dsu` 为输入，不要另起机制。

## 6. 套餐三件套与到期扫描

**三张表**：`sys_plans`（套餐主档：名称、`expiry_policy` 到期策略等）→ `sys_plan_modules`（每套餐一行一个模块的白名单）+ `sys_plan_quotas`（用量上限，随计费面接入）。HTTP 闸门的第 6/7 步就是这三张表的判定链：`tenant.plan_id` → `sys_plans`（拿 `expiry_policy`）→ `sys_plan_modules`（白名单比对）。

**到期扫描 cron**（`apps/admin/src/admin/server/apalis.rs`）：

- 注册名 `tenant_expiry_scan`，spec `0 * * * *`（每小时整点）；
- 判定链：`status = ON` 且 `expired_at` 非空且 `expired_at <= now` 的租户行全部查出，逐行 `status → "EXPIRED"`；
- 与闸门的衔接见 §2 的"一小时窗"说明：cron 落状态前走"写拒读通"，落状态后全方法拒；
- cron 面还有两个静态成员：`audit_log_archive`（`30 3 * * *`）与 DB 驱动的 `sys_tasks_periodic`（每分钟扫 `sys_tasks` 的 PERIODIC 行，业务定时任务随 DB 配置即时生效，无需重启）。

## 7. 租户开通与自证

`CreateTenantWithAdminUser` 在**一个事务**里完成开通（`services/tenant.rs::provision_with_admin`），四步：

1. **租户行**：`status = ON`、`audit_status = APPROVED`（API 侧缺省）、类型缺省 `PAID`；
2. **租户管理员角色**：从平台模板 `template:tenant:manager`（租户 0 名下）拷贝出 `tenant:manager`（`is_protected = true`、data_scope/home_path 随模板），模板的权限绑定逐行随迁；
3. **管理员账号 + 凭据**：`sys_users` 一行（`status = NORMAL`）+ `sys_user_credentials` 一行（bcrypt 哈希落 `credential`）；
4. **AssignTenantAdmin 语义**：`sys_user_roles` 主绑定（`is_primary = true`、`status = ACTIVE`）+ 租户行 `admin_user_id` 回指。

开箱即可登录管理自己的组织——组织树由租户管理员自建，全部落在第 3/4 节的谓词覆盖之下。开通前的查重是 OR 语义（code 或 name 任一撞在营行即 `tenant already exists`）；租户管理员再往下建部门、角色、用户，同样走谓词。

## 8. 租户回收站：停车、复原、硬删

### 8.1 Delete 的停车语义

租户删除不是删除，是**停车**（`services/tenant.rs::delete`）：

```rust
self.cleanup_rows(row.id).await?;               // ① 级联清关联行
let mut a: ActiveModel = row.into();
a.status = Set(Some("OFF".into()));             // ② 置 OFF
a.deleted_by = Set(Some(payload.user_id));      // ③ 盖操作者戳（回收站显示谁停的）
a.deleted_at = Set(Some(crate::admin::data::now()));
a.update(&self.state.db).await.map_err(db_err)?;
```

- **级联清单**（`cleanup_rows` 的 `wipe!` 宏逐表 `delete_many ... TenantId.eq`）：角色与全部角色附表、用户与凭据/MFA、组织单元、岗位、字典、任务、登录策略、访问密钥、AI/BI/打印三域、内部消息、文件、六张审计表——**四十张**租户域表一次清空（issue #3 修正过 AI/BI/打印面的漏项）；
- **停车三件套**：`status = OFF` + `deleted_by` + `deleted_at`。`deleted_by` 让回收站能显示"谁停的车"；
- 停车行在全部在营读面隐身：`TenantRepo` 的每个读方法都叠 `alive()`（`DeletedAt.is_null()`）——`find_by_id/find_by_code/find_by_name/paged_list/exists_*` 全覆盖。**exists 闸随之放开**：停车租户的 code/name 可以被新租户占用（测试钉住：停车后重建同名租户通过 exists 闸）。

### 8.2 三个平台 RPC 逐个走

**ListDeletedTenants**——回收站列表（`TenantRepo::paged_deleted`）：

```rust
tenants::Entity::find()
    .filter(tenants::Column::DeletedAt.is_not_null())   // 与 alive 域互斥
    .order_by_desc(tenants::Column::DeletedAt)          // 删除时间降序
```

`is_not_null` 与在营面的 `is_null` 是同一列的两副面孔，互不可见；分页走同一条 `fetch_paged` 管线。三个 RPC 全部过 `require_platform_admin` 闸（非平台管理员 403 `only platform admin can …`，测试钉住）。

**RestoreTenant**——清戳复原：

1. `find_by_id_unfiltered` 拿原始行（**不做 alive 过滤**——停车行就靠它寻址）；
2. `deleted_at.is_none()` → 400 `tenant is not parked`（在营行不走这个口）；
3. **alive 域查重**：code/name 被在营租户占用即 400 `tenant already exists`（与 create 同文案——停车期间编号被占是恢复的头号冲突）；
4. 清戳：`deleted_at = None`、`deleted_by = None`、`status = ON`，`stamp_update!` 盖 `updated_by/at`。

**空壳语义**：恢复出来的是空壳——delete 时关联行已被级联清理，`admin_user_id` 指向已删用户仅是历史痕迹（`user_count` 归零，测试断言）。平台管理员恢复后需另行补管理员账号（`create_tenant_with_admin` 面向新租户，不适用）。

**PurgeTenant**——硬删，不可逆：

1. 同样 `find_by_id_unfiltered` 寻址；在营行拒绝：400 `tenant is not parked; delete it first`（想抹掉在营租户请先走 delete 停车）；
2. `cleanup_rows` 再跑一遍级联（停车期间若仍有关联残行则清干净）；
3. `sys_tenants::Entity::delete_by_id` 物理删——**连不带租户谓词的读面也不再可见**（`find_by_id_unfiltered` 返回 None，测试断言）。

三 RPC 全链的时序在 `recycle_bin_chain` 测试里钉成一条线：删除停车 → trash 可见（deleted_by 有戳）→ 在营占用者让 restore 拒绝 → purge 占用者 → restore 成功（清戳、ON、空壳）。

## 9. X-Tenant-Id：平台管理员代租户

实现全在 `crates/auth/src/lib.rs` 的 `auth_gate` 里（[第 5 章 §1](./05-permission-model.md) 四阶段中"租户检查"之前的一段），三段逻辑：

```rust
let token_tenant_id = claims.0.get("tid")...unwrap_or(0);
let is_platform_admin = roles.iter().any(|r| r == "platform:admin");
let impersonated = if token_tenant_id == 0 && is_platform_admin {
    parse_impersonated_tenant(req.headers().get("x-tenant-id")...)
} else {
    None                                          // 租户成员 / 非平台管理员：一律忽略
};
let tenant_id = impersonated.unwrap_or(token_tenant_id);
```

- **谁可代**：令牌 `tid = 0` 且 `roc` 含 `platform:admin`（注意是**角色编码**判定，不是 `ipa` 声明——`ipa` 是登录时打的平台管理员标志，门的判定按角色集）。租户成员与非平台管理员携带该头**一律忽略**，其租户由令牌决定；
- **头的解析**（`parse_impersonated_tenant`）：正整数十进制才生效；缺失/空白/非数字/零/负数一律不构成代租户——宽松失败，垃圾值不得变成另一个身份（单测逐值钉住：`"42"` 生效、`"0"/"-3"/"abc"/"1.5"` 全不生效）；
- **闸门同标准**：`tenant_id > 0` 时对该租户跑完整租户阶段（存在/启用/到期只读/套餐白名单）——平台管理员代一个到期租户写数据，同样吃 `tenant is read-only due to expiry`；
- **tid 换乘**：授权评估用生效租户，评估通过后 `claims.0.insert("tid", effective)` 覆写声明包再注入请求上下文——下游 Viewer 的租户视图、建行的租户归属全部按所选租户行事，**令牌本体未动**；
- **审计归属**：审计面从 bearer 令牌直接解 claims（`audit.rs::claims_from_token`，不验签只取归属）——记录的是令牌原始 tid（0，平台侧）与真实 uid，不是代租户的 tid。审计答"谁干的"，业务行归属答"给谁干的"，两个问题两个来源；
- 头缺失即平台全域视图（现状行为）。

## 10. 新表接入与跨租户陷阱

### 10.1 新表接入租户隔离的清单

1. 实体带 `tenant_id` 列（全局表除外），注册进 `migration.rs`；
2. 仓储经 Viewer 派生谓词（用既有仓储骨架，不自写裸查询；租户表的 get/delete 在手写 impl 里带谓词）；
3. 路由的 (path 模板, method) 注册进 Api 表并归类业务模块（启动 `sync_apis` 自动 MERGE，人工兜底走 SyncApis）；
4. 套餐若需管控该模块 → 模块进套餐白名单语义；
5. 台架/探针里加一条**跨租户访问用例**（A 租户令牌摸 B 租户行 → 应 404/403）。

### 10.2 跨租户陷阱清单

- **裸 `find_by_id` 不带谓词的路径**：服务里绕开仓储直接 `Entity::find_by_id(id)` 是唯一能造出越权读的写法——骨架方法全带谓词，裸查全不带。仓储层唯一的故意裸查是 `TenantRepo::find_by_id_unfiltered`（回收站寻址，配 `tenant is not parked` 检查），新代码不得复制这个形态；
- **平台行 vs 租户行的种子归属**：启动种子全部挂租户 0（`platform:admin`、`template:tenant:manager`、BI 模板 `tenant_id: Some(0)`、菜单/权限/接口行）——它们是平台资产，租户谓词下不可见（租户看到的是开通时拷贝出的自有副本）。新种子挂错租户会让租户要么看不见该看的、要么看见别家的；
- **显式参数化仓储的参数来源**：`UserRepo` 类方法带 `tenant_id` 参数时，参数必须来自服务端判定的范围（平台管理员的显式代查、租户自己的 tid），不能把请求体里的租户字段直接喂进去——那等于把盖章权交回客户端；
- **测试上下文的租户面**：`testsupport::ctx(tid, uid)` 里 tid 决定 Viewer 形态——租户表测试要成对写（`ctx(7,1)` 建的行 `ctx(9,2)` 必须看不到），单边测试测不出谓词漂移；
- **`no_paging` 不豁免谓词**：不分页只是不切片，`condition()` 照叠——别在"导出全量"场景绕开 repo 的 paged_list 自拼查询。

## 11. 本章小结

- HTTP 闸门管"能不能用"，七步 fail-closed（含 `tenant plan has no whitelisted modules` 的专属文案）；数据层 Viewer 管"是不是你的"，谓词全覆盖；
- 到期只读、模块白名单、套餐配额是租户运营的三道商业闸；到期判定与 cron 状态机之间有一小时窗，两态判定都安全；
- `repo_shell!` 的 tenant 臂 = viewer 字段 + 双谓词 condition；租户 get/delete 手写带谓词，故意没有租户名词臂；
- 租户删除是停车进回收站（三个平台 RPC 管理复原与硬删；deleted_by/deleted_at 盖戳；alive/is_not_null 两副面孔互斥），平台管理员可携 `X-Tenant-Id` 代租户操作（门跑同标准、tid 换乘、审计仍归真实操作者）；
- 平台/租户/系统三态是全部特权的来源，别在服务里另开旁路；
- 新表接入按清单走，跨租户用例是回归的必测项。

下一步：[07 · 审计与等保](./07-audit-compliance.md)。
