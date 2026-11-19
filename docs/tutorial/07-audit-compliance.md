# 07 · 审计与等保

> 前置：[01 架构全景](./01-architecture-overview.md)。读完你会知道：六类审计日志各记什么、**谁在哪条路径写入**、关键字段从哪来、怎么留存归档；身份鉴别三件套与 MFA 怎么用、各自在代码里的执行点；能力边界在哪。

## 1. 六类审计日志

| 日志 | 表 | 记什么 | 采集点 |
|---|---|---|---|
| 登录日志 | `sys_login_audit_logs` | 成功/失败登录、IP 归属地、设备、**风控打分** | 审计中间件（登录族操作） |
| 操作日志 | `sys_operation_audit_logs` | 操作类型、资源对象定位、请求快照、请求 ID | 审计中间件（写方法） |
| API 日志 | `sys_api_audit_logs` | 操作者、路径、方法、成功状态 | 审计中间件（除跳过名单外全部请求） |
| 数据访问日志 | `sys_data_access_audit_logs` | 数据访问行为、SQL 词法脱敏、涉及表名与数据分类 | 表与读面已就位，**当前代码无写入者**（见 §3.5） |
| 权限变更日志 | `sys_permission_audit_logs` | 权限变更的操作者、目标对象、原因、请求快照 | 审计中间件（写方法 + 可解析出目标） |
| 策略评估日志 | `sys_policy_evaluation_logs` | 每次鉴权判定的结果与上下文、trace_id | authz 评估器（[第 5 章](./05-permission-model.md)） |

分工可以按"生产者"归纳成三组：

- **审计中间件**（`apps/admin/src/admin/audit.rs`）：挂在路由包上的 axum 中间件，认证放行后、handler 前后采集，业务代码零侵入——登录、API、操作、权限四类日志都由它写；
- **authz 评估器**（`apps/admin/src/admin/authorizer.rs`）：鉴权门内部，每次评估落一行策略评估日志；
- **任务系统**（`apps/admin/src/admin/server/apalis.rs`）：不写日志，但拥有留存/归档的作业面（§8）。

审计层挂在路由包上、以 `AuditFace` 参数化：业务路由包的审计行同样写进这六张表（见 [09 章](./09-write-your-first-module.md)的模块审计面）。

## 2. 审计中间件面：一次请求怎么变成日志

### 2.1 挂载位置

`edge_router`（`server/edge.rs`）的组装顺序是：路由桶（公开/门内）→ docs 面 → extras → **审计层最外层**。最外层意味着它看到的是**最终状态码**（内层 gate 已跑完、handler 已返回），同一份装配逻辑覆盖两个路由包。

### 2.2 审计面（AuditFace）：路由表 + 三份跳过名单

```rust
pub struct AuditFace {
    routes: Vec<AuditRoute>,          // (method, path, operation_id)
    login_ops: Vec<&'static str>,     // 写登录日志的操作
    skip_api_audit: Vec<&'static str>,// 不写 API 日志的操作
    session_only: Vec<&'static str>,  // 写方法里跳过操作/权限日志的操作
}
```

admin 面的路由表直接取自生成的 `proto::gen::routes::ROUTES`（进程级 `OnceLock<Arc>` 单例，路由表零拷贝）；**业务模块面用 `AuditFace::from_rows` 同样从自己的 ROUTES 表派生，只是三份名单为空**。admin 面的名单（`audit.rs` 常量）：

```rust
const SESSION_ONLY: &[&str] = &[
    "/admin.service.v1.AuthenticationService/Login",
    "/admin.service.v1.AuthenticationService/RefreshToken",
    "/admin.service.v1.AuthenticationService/Logout",
    "/admin.service.v1.MfaService/VerifyMFAChallenge",
];
const LOGIN_OPS: &[&str] = &[
    "/admin.service.v1.AuthenticationService/Login",
    "/admin.service.v1.MfaService/VerifyMFAChallenge",
    "/admin.service.v1.AuthenticationService/Logout",
];
const SKIP_API_AUDIT: &[&str] = &[
    "/admin.service.v1.AuthenticationService/Login",
    "/admin.service.v1.MfaService/VerifyMFAChallenge",
];
const WRITE_METHODS: &[&str] = &["POST", "PUT", "PATCH", "DELETE"];
```

读法：`Login`/`VerifyMFAChallenge` 只进登录表（API 表跳过——失败尝试的风控语义在登录表，不重复记 API 行）；`Logout` 进登录表（action=LOGOUT）**同时**也写 API 行（它不在 skip 名单里）；`RefreshToken` 是纯会话维护，既不进登录表、也不进操作/权限表，但仍写 API 行。守卫测试 `admin_face_carries_the_login_family_skips` 钉住了这组关系。

### 2.3 关键字段的来源

- **操作者身份**：直接从 `Authorization: Bearer` 里解 JWT payload（**只解码不验签**——门内路由已在 gate 里验过，这里只做归因）。取 `uid`/`tid`/`sub`；匿名请求（无 token 或无 uid）归因为 `uid=0, tid=0`。
- **request_id 三级回退**：`x-request-id` → `x-correlation-id` → `x-fc-request-id`，全空则生成 uuid v4。同一请求的登录/API/操作/权限行共享同一个 request_id——这就是"串起同请求多类日志"的钥匙。
- **IP 双优先级分型**：审计层用 `IpSource::ForwardedFirst`（`X-Forwarded-For` 优先），鉴权门用 `IpSource::RealIpFirst`（`X-Real-IP` 优先）——**这是契约差异不是疏漏**：审计想跟链路追踪的视角（XFF 是请求源头链），鉴权想跟反代直写的真实客户端视角。对齐基准见 [binding-spec.md](../binding-spec.md)。
- **请求快照**：仅写方法且 `Content-Type` 含 `application/json` 的请求缓冲 body，上限 64 KiB（`MAX_BODY_SNAPSHOT = 64 << 10`），缓冲后重建请求交内层链正常消费。超限/非 JSON 不快照。
- **写入是 best-effort**：六类行的 insert 各自 `tokio::spawn` 里跑，失败只 `eprintln!("[audit] … insert failed")`，**永远不会把响应打断**。

## 3. 六表逐表：生产者与写入点

### 3.1 登录日志（中间件 × LOGIN_OPS）

`Login`/`VerifyMFAChallenge`/`Logout` 命中时写一行：`action_type` LOGIN/LOGOUT，`status` SUCCESS/FAILED（按最终 HTTP 状态），`login_method` 恒为 `PASSWORD`，`risk_score`/`risk_level`/`risk_factors` 来自打分引擎（§4）。用户名的取值链：请求体 JSON 的 `username` 字段优先，取不到再读 **`X-Audit-Username` 响应头**——这个头由 MFA 挑战校验端点回填（§6，挑战阶段请求体里只有 operation_id 和验证码，没有用户名）。

### 3.2 API 日志（中间件 × 全量 − SKIP_API_AUDIT）

每条请求一行（跳过名单之外），`api_operation` 是完整 operation id（如 `/admin.service.v1.UserService/Get`），`path` 列取 operation 第一段（`admin.service.v1`），`request_uri` 才是具体路径；`latency_ms` 中间件实测、`success` 即 `status == 200`、失败时 `reason = "HTTP {code}"`。

### 3.3 操作日志（中间件 × 写方法 − SESSION_ONLY − 未解析出操作）

条件：写方法 + operation 非空 + 不在 session_only。三个解析函数：

```rust
fn path_matches(template: &str, path: &str) -> bool   // {var} 段匹配任意非空段
fn resolve_operation(&Method, &path, &face) -> &str   // 路由表 first-match
fn last_numeric_segment(path: &str) -> Option<String> // 从尾扫描的纯数字段 = resource_id
```

- **动作映射**：只有写方法进操作日志，操作类型按路由语义映射为 `CREATE / UPDATE / DELETE / EXPORT / IMPORT / ASSIGN / UNASSIGN / RESTORE / PURGE`，映射表之外的动词一律落 `OTHER`——新增路由时对照动作映射补臂；
- **目标定位**：`resource_type` = operation 服务段去掉 `Service` 后缀小写（`UserService/Get` → `user`），`resource_id` = 路径最后一个纯数字段（`/admin/v1/users/42/roles/7` → `7`）；
- **解析失败的行为差异**：请求方法不在该路径的登记集（如 `PATCH /admin/v1/access-keys/42`）、或路径根本不是登记路由时，`resolve_operation` 返回空串——操作/权限日志整体跳过，但 **API 日志照写**（它只需要 method+path）。想查"某条没登记的路径谁在打"时看 API 表。

### 3.4 权限变更日志（中间件 × 写方法 − SESSION_ONLY − 目标解析失败）

与操作日志同条件，外加"能从 operation 解析出目标类型"。`action` 用的是 `parse_target_and_action` 的映射（**无 Export/Import 臂**——`Export` 落 OTHER，而操作日志的映射有 EXPORT/IMPORT，两份映射是有意差异：权限变更语义里导出不算权力变化）。`target_name` 从请求体 `data` 对象按 `name → title → username → nickname → realname → code` 取第一个非空字符串（无 `data` 信封时探测根对象）；`old_value`/`new_value`/`reason` 列是表契约的预留位，中间件通路未接线。

需要"评估语义"的一类由评估器在动作点落笔：`sys_policy_evaluation_logs` 的唯一写入者就是 authz 评估器（`authorizer.rs::authorize`），no-op 引擎下每个门内请求一行：`effect_details` 形如 `engine=noop subject=<role> latency=3ms; allowed`，`evaluation_context` 是含 engine/subject/action/resource/userId/tenantId 的 JSON；permission/policy 引用来自 `(path, method)` 端点快照回解（TTL 60s、容量 1024 的进程内缓存），回解失败落空引用不落错误。**免鉴权路由不产生评估行**（评估器只骑在门内）。

### 3.5 数据访问日志：表与读面已就位，写入面未接线（如实）

`sys_data_access_audit_logs` 的实体、迁移、租户删除时的 wipe、以及查询服务（`DataAccessAuditLogService` 的 List/Get）都在，但**当前代码没有任何写入点**——`sql_text`/`sql_digest`/`data_masked` 等列是契约预留，数据层钩子是规划项而非现状。排障时这张表为空是正常现象；使用前先确认你关心的事件在采集面内。

## 4. 登录风控打分

每次登录按启发式累计风险分并分档（LOW ≤ 30 < MEDIUM ≤ 70 < HIGH），打分逻辑是纯函数（`apps/admin/src/admin/audit/risk.rs`），输入输出都有单测钉住——调策略不改行为。

| 信号 | 分值 |
|---|---|
| 登录失败 | +50 |
| 无 uid（token 缺失/无 uid claim） | +10（带用户名，UNKNOWN_USER）/ +20（连用户名都没有，ANONYMOUS_LOGIN） |
| 缺设备标识 | +10（UA 为空即"缺设备"） |
| 缺 IP | +5 |
| 内网来源 | −10（10/8、127/8、172.16–31、192.168、169.254 私网段与 ::1/fc/fd 的 IPv6 ULA） |

总分 `clamp(0, 100)`。

### 4.1 算一遍完整示例

一次失败登录：用户名 `ghost`（库里没有 → 解析不到 uid，claims 无 → `user_id=0` 且用户名非空）、IP 是公网 `8.8.8.8`、UA 正常：

```text
失败            +50
未知用户名(带名) +10      # uid=0、username 非空
设备齐全          +0
IP 在           +0（公网也不加分）
内网            —        # 不适用
─────────────────────
合计             60 → MEDIUM
```

风险因子（去重排序后的字符串数组）：`FAILED_LOGIN`、`UNKNOWN_USER`、`EXTERNAL_IP`、`NO_SESSION`（恒在——中间件通路没有会话语义）、`MEDIUM_RISK_SCORE`。对照地，匿名 + 失败 + 无设备 + 无 IP 是最重档：`50+20+10+5 = 85 → HIGH`。

### 4.2 因子表的诚实说明

`risk_factors` 的签名还接收 `mfa_status` 与 `failure_reason` 两个输入（MFA_FAILED / MFA_UNVERIFIED / PASSWORD_FAILURE / MFA_FAILURE_REASON 四个因子由它们驱动），但**中间件通路的调用点把 mfa_status 传空串、failure_reason 恒传 `"login failed"`**——所以这四个因子从审计中间件出发永远不触发，它们是打分引擎为更完整调用方预留的臂。

### 4.3 UNVERIFY 字面量怪癖

`MFA_UNVERIFIED` 的判定是 `mfa.to_uppercase().contains("UNVERIFY")`——**字面子串**。英文习惯写法 `UNVERIFIED`（…F-I-E-D 结尾）并不包含 `UNVERIFY`（…F-I-Y 结尾），所以**不会**命中；只有状态字面量真的写着 `UNVERIFY` 才命中。单测 `factors_mfa_substring_is_literal_unverify` 把这个怪癖钉成了行为。接自己的 MFA 状态源时照字面量拼写传值，别靠语义猜。

## 5. 操作日志的细节约定

- **模板匹配**：`path_matches` 按 `/` 切段对齐——`{var}` 段匹配任意一个非空段，字面段必须逐字相等，段数必须一致。守卫测试里 `/admin/v1/users/{id}` 不匹配 `/admin/v1/users/42/roles/7`（段数差一）；
- **first-match 语义**：路由表按声明序扫，第一个 (method, path) 命中的 operation 就是答案。解析实例（`audit.rs` 测试 `resolves_against_the_admin_face_route_table`）：

  | 请求 | 解析结果 |
  |---|---|
  | `GET /admin/v1/access-keys` | `/admin.service.v1.AccessKeyService/List` |
  | `GET /admin/v1/access-keys/42` | `/admin.service.v1.AccessKeyService/Get` |
  | `POST /admin/v1/access-keys` | `/admin.service.v1.AccessKeyService/Create` |
  | `PATCH /admin/v1/access-keys/42` | `""`（该路由只登记了 GET/PUT/POST/DELETE，PATCH 无臂） |

- **解析失败落 OTHER 与落空的分界**：operation 解析出来了但动作方法不在映射表 → 动作落 `OTHER`（如 `Export` 在权限映射里）；operation 本身解析不出来（空串）→ 操作/权限日志整体跳过、API 日志照写；
- **请求快照**：body 截断至 64 KiB 存档，配合请求 ID（三级回退生成）串起同请求的多类日志；
- **IP 字段优先级按日志类型区分**（登录类优先 XFF、鉴权类优先 X-Real-IP），这是线上契约的一部分，对齐基准见 [binding-spec.md](../binding-spec.md)。

## 6. MFA：TOTP 绑定 → 登录挑战 → 发令牌

MFA 的完整通路分三段，全部围绕 `sys_user_mfa_factors` 表和三组 Redis 键：

### 6.1 绑定（个人中心）

`StartEnrollMethod`：生成 20 字节随机 secret（base32），返回 otpauth URL（`otpauth://totp/RushWindAdmin:uid:<n>?…&algorithm=SHA1&digits=6&period=30`）与 QR 图；secret 存 Redis `mfa:enroll:{operation_id}`，**TTL 300 秒**，确认前不落库。`ConfirmEnrollMethod`：校验操作归属（uid/tid 一致）+ 当前 TOTP 码，通过后写因子行（`status=ENABLED`），secret 以 `enc:` 前缀的 AES-256-GCM 落库——密钥来自 `RUSHWIND_CRYPTO_KEY`，未设置时明文存储（部署章节的必改项之一）。

### 6.2 挑战通路（登录侧）

登录链在口令验证通过后查启用中的 TOTP 因子（tenant+user+`Method=TOTP`+`Status=ENABLED`）：

```rust
if has_totp {
    let op_id = new_jwt_id();
    self.set_mfa_challenge(&op_id, &payload).await …;
    return Ok(LoginResponse {
        access_token: String::new(),
        expires_in: 0,
        mfa_operation_id: Some(op_id),   // ← 响应形状：不给令牌，给挑战 ID
        ..
    });
}
```

挑战行是 Redis `mfa:login:{op_id}`（300 秒 TTL），载荷是已通过口令验证的 token payload。客户端凭 `mfa_operation_id` + TOTP 码调 `VerifyMFAChallenge`（免鉴权白名单成员），成功即删挑战、写 `last_used_at`、发双令牌 + Cookie——**挑战行里的身份没有走第二因子前永远换不到令牌**。

### 6.3 校验端的防线

- **三振出局**：失败计数在 `mfa:loginfail:{op_id}`，3 次失败即销毁挑战（只能重新走口令登录）；
- **同码重放防线**：`mfa:usedcode:{uid}:{code}` SET NX EX 90——TOTP 在 ±skew 窗口内同一码值有效约 90 秒，SETNX 抢占失败即拒绝重放；
- **归因**：校验端点响应头 `X-Audit-Username` 携带挑战行里的用户名，审计中间件用它给登录日志归因（§3.1 的取值链第二级）；
- **管理端救援**：`DisableMfa` 带 `user_id`（≠ 本人）且仅平台管理员可用，按 credential_id 精确删或按方法清空该用户全部因子——认证器丢失时的救援路径；
- **本人解绑 step-up**：拆自己的第二因子必须携带当前有效 TOTP 码（会话被劫持者拿不到 secret，生成不了有效码——口令可能已泄露，TOTP 才是劫持者缺失的一环）。

## 7. 身份鉴别三件套（等保合规面）

| 要求 | 实现 | 调整入口 |
|---|---|---|
| 口令复杂度 | ≥8 位、小写/大写/数字/符号四类取三 | 平台参数（`sys_config`）热调 |
| 口令历史 | 默认近 3 条不可复用 | 同上 |
| 口令有效期 | 默认 90 天 | 同上 |
| 登录失败限流 | IP + 用户名双维度，5 次 / 15 分钟 | 代码常量（Redis Lua） |
| 图形验证码 | 渐进式：同一 ip+用户名失败达 `captcha_required_failures`（默认 3）后每次登录校验；答案 Redis 单次有效 | — |
| MFA | TOTP 绑定 + 登录挑战；管理员救援重置 | 个人中心 / 用户管理 |
| 口令保密 | 前端 AES 加密传输、bcrypt 存储、恒时假校验防枚举 | — |

### 7.1 每条规则的执行点

- **最小长度 + 四类取三**：`sys.password.minLen`（种子默认 8）与 `char_classes ≥ 3` 在两条路径强制执行——本人改密（`user_profile.rs::change_password`）与验证码重置（`authentication/mod.rs::reset_password_by_code`）。**管理员代改（`user.rs::edit_user_password`）当前只执行最小长度**，不查字符类与历史——知悉此差异再决定把"管理员代设初始密码"用在什么流程里；
- **口令历史**：`sys.password.historyCount`（种子默认 3），存在凭据行 `extra_info.password_history` JSON 数组里：新口令先对历史逐条 bcrypt 校验，命中即拒；通过后把当前 hash 追加进历史、裁剪到最近 N 条；
- **口令有效期**：`sys.password.maxAgeDays`（种子默认 90，≤0 关闭）在**登录链**的 `verify_credential` 末尾执行——凭据行 `updated_at` 距今超过 N 天即回 "password expired"，只对 `PASSWORD_HASH` 型凭据生效；
- **失败限流**：Redis Lua 原子自增（`ratelimit.rs`，5 次 / 15 分钟，键 `admin:login:fail:ip:{ip}` / `admin:login:fail:user:{username}`，登录成功清零、Redis 异常 fail-open）；
- **口令保密**：bcrypt 存储（DEFAULT_COST 12）；用户不存在/凭据禁用/库错误路径一律跑 `dummy_verify()`（对一个固定哑 hash 做一次真 bcrypt 校验）拉平响应时间，防用户名枚举。前端到后端是 rsa-v1 混合信封（RSA-2048 OAEP 包 AES-256-GCM 会话钥），兼容期接受 legacy AES-128-CBC 形——算法细节与换钥 runbook 见 [08 章](./08-deployment.md)。

等保 2.0 技术要求的完整对照表（含安全审计、访问控制、多租户隔离、数据保密性、备份恢复）见根 [README](../../README.md#安全与等保合规)；非软件范畴（管理制度、物理环境等）不在软件覆盖面内。

## 8. 留存与归档

- **库内留存是当前唯一生效面**：审计行只增不删（租户删除时的 wipe 除外），**代码里没有任何留存天数参数与超期清理作业**——根 README 的"默认库内留存 180 天"是产品层的规划口径，不是代码行为；容量规划时按"审计表无限增长"估算（或自建 DB 侧的定期清理）；
- 归档作业面已就位但**导出/清理是占位实现**：系统 cron（`audit_log_archive`，每日 03:30）按期投递任务，worker 收到后只记录 skip、不产出归档文件——"超期数据导出 JSONL 归档后清理"是规划项而非现状，配置留存策略时以此为准；
- 作业面的运行形态：两个静态 cron（`tenant_expiry_scan` 每小时整点、`audit_log_archive` 每日 03:30）+ 一个每分钟滴答的 `sys_tasks_periodic` 通配作业（重读 `sys_tasks` 里 ENABLE 的 PERIODIC 行、按各自 cron_spec 匹配后投递——控制台对任务的新增/启停即时生效，无需重启）。

## 9. 边界与已知取舍

- 数据访问日志的采集面（§3.5）尚未接线——表与读面就位，写入面是预留；
- 审计中间件对 JWT 只解码不验签：归因数据来自已过验签门内的请求，但**外层免鉴权路由的伪造 Bearer 也会被解码归因**——API 日志里匿名来源的 uid 字段以 0 出现，别把它当攻击面，把它当"未经门验证"的标记；
- 审计日志本身也是租户数据：平台视图看全部、租户视图只看自己的（Viewer 规则同[第 6 章](./06-multi-tenant-isolation.md)）；
- 审计写入与业务同事务语义的动作（如权限变更）在服务层落笔，保证"改了就有日志"；
- 打分启发式的输入只有请求事实（成败、uid、用户名、IP、UA）——不出设备指纹、不出地理库，风险因子里的 `INTERNAL_IP` 也只是 CIDR 判断。要接 GeoIP 得自己扩。

## 10. 本章小结

- 六类日志分工明确：登录管"谁进来了"，操作/API 管"做了什么"，数据访问管"碰了哪些数据"（当前未接线），权限/策略评估管"权力怎么变的"；
- 生产者只有两处代码 + 一个作业面：审计中间件（登录/API/操作/权限四类）、authz 评估器（策略评估）、任务系统（留存归档占位）；
- 风控打分、动作映射、请求快照、request_id 三级回退、trace 关联是排障的五件利器；IP 双优先级是审计（XFF 优先）与鉴权（X-Real-IP 优先）的契约差异；
- MFA 通路 = Redis 挑战行（5 分钟）+ 三振出局 + 同码重放防线；鉴别三件套阈值全部热调，但各条规则的执行点分布要按 §7.1 的清单对号；
- 动审计采集代码前，先跑打分与映射的既有单测（`risk.rs` / `audit.rs` 的 `#[cfg(test)]` 全套）。

下一步：[08 · 部署上线](./08-deployment.md)。
