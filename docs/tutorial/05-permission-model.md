# 05 · 权限模型

> 前置：[04 第一个服务模块](./04-first-service-module.md)。读完你会理解："谁能看到什么、调到什么"——从一张令牌到一次 403 的完整判定链。
>
> 权威规格指针：错误信封与 reason 表见 [binding-spec.md](../binding-spec.md) 与 [第 4 章 §3.3](./04-first-service-module.md)；租户闸门细节见 [第 6 章](./06-multi-tenant-isolation.md)。

## 1. 鉴权门：一次请求的四阶段

每个门控路由的请求都穿过 `crates/auth::auth_gate` 的四个阶段，任何一阶段失败即终止（认证阶段由框架 crate `rushwind-authn-gate` 承载，租户与授权两阶段在 `crates/auth` 组合）：

| 阶段 | 判什么 | 失败形态 |
|---|---|---|
| ① 认证 | JWT RS256 验签 + claims 解析 + Redis 会话检查（未被吊销/拉黑） | 401，两分支：`missing bearer token` / `access token expired` |
| ② 租户检查 | 租户存在、状态 ON、未到期只读、模块白名单 | 403（见[第 6 章](./06-multi-tenant-isolation.md)） |
| ③ 授权评估 | authz 引擎判定该操作者能否调该接口 | 403（`missing authz subject` 等） |
| ④ 放行注入 | claims 注入请求上下文，handler/仓储取用 | — |

门的主体（`crates/auth/src/lib.rs` 的 `auth_gate`，按 wire 顺序）：

```rust
let mut claims = match rushwind_authn_gate::authenticate_and_check_session(
    auth.as_ref(), checker.as_ref(), &headers, bearer.as_deref(),
).await {
    Ok(claims) => claims,
    Err(err) => return envelope::error_response(unauthorized(err)),   // 401，message 钉死
};
// …取 MatchedPath 路由模板、method、client_ip、trace_id…
if tenant_id > 0 {
    if let Err(message) = tenant_checker.check_tenant_access(tenant_id, &path, &method).await {
        return envelope::error_response(forbidden(&message));          // 403，check 自己的文案
    }
}
if let Err(message) = authorizer.authorize(evaluation).await {
    return envelope::error_response(forbidden(&message));              // 403
}
req.extensions_mut().insert(claims);                                  // ④ 注入
next.run(req).await
```

两个结构性事实：

- **公开面是白名单制**：只有 `apps/admin/src/auth_free.rs` 登记的 8 个端点免鉴权（登录、验证码、刷新、MFA 挑战、机器令牌交换、找回密码等），其余 283 条（291 条绑定减去白名单 8 条）全部门控——新路由默认门控，豁免必须显式登记并有守卫测试双向钉住；
- **绑定先于鉴权**：绑定层在门之外，畸形请求先吃 400（线上契约，见 [binding-spec.md](../binding-spec.md)）。

守卫测试 `apps/admin/tests/whitelist.rs` 两个方向都钉：白名单里的 (service, method) 对必须仍是路由绑定（漏了 proto 方法 = 静默扩大门控面），白名单之外的任何路由不得分类成公开（表打错字 = 静默暴露受保护端点），公开绑定计数也要吻合（`additional_bindings` 漂移抓得住）。

会话吊销是**服务端事实**：登出、管理端强制下线、改密后，Redis 白名单里的令牌即刻失效——客户端手里的 JWT 未过期也会 401。吊销的三个入口都落在 `TokenStore`（§2.3）：登出走 `revoke_user_token`（SCAN 删该 uid 全部 at/rt/us 行）、按会话下线走 `revoke_token_by_jti`（DEL 三键）、黑名单走 `bl:{jti}`。

## 2. 令牌与声明

### 2.1 签发与生命周期

RS256 双钥（`auth.yaml` 内嵌演示钥，生产必换，见[第 8 章](./08-deployment.md)）。时效分层：`auth.yaml` 的 `access_token_expires: 5400s`（1.5h）、`refresh_token_expires: 43200s`（12h）优先生效；两处都不配置时代码默认是 900s / 7 天（`config.rs`，环境变量 `RUSHWIND_ACCESS_TOKEN_EXPIRES_SECS` / `RUSHWIND_REFRESH_TOKEN_EXPIRES_SECS` 可覆盖）。

令牌对由 `token_issue.rs::issue_token_pair` 铸造：`to_access_claims` / `to_refresh_claims` 各造一份声明包，`jwt.create_identity` 分别签名，随后 `add_token_pair` 把两串令牌写进 Redis 白名单（`at:`/`rt:` 双键、各按自己的 TTL 过期）。**jti 是每对令牌的会话身份证**——UUIDv7 的 32 位十六进制连写（`new_jwt_id`），Redis 键、会话元组、吊销操作全按它寻址。

### 2.2 access 令牌的 claims 全表

claims 就是权限模型的数据载体。签发侧 `UserTokenPayload::to_access_claims` 逐字段落键（名字原样，`tid` 恒在、0 = 平台），消费侧 `from_claims` 反解：

| claim | 载荷字段 | 类型与形态 | 消费方 |
|---|---|---|---|
| `sub` | `username` | 字符串 | 审计归属、`operator_of` 的身份锚（缺它整个 payload 解不出） |
| `uid` | `user_id` | 数字或数字字符串 | 上下文、审计、会话管理（`us:` 键的 uid 段） |
| `tid` | `tenant_id` | 数字，缺省按 0（平台） | 租户谓词、租户闸门 |
| `iat` / `exp` / `jti` | — | 标准 JWT 注册声明 | 验签引擎、吊销寻址 |
| `roc` | `roles` | **字符串数组**（角色编码集） | authz 评估逐角色短路、`platform:admin` 判定 |
| `ouid` | `org_unit_id` | 数字 | 数据范围的"本单元"锚 |
| `ipa` / `ita` | `is_platform_admin` / `is_tenant_admin` | 布尔 | 闸门旁路语义（代租户资格在门里按 `platform:admin` 角色判定，见[第 6 章 §9](./06-multi-tenant-isolation.md)） |
| 数据范围三声明 | `ds` / `dss` / `dsu` | `ds` 单档字符串；`dss` 字符串数组（多角色聚合）；`dsu` **逗号拼接的十进制单元 id 串** | 数据范围的行过滤输入（[第 6 章 §5](./06-multi-tenant-isolation.md)） |
| `hfs` | `hidden_fields` | `资源.字段` 字符串数组 | 前端字段隐藏（§4） |
| `cid` / `did` | `client_id` / `device_id` | 字符串 | 登录策略、设备维度限制 |

读表须知（来自 `from_claims` 的容错面）：数字声明字符串形态也收（`"uid": "7"` 与 `"uid": 7` 等价）；`sub`/`uid` 缺失整个 payload 直接 `None`（`operator_of` 随即 401）；`tid` 缺失按平台 0。refresh 声明包是**精简形态**——只有 `uid`/`jti`/`iat`/`exp` 四键，没有 `sub`/`tid`：刷新身份以 uid 为锚，权限面在刷新时从 DB 重算（§2.4）。

### 2.3 Redis 会话面：键形与吊销

`TokenStore`（`apps/admin/src/admin/token.rs`）的四个键族，`(ct, uid, jti)` 三元寻址（ct = 客户端类型，REST 面 = 0）：

| 键形 | 内容 | TTL |
|---|---|---|
| `at:{ct}:{uid}:{jti}` | access 令牌原文——白名单精确比对 | access 时效（1.5h） |
| `rt:{ct}:{uid}:{jti}` | refresh 令牌原文 | refresh 时效（12h） |
| `us:{ct}:{uid}:{jti}` | `SessionMeta` JSON（username/tenantId/ip/ua/dev/loginAt）——在线会话清单的数据源 | refresh 时效 |
| `bl:{jti}` | 黑名单标记（EXISTS 即拒） | — |

认证阶段的会话检查是**白名单 + 黑名单双查**：`is_valid_access_token` 要求 Redis 里存的串与请求头里的串精确相等（改签发的令牌即刻不可用），`is_blocked_access_token` 对 `bl:{jti}` 做 EXISTS。键写入用 `SET ... EX` 语句形而非 SETEX 命令——mini-redis 测试面只认 get/publish/set/subscribe，两端语义等价。

refresh 的**单次有效**是一段 Lua 原子脚本（`verify_and_revoke_refresh_token`）：`GET rt:` 比对令牌 → 相等则 `DEL rt:/at:/us:` 三键一次完成——刷新与吊销在同一脚本里，并发重放第二个请求必然落空。

### 2.4 双 Cookie 续期面

refresh 走**双 Cookie 轮换**（`refresh_cookie_values`）：

```text
Set-Cookie: refresh_token=<jwt>; Path=/admin/v1/refresh-token; Max-Age=43200; HttpOnly; SameSite=Lax[; Secure]
Set-Cookie: refresh_exp=<unix 秒>;    Path=/;                     Max-Age=43200; SameSite=Lax[; Secure]
```

分工：`refresh_token` HttpOnly、路径钉死在刷新端点（浏览器脚本拿不到、别的请求也带不上它）；`refresh_exp` 是**前端可读**的到期时间戳——前端靠它提前静默续期，不用等 401。`Secure` 按部署 TLS 自适应（看 `x-forwarded-proto` 是否 https）。登出清对偶 Cookie（`Max-Age=-1`）。

刷新端点 `POST /admin/v1/refresh-token`（白名单成员）的轮换序：读 `refresh_token` Cookie → 验签 + 过期检查（`INCORRECT_REFRESH_TOKEN` 三连）→ **Lua 验证并吊销旧对** → 从 DB 重算权限面（角色可能已变——`payload_for_user` 重新聚合 roles/dss/dsu/hfs）→ 新 jti 铸新对 → 写新会话元组 → 下发新 Cookie 对。旧 access 未过期也随旧对一起吊销（Lua 删了 `at:` 键）。

## 3. 授权评估（authz）

引擎可插拔（`auth.yaml` 的 `authz.type`，当前 `noop`），装配在 `apps/admin/src/admin/authorizer.rs`。评估入口三件事：**判定、反查、留痕**。

### 3.1 判定：逐角色短路

```rust
let Some(subject) = evaluation.roles.first() else {
    return Err("missing authz subject".to_string());     // 403 文案
};
let allowed = true;                                       // noop 引擎放行每个 subject
```

按角色逐个评估、首允即短；无角色（`roc` 空）直接 403 `missing authz subject`。noop 引擎放行一切，所以"有任意角色"就能过授权阶段——**真正的细粒度约束在租户闸门与数据层**，角色权限当前主要作为权限点资产与前端可见性存在。

### 3.2 反查链：从路由模板到角色持权

`resolve_permission_policy` 把 `(角色编码, 路径模板, method)` 反查成 `(permission_id, policy_id)`，三跳全在 DB：

```text
sys_permission_apis（按 endpoint_path + endpoint_method 快照列）
  → permission_id
sys_roles（按 code 找角色行）
  → role_id
sys_role_permissions（role_id + permission_id 联查）
  → 持有 → (permission_id, 0)；任一跳落空 → (0, 0)
```

两个细节值得记住：

- **绑定按端点身份快照裁决，起跳就在 `sys_permission_apis`**：裁决键是 `(endpoint_path, endpoint_method)` 文本对（与租户路由闸查 `sys_apis` 的键同形），`api_id` 列仅历史兼容——上游 7eb7f9f0 同语义，免疫 `sys_apis` 行删除重建与 id 洗牌；`sys_apis` 本身只在租户闸门那一步查（[第 6 章](./06-multi-tenant-isolation.md)），反查链不经过它；
- **策略引用恒空**：本 schema 无策略表，`(permission_id, 0)` 的第二位永远是 0（trail 里的 `policy_id` 相应为 NULL）。

### 3.3 60s TTL 缓存与失效时机

反查结果按 `{subject}|{path}|{METHOD}` 键缓存在进程内 `HashMap`：

- **TTL 60s**（`RESOLVE_TTL`）：权限数据变更（改角色绑定）最多 60s 后反映到反查结果——**没有主动失效**，写侧不清缓存，纯粹靠 TTL 自然过期；
- **容量上限 1024**（`RESOLVE_CACHE_CAP`）：每次插入顺手 `retain` 掉过期条目；上限时的插入回收最快过期的条目——不同 subject 的长跑不会撑爆 map；
- 缓存只影响 trail 的 permission_id 引用，**不影响判定结果**（noop 引擎恒放行）。

### 3.4 留痕：sys_policy_evaluation_logs

每次评估落一行：tenant_id/user_id、permission_id/policy_id（反查命中才填）、request_path/request_method、result、effect_details（引擎名、subject、耗时 ms）、ip、trace_id（`traceparent` 的 trace id 或 `X-Request-Id`）、evaluation_context（JSON 上下文）。排障时按 trace_id 追每一次判定的输赢。**留痕写入失败不拦请求**（`let _ = insert`）——观测面永远不让位于可用性。

### 3.5 角色继承（父子角色）

`sys_roles.parent_id` 允许角色成树，约束全部在**编辑边界**（`services/role.rs`），运行时零开销：

1. `validate_parent_binding` 三查：父角色存在（否则 400 `parent role not found`）；父子同租户（否则 403 `parent role must belong to the same tenant`）；子角色的权限集 ⊆ 父角色已授予集合——越界按**数量**报错（403，`"{n} permission(s) exceed the parent role's granted permission set"`）；
2. `ensure_no_role_cycle`（Update 改父时）：从候选父沿 `parent_id` 上溯，回到自身即 403 `circular role hierarchy detected`；上溯深度上限 **MAX_DEPTH = 32** 防脏数据死循环；链断裂不算错（那是第 1 条的管辖）。

运行时授权不需要"展开"继承：写侧不变式保证了子角色自己的授权集已覆盖它可继承的一切，反查链照旧只看本角色绑定。保留角色码（`tenant:manager`、`platform:*`、`template:*` 前缀）禁止经服务层创建——只能由种子与租户开通等内部链路产生。

## 4. 菜单、按钮与字段：前端可见性

- **路由与菜单**：`sys_menus` 三类节点（目录/菜单/按钮），菜单行携带路由与组件路径，角色—菜单授权决定登录后看到的导航树；后端对菜单接口本身同样走门控——前端可见性是体验层，**真正的防线始终是接口门**；
- **权限码下发**：前端登录后拉两个面——`ListPermissionCode`（权限码集）与 `GetInitialContext`（菜单树 + 权限码 + hidden_fields），都出自 `services/admin_portal.rs`，直接从令牌 claims 与 DB 聚合；
- **按钮权限**：按钮节点的权限标识（如 `sys:user:create`）由前端按令牌权限集控制显隐；
- **字段级权限的两层**：
  - **动态层（hfs）**：角色绑定 `sys_role_field_permissions` 黑名单字段（resource + field 两列），登录时 `resolve_authority` 聚合成 `hfs` 声明（`"{resource}.{field}"` 串数组），经 `GetInitialContext` 下发——前端按它隐藏列/字段；
  - **静态层（redact.v1）**：契约注解驱动的响应脱敏（用户 email `keep_local_first: 2`、mobile `keep_first: 3 keep_last: 4`、`ListUserResponse.items` 的嵌套脱敏信封），编进 `redact_plan()`，每个响应序列化前先裁剪——golden 测试逐字节钉住形状（`"email":"zh******@example.com"`）。

两层同源不同职责：redact 是"人人都见掩码"的隐私面（不管角色），hfs 是"角色差异"的可见性面（管理员的工资列才可见）。

## 5. 身份鉴别的登录侧附加面

登录本身（免鉴权白名单成员）外面套了多层可叠加防线，全部可由平台参数与登录策略控制。`do_password` 的闸门顺序（`services/authentication/mod.rs`）值得完整走一遍——每道闸一个失败码，顺序即优先级：

```rust
// 0. 租户解析（tenant_code → 租户行，空 = 平台 0）
//    失败 → INVALID_TENANT（"invalid tenant"）
// 1. 限流预检：ratelimit::is_locked（只读，不计数）
//    命中 → TOO_MANY_ATTEMPTS（"too many login failures…"）
// 2. 渐进验证码闸：captcha_required() 达阈值才激活
//    缺头（id/value 全空）→ CAPTCHA_REQUIRED；给了但错/过期 → CAPTCHA_INVALID
// 3. 登录策略全局遍（IP CIDR/时间窗/设备），凭据校验前
//    命中 → FORBIDDEN（"login blocked by security policy"）
// 4. 凭据校验：AES 解密 → bcrypt（恒时假校验防用户枚举）
//    失败 → check_and_incr + incr_captcha_fail 计数，
//           USER_NOT_FOUND/USER_FREEZE/INVALID_PASSWORD 归一成 INVALID_PASSWORD
// 5. 用户复核（租户匹配）→ 登录策略用户遍
// 6. 权威解析：权限码须含 sys:access_backend，否则 FORBIDDEN "insufficient authority"
// 7. MFA 闸：TOTP 已启用 → 签发挑战（mfa_operation_id），不发令牌
// 8. 铸令牌对 + Redis 白名单 + 会话元组 + 双 Cookie
```

四道主力防线的机制对照（原表保留，代码锚补充）：

| 防线 | 机制 | 位置 |
|---|---|---|
| 图形验证码 | 渐进式：同一 ip+用户名失败达 `captcha_required_failures`（默认 3，`RUSHWIND_CAPTCHA_REQUIRED_FAILURES` 可调，**clamp 1..4**）才要求；6 位码（32 字符字母表去易混字形）、Redis `admin:captcha:{id}` 存答案、**TTL 600s**、verify-and-delete 单次有效；`x-captcha-id/-value` 头校验 | `apps/admin/src/admin/captcha.rs` |
| 登录限流 | Redis Lua：IP + 用户名双维度，5 次失败 / 15 分钟窗（锁定回 `TOO_MANY_ATTEMPTS`，1161）；Lua 原子 incr-if-not-locked，Redis 故障 fail-open（闸不住不算拒绝） | `apps/admin/src/admin/ratelimit.rs` |
| 登录策略 | CIDR / 时间窗 / 设备维度限制，命中即拒；凭据前后各跑一遍（全局遍 + 用户遍） | `apps/admin/src/admin/services/login_policy.rs` |
| MFA（TOTP） | 绑定后登录出挑战，验证通过才发令牌；挑战限时、连败锁定；管理员可救援重置 | `apps/admin/src/admin/services/mfa.rs` |
| 口令策略 | 复杂度四取三 / 历史复用 / 有效期，阈值走平台参数热调 | 鉴别三件套（[第 7 章](./07-audit-compliance.md)） |

### 5.1 渐进验证码：计数键与窗口细节

验证码的"渐进"由两把计数键驱动（与锁键同构，空维度不产生键——不做 match-all）：

```text
admin:login:captcha:ip:{ip}        admin:login:fail:ip:{ip}
admin:login:captcha:user:{user}    admin:login:fail:user:{user}
```

- **计数**：`incr_captcha_fail` 只在**真实凭证失败**上累加（captcha/policy 分支不计——不给验证码的请求推不满阈值）；窗口是滑动式——**每次失败都重置 TTL**（15 分钟，`15 * 60`），持续失败不清零；
- **判定**：`captcha_required` 纯读，任一维度 ≥ 阈值即要求验证码；Redis 故障 fail-open（不把正常人挡在无验证码快车道外，锁闸仍兜底）；
- **清零**：登录成功 `reset_captcha_fail` 写 0 值（SET+EX 而非 DEL——mini-redis 下 SETEX 是 no-op，写 0 才真正清掉旧计数）；
- **四个错误码的产生点**（全部在 `do_password` 闸门序里，reason → HTTP 码见 `admin_error.proto`）：`INVALID_TENANT`（租户解析，400）→ `TOO_MANY_ATTEMPTS`（限流预检，429/1161）→ `CAPTCHA_REQUIRED`（达阈值且两头全缺，400）→ `CAPTCHA_INVALID`（达阈值且头给错，400）。每个都有自己的 reason 而非共享 BAD_REQUEST——登录页按 reason 映射文案（issue #1 的修正：共享 BAD_REQUEST 会渲染成"参数错误"）。

限流键还有一个安全细节：P2 修复后**统一用解析后的 username 作限流键**——email/手机号/变体全落同一账号键，`admin` 被锁后换 `admin@victim.com` 等变体绕不过预检。

口令**永远不裸传**：前端按约定密钥做 AES 加密后传输，服务端解出后 bcrypt 哈希存储（含恒时假校验防用户枚举）。

## 6. 实操配方：加一个按钮权限的完整链路

以"给用户管理加一个『导出』按钮"为例，全链路五步（管理页操作或种子脚本均可）：

1. **菜单树挂按钮节点**：`sys_menus` 插一条 type=按钮 的节点，挂在用户管理菜单下，权限标识写 `sys:user:export`（前端按它控制按钮显隐）；
2. **权限点落表**：`sys_permissions` 建 `sys:user:export` 权限行（归属权限组）；
3. **权限—接口绑定**：`sys_permission_apis` 写绑定行——`endpoint_path = /admin/v1/users/export`、`endpoint_method = POST`（**快照列是裁决键**，与路由模板逐字一致）；
4. **角色绑定**：目标角色在 `sys_role_permissions` 绑上该权限；角色—菜单授权把按钮节点授给角色；
5. **前端生效**：下次登录（或刷新令牌——refresh 会重算权威）`ListPermissionCode` 返回的权限码集里出现 `sys:user:export`，按钮可见；接口侧 authz 反查链（§3.2）能从 `(path, method)` 反查到持有它的角色。

排障口诀：按钮不显示查第 4/5 步（角色绑定 + 重新登录）；接口 403 查第 3 步（端点快照拼写）与租户闸门（[第 6 章](./06-multi-tenant-isolation.md)的 Api 表注册）；60s 内改完权限不生效属正常（TTL 缓存）。

## 7. 本章小结

- 判定链四阶段，失败形态分型：认证 401（两分支文案钉死）、租户/授权 403（check 自己的文案）；
- 公开面是白名单制（8 条、守卫双向钉），会话吊销是服务端事实（白名单精确比对 + 黑名单 + Lua 单次刷新）；
- 令牌 claims 携带全部横向信息（角色数组、数据范围、字段黑名单），每层各取所需；数字声明双形态可解、refresh 包四键精简；
- authz 反查链三跳走 DB（端点快照起跳），60s TTL 自然过期、1024 容量回收，每次评估留痕不拦请求；
- 登录侧防线层层叠加、阈值热调（验证码阈值 clamp 1..4、窗口滑动续期）、四个登录错误码各有 reason；
- 角色继承的约束在编辑边界（子集校验 + 环防护 depth≤32），运行时反查零展开。

下一步：[06 · 多租户与行级隔离](./06-multi-tenant-isolation.md)。
