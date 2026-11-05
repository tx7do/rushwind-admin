# rushwind-admin 开发评估与计划

> 目标：以 Rust 复刻 `D:\GoProject\go-wind-admin`（下称「Go 后端」）的管理后台服务，
> **proto 为唯一 API 契约，三个前端（react / vue-element / vue-vben）零改动可对接两个后端**。
> 底座：`D:\RustProject\rushwind`（框架 monorepo）+ `D:\RustProject\rust-utils`（工具库）。
> 本文档基于对四个仓库的系统性调研（含 Go 侧中间件/存储/审计链路、前端传输层逐行核对、
> Rust 底座能力矩阵），结论与计划如下。

---

## 1. 硬约束（「前端零改动」的精确含义）

前端不做任何修改即可切换后端，等价于 Rust 后端必须在**线上字节层面**复刻以下行为。
这份清单是整个项目的验收基准，全部来自对前端传输层代码的逐行核对：

### 1.1 传输与头部

| # | 契约 | 出处（前端） |
|---|---|---|
| T1 | 所有 REST 路径相对形式 `admin/v1/...`（无前导 `/`，与 baseURL 拼接），约 197 条路由，含 `:exists`、`:with-admin`、`tasks:start` 等冒号后缀、`additional_bindings` 多路由、`dict/langs/batch` 等批量端点（注：早前调研引文中的 “dilt” 为转写错误，proto 与 Go 生成物实为 “dict”） | `src/api/generated/admin/service/v1/index.ts`（三端字节相同，10566 行） |
| T2 | `Authorization: Bearer <JWT>`（RS256 签名，见 §2.4） | `request-client.ts` 请求拦截器 |
| T3 | 每请求携带 `X-Request-ID`（uuid）、`X-Requested-With: XMLHttpRequest`；react/vue-element 另发 `Accept-Language: zh-CN\|en-US`；`Content-Type: application/json;charset=utf-8` | 同上 |
| T4 | `withCredentials: true`：refresh token 走 HttpOnly cookie | 同上 |
| T5 | 登录请求额外头 `X-Captcha-Id` / `X-Captcha-Value`（仅 `admin/v1/login`） | `captcha-headers.ts` |
| T6 | CORS：生产环境三前端与 API/SSE 为独立域名跨域直连，自定义头（含 `X-Captcha-*`）必须在 preflight 允许列表内，`Access-Control-Allow-Credentials: true`，白名单来源见 Go 侧 `configs/server.yaml` | `.env.production`、`scripts/deploy/nginx.conf` |

### 1.2 Cookie（refresh 会话）

Set-Cookie 精确属性（Go 侧 `authentication_service.go:74-111`）：

- `refresh_token=<JWT>`：`HttpOnly; Path=/admin/v1/refresh-token; Max-Age=<refreshExpiresInSeconds>; SameSite=Lax`，`Secure` 按实际传输层自适应（TLS 或 `X-Forwarded-Proto: https` 时加，明文 HTTP 省略）。
- `refresh_exp=<now+exp unix秒>`：非 HttpOnly、`Path=/`、其余同上（前端定时器与 bootstrap 静默恢复依赖它）。
- 登出/失效时对两个 cookie 按各自原 Path 发 `Max-Age=0` 清除。
- `POST /admin/v1/refresh-token`，body `{grant_type:"refresh_token"}`，cookie 鉴权；响应为标准 `LoginResponse`；该端点 401 必须可区分（前端据此强制登出，不做刷新重试）。

### 1.3 请求体格式

- B1 body 一律 protojson，**字段拼写以生成 TS 类型为准（混合命名）**：`grant_type`/`tenant_code`/`client_id` 为 snake_case，`captchaId`/`orderBy`/`pageSize`/`updateMask`/`newPassword` 为 camelCase——即 proto json_name，逐字段对齐，不得整体转 snake 或 camel。
- B2 密码字段为 AES-128-CBC/PKCS7 密文，key=iv=`VITE_AES_KEY` 的 utf8 字节（Rust 侧需对称解密）；例外：`reset-password-by-code` 的 `new_password` 当前是明文（对齐现状）。
- B3 List 类接口：分页/过滤全部在 URL query，参数名固定 `page,pageSize,noPaging,orderBy,query,fieldMask`（前端把 sorting/offset/limit/token/filter/filterExpr 显式置 undefined）；`orderBy` 是 JSON 数组字符串（默认 `["-created_at"]`）；`query` 是 go-crud 过滤语法的 JSON 字符串（`{"field__op": value}`，前端对字符串字段追写 `__contains`，别名表见前端 `pagination.ts:42-62`）；`fieldMask` 是逗号分隔列名。
- B4 文件上传 `POST/PUT admin/v1/file/upload`：multipart，固定字段 `file`（二进制）、`storageObject`（JSON 字符串 `{bucketName,fileDirectory}`）、`sourceFileName`、`mime`、`size`、`method`。
- B5 文件下载 `GET admin/v1/file/download`：query 携带 `fileId`、`storageObject.*`（点展平嵌套）、`preferPresignedUrl`、`presignExpireSeconds`、`disposition`、`acceptMime`、`rangeStart/rangeEnd`、`downloadUrl` 等（绑定器必须支持嵌套消息的点展平 query 绑定）。
- B6 头像 `POST admin/v1/me/avatar`：JSON `{imageBase64 | imageUrl}`（非 multipart）。

### 1.4 响应体格式

- R1 成功：直接返回资源 protojson；列表为 `{"items":[...],"total":<int>}`（total 为独立 COUNT）；写操作多为 `{}`；access-key 的 Create/ResetSecret 返回明文 secret。
- R2 登录/刷新响应：`{access_token, expires_in, token_type:"bearer", refresh_token?, refresh_expires_in?, mfa_operation_id?, id_token?, scope?}`；`mfa_operation_id` 非空 ⇔ `access_token` 为空串（MFA 闸门，配套 `POST /admin/v1/mfa/verify`）。
- R3 验证码响应：`{captchaId, imageBase64}`（base64 图片，一次性消费）。
- R4 错误：HTTP 状态 + body 为 **Kratos `Status` 消息的 protojson**：`{"code":<int32>,"reason":<string>,"message":<string>,"metadata":<map>}`（`kratos/v2@v2.9.2/errors/errors.pb.go:29-32` 已核实字段序与命名）。HTTP 状态由 `*_error.proto` 中每个 reason 的 `(errors.code)` 注解决定（含 `599` 等非常规码）。前端只消费 `reason`（映射本地 i18n 文案）与 HTTP 状态（401 触发刷新/重放队列），因此 **reason 枚举值与状态码必须逐一对齐，`code` 数值同样按 Go 侧行为输出**。

### 1.5 SSE（独立端口 7789）

- `GET /events?stream=<userId>`，头 `Authorization: Bearer` + `Accept: text/event-stream`；`stream` 必须等于 token 所属 userId，否则拒绝。
- 事件仅一种：`event: notification`，data 为 `InternalMessageRecipient` 的 protojson。长连接、无缓冲（网关侧 `proxy_buffering off`、24h 读超时）。

### 1.6 明确不存在的（避免过度实现）

无 WebSocket、无长轮询、无 csv/excel 导出端点、前端不调 `/health` `/metrics`、无 `X-Tenant-Id` 头（租户只走登录 body `tenant_code`）、无 gRPC 通道（Go 侧生成了代码但从未注册 gRPC server）。UI 文案 i18n 全部前端本地打包，后端仅提供语言**元数据**与字典条目（含 i18n map）——切换语言不发请求。

---

## 2. 源系统解剖（Go 后端）

### 2.1 部署拓扑

| 组件 | 说明 |
|---|---|
| `admin-service` 容器 | REST `:7788`（Swagger `/docs`、pprof 当前开启）、SSE `:7789` path `/events` |
| `postgres` | 生产库（`configs/data.yaml` driver=postgres，ent 自动迁移 `migrate:true`）；测试用 SQLite |
| `redis` | 令牌/会话/验证码/登录限流/脚本热重载发布订阅 |
| `minio` | 对象存储（bucket `images`），预签名 URL + HMAC 签名图片代理 |
| SMTP | 直连外部（无容器），找回密码验证码邮件 |

### 2.2 规模

- proto：112 个文件、16 个模块；rpc 约 456 个（源领域层 ~260 + BFF 层 196），带 `google.api.http` 注解 **197 个**（BFF 全覆盖 + `storage/file_transfer` 1 个）。
- 两层结构：源领域层 `<module>/service/v1/*.proto`（message + gRPC Service，无 http 注解）；BFF 展示层 `admin/service/v1/i_*.proto`（41 个 service 文件，import 源领域 message，逐 rpc 带 http 注解 + 少量 `gnostic.openapi.v3.operation security:{}` 免鉴权标注 + `redact.method_skip` 5 处）。
- 注解密度：PGV 校验 13 处（5 个文件）、redact 9 处（identity/user 字段级 3 + admin/i_user method_skip 5 + authentication 1）。
- 手写 Go 代码 ~63.9k 行：`pkg/` 17 个横切包 20.3k，`app/admin/service` 非 ent 部分 43.4k（其中 service 层 10.5k、repo+基础设施 22.2k、GORM 备用后端 9.1k）。另有 ent 生成 340.9k、api/gen 生成 160.6k（复刻中由等价生成器替代，不手写）。
- 存储实体 50 个（表名映射见 §7 模块清单），约 30 个带 `TenantID` mixin；树形：menu/org_unit（`Tree`+`TreePath` 物化路径，org_unit）、permission_group（`Tree`）；**无软删除**（物理 DELETE）；复合唯一索引（如 tenant_id+username、tenant_id+parent_id+name）依赖 Atlas 迁移建出。

### 2.3 代码生成链（`backend/api/buf*.yaml`）

Go 侧：`protoc-gen-go`、`go-grpc`（未用）、**`protoc-gen-go-http`（路由 + 参数绑定 + operation id 生成器）**、`go-errors`（reason→HTTP 状态错误构造器）、`validate`（PGV）、`go-redact`（响应脱敏包装器）；OpenAPI v3 资产（merged、enum_type=string）；三前端 TS 客户端（`protoc-gen-typescript-http`，三份输出字节相同）。
`make register`（`tools/register`）向 `wiring_ent.go` / `rest_server.go` 的 5 个锚点注入新模块登记行。`.zcode/skills/add-crud-module` 记录了标准模块九步模板（源 proto → BFF proto → make api → ent schema → repo → service → register）。

### 2.4 鉴权与数据访问链（复刻最高风险区）

```
JWT(RS256, claims: uid/tid/cid/did/roc/ds|dss+dsu/hfs/ouid/ipa/ita/jti)
  → pkg/middleware/auth: 提取+验签+Redis 在库校验(at:*/rt:*)+黑名单(bl:*)
  → UserTokenPayload 注入 ctx（双通道：viewer + x-md-global-operator）
  → 租户门 CheckTenantAccess：sys_tenants 状态/到期/READONLY 策略
      + (pathTemplate,method)→sys_apis.business_module × 租户套餐 plan_modules 白名单
  → authz 引擎（当前配置 noop；支持 casbin/opa）+ 每次判定写 sys_policy_evaluation_logs
  → ent privacy：TenantPrivacy/TenantMutationGuard(Update|Delete 补齐) + DataScopeGuard
      （viewer 数据范围 ALL/SELF/UNIT_ONLY/UNIT_AND_CHILD/SELECTED_UNITS → SQL 谓词；空集 fail-closed）
  → 字段权限：JWT hfs（sys_role_field_permissions 聚合）→ 读路径 ApplyReadMask 清值、
      写路径 StripWriteFields 清值+剔除 update_mask 路径（仅 user/authentication 两模块包装）
  → 静态脱敏：redact 注解生成的响应包装器（email keep_local_first、mask keep_first/keep_last、element nested）
```

### 2.5 审计管道（跨四层的隐式链路）

`pkg/middleware/logging`（pre-handler 64KB body 快照重放、accumulator/sink 防递归）→ 五个子中间件（api/login/operation/permission/data_access）→ repo 回调落五张表；
SQL 级事件由 `audit_driver_wrapper`（包装 ent 的 sql.Driver）产出：`MaskSQL` 字面量脱敏（串/数值→`***`）+ `ExtractTables` 表名/数据分类提取 + 时延/行数；
GeoIP（mmdb）与 UA 解析用于登录审计；`asynq` cron `30 3 * * *` 归档超 180 天审计为 JSONL 后删除。

### 2.6 任务、脚本、事件

- 任务：asynq（Redis，队列 critical:10/default:5/low:1）。表驱动：`sys_tasks`（type=PERIODIC、cron_spec）→ `StartAllTask`/`RestartAllTask`（remove-then-rebuild）；系统任务：租户到期扫描（`0 * * * *`）、审计归档、站内信 fan-out（幂等靠 (message_id,recipient) 唯一约束 + ON CONFLICT DO NOTHING）；脚本任务统一 `script_task` 单分发类型（payload 携带 handler 名）；payload AES-256-GCM 加密（`GOWIND_CRYPTO_KEY`）。
- 脚本：gopher-lua + goja 双引擎，单 VM 全局串行（execMu）、VMTimeout 5s、MaxMemory 50MB、MaxVMs 10；源=DB(sys_scripts)/文件 + **Redis pubsub 热重载**；实体 before/after 钩子（before 同步可否决、after 异步旁路）；VM 模块：log/crypto/util/cache/eventbus/hook/task/oss；HTTP 出站白名单 `SCRIPT_HTTP_ALLOWED_DOMAINS`。
- 事件总线：**进程内**（handler map + RWMutex；同步/异步/一次性三模式）——不是 Redis。

---

## 3. Rust 底座能力映射

| Go 侧组件 | Rust 对应物 | 状态 |
|---|---|---|
| Kratos HTTP server / 路由 | axum（`rushwind-transport-axum`，任意 Router 接入生命周期） | ✅ 可用 |
| recovery/request-id/logging/CORS/timeout 中间件 | `rushwind-http` `HttpEdge`（固定次序组装） | ✅ 可用（见 §4-D1 信封差异） |
| JWT RS256 签发/验证（AuthClaims 任意 claims） | `rushwind-authn-jwt`（12 算法含 RS256，claims=任意 JSON Map）+ `rust-utils jwt` | ✅ 可用 |
| Bearer 提取 + 白名单 | `rushwind-http` `with_authn`（per-subtree 装配，白名单=public 子树不包再 merge）+ `Authenticated` 提取器 | ✅ 可用 |
| casbin/opa RBAC（(role,path,method) 策略 + 热更 `ResetPolicies`） | `rushwind-authz-rbac`（role→permission 模式匹配带通配符、`set_policies` 热更、角色继承）+ `with_authorization` 包装器 | ✅ 可用（判定日志用装饰器包一层，对位 `policy_eval_logging_engine`） |
| ent 隐私（租户/数据范围谓词） | `rushwind-storage` `Viewer/DataRange`（All/Unit/User/Own/None → scope() 谓词） | ⚠️ 部分：无租户轴 → admin 侧建 `TenantScopeRepo` 装饰器统一注入（对位 TenantMutationGuard + DataScopeGuard，含 UNIT_AND_CHILD/SELECTED_UNITS 的树展开，在应用层预展开成单元 ID 集合后走 `Any(units)` 谓词） |
| ent 动态 ORM（50 表、PostgreSQL） | `rushwind-storage-seaorm`（**动态 Schema/sea_query，无 entity 生成物**，pg/mysql/sqlite 三方言，一致性套件钉死） | ✅ 可用（列模型见 §4-D4） |
| Entity↔DTO（CopierMapper + 时间/枚举转换器） | `rushwind-storage-macros` `ToRecord/FromRecord`（`rename`/`as_text`(枚举)/`with`(自定义转换模块，用于 Timestamp↔unix-ms Int)） | ⚠️ 需逐模块编写映射与转换模块 |
| 软删除 | Go 侧本来就物理删除；`SoftDeleteRepo` 不需要 | ➖ 不适用 |
| 树形表（org_unit/menu/permission_group） | `rushwind-storage-tree`（`Tree::new(&dyn Repository)`：children/roots/ancestors/subtree，BFS 逐层查询 + 环守卫） | ✅ 可用 |
| go-crud 分页/过滤语法（`query` JSON、`orderBy` 数组） | `rust-utils query_parser`（25 算子 Django 风格解析，与 go-crud 同源对位）+ `rushwind-storage` `FilterExpr`（17 算子）映射 | ⚠️ 需建算子矩阵（§4-D3） |
| `PagingRequest`（旧形状平铺 oneof）绑定 | 生成 TS 客户端把它拆成 query 参数 → Rust 绑定器按 json_name 组装 → pbjson 反序列化为 prost 类型 → admin 层翻译成 `ListQuery` | 🔨 需建（绑定器 + 翻译层） |
| 验证码（字符串图形码、Redis、一次性） | `rust-utils captcha`（Digit/String/Math/Chinese 驱动，`RedisStore`（`captcha-redis` feature，键名与 Go 一致），`include_bytes!` 内嵌 CJK 位图资产） | ✅ 可用（base64 形态需差分对齐） |
| bcrypt 口令哈希 / 恒定时间防枚举 | `rust-utils password`（`BCryptCrypto`） | ✅ 可用 |
| AES-CBC（登录密码解密）/ AES-GCM（任务 payload） | `rust-utils crypto`（`AesCipher` CBC+PKCS#7 / `AesGcmCipher`） | ✅ 可用 |
| Redis（令牌在库/会话/限流/脚本热重载） | `rushwind-cache-redis`（含 TTL、pipeline、key 前缀）+ `rushwind-broker-redis`（pub/sub） | ✅ 可用 |
| MinIO（预签名、bucket、对象流） | `rushwind-oss-s3`（rusty-s3 SigV4 + reqwest，path/virtual-host 寻址） | ✅ 可用 |
| 进程内事件总线（脚本 API + 实体钩子） | Go `pkg/eventbus` 直接移植为 admin 内库（Rust 侧无现成对应物；rushwind broker 是跨进程的，语义不同） | 🔨 需移植 |
| Lua/JS 脚本（沙盒、配额、热重载、DB/文件源） | `rushwind-script-lua`（mlua 白名单沙盒 + 指令配额 + wall-clock + 源热重载 watch）/ `rushwind-script-javascript` | ⚠️ VM 业务模块（cache/eventbus/hook/task/oss/crypto/log/util）与实体钩子桥接需 admin 侧编写 |
| 任务队列（asynq/Redis，cron 表驱动） | `rushwind-apalis-postgres`（仅 PG）；apalis 上游有 redis 后端 | 🔨 需建 `rushwind-apalis-redis` 适配 crate（见 §4-D5） |
| SMTP 邮件（SSL/STARTTLS/NONE） | `lettre`（三模式对位） | 🔨 需建 admin 邮件模块 |
| GeoIP / UA 解析（登录审计） | `rust-utils geoip`（mmdb 后端；数据文件外部加载）+ UA 解析需选型（`uaparser` 或最小自研，对位 mileusna/useragent 的设备四元组） | ⚠️ 数据文件与解析字段需对齐 |
| TOTP MFA | 无现成；`totp-lite`/自研（HMAC-SHA1/SHA256 + RFC 6238，base32 密钥） | 🔨 需建 |
| 雪花 ID（如启用） | `rust-utils id`（位布局对齐 bwmarrin/snowflake） | ✅ 可用（实体主键为自增，大概率不用） |
| HTTP 边缘错误信封 | `rushwind-http` `HttpError`：`{code:<gRPC码名字符串>,reason,message,details?}` | ❌ 与 Kratos `{code:<int>,reason,message,metadata}` 不符 → §4-D1 |
| proto→路由生成器 | 无 | 🔨 本项目最大自建件（§4-D2） |
| DDL/迁移（ent Atlas 自动迁移 + 索引） | `SeaRepo::migrate_create()` 仅 `CREATE TABLE IF NOT EXISTS`、**无索引/约束** | 🔨 §4-D4 |
| 配置驱动装配 | `rushwind-bootstrap`（YAML→存储+服务器+路由包） | ⚠️ 不含边缘中间件/鉴权装配——admin 采用自写装配器（对位 Go `wiring_ent.go` 的分层与 LIFO cleanup），bootstrap 仅作参考或后期迁移 |
| OpenAPI/Swagger `/docs` | `protoc-gen-openapi` 产物可直接复用（同一生成器） | 可选（最后阶段） |

`rushwind-encoding-proto`（prost+pbjson 的 protojson 通道，build.rs 用 protox 编译）为类型生成提供了成熟范式，但**只对其自家 query.proto 落地**——本项目需把该 build 模式推广到全部 112 个 proto（admin 专用 crate，见 §6）。

**无运行时 proto 反射**：全仓无 prost-reflect 消息构造；本计划走「prost+pbjson 编译期类型 + 描述符驱动的路由/绑定代码生成」，与 Go 侧同构（Go 也是编译期生成绑定代码），不需要 DynamicMessage。

---

## 4. 关键差距与决策

| # | 决策 | 内容与理由 |
|---|---|---|
| D1 | **Kratos 兼容错误信封在 admin 侧实现** | rushwind-http 的 `ErrorEnvelope`（code 为字符串、字段名 details）与前端契约不符；直接改框架会破坏其对位 Go 前作的语义。→ admin 内建 `KratosStatus` 信封（code=int32、reason、message、metadata map），状态码与 reason 绑定关系由生成器从 `*_error.proto` 的 `(errors.code)` 注解产出静态表；后续可作为 `rushwind-http` 的可选变体回馈上游。中间件栈（recovery/request-id/logging/CORS/timeout）继续用 rushwind-http。 |
| D2 | **自建 `protoc-gen-rust-http` 生成器** | 路由 + 绑定 + 服务 trait + 错误表 + 脱敏规则表 + operation id + sys_apis 同步表，全部从 descriptor 生成（Rust 写的 protoc 插件，buf `local:` 调用，或 build.rs 内进程调用）。栈：protox 编译 → FileDescriptorSet → prost-reflect 解析扩展（google.api.http、errors.code、redact、PGV——这些扩展声明文件由 buf.lock 钉定的 BSR 依赖模块解析，不落仓）→ 生成代码。**绑定语义逐条对位 `protoc-gen-go-http` 生成物**（路径模板变量、query 按 json_name（含嵌套消息点展平）与 oneof 分支、body `*` 全量 protojson、响应 protojson + 空消息 `{}`），差分金样逐端点钉死。这是把 197 条路由的接线成本从「手写失控」压回「一跑生成」的核心。 |
| D3 | **分页算子矩阵** | 前端实际发出的算子集 = 前端 `pagination.ts:42-62` 别名表 ∩ go-crud 25 算子；rushwind `FilterExpr` 支持 17 个关系算子。Phase 0 产出三列矩阵（前端别名 → go-crud 算子 → rushwind 算子），逐一标注 直映/折ILIKE/不支持（不支持者在差分测试中定为「与 Go 同样报错」）。字段名映射表需同时收录 proto 原名与 json_name（对位 Go `fieldperm.ParseTokenEntries` 的双拼写处理）。 |
| D4 | **DDL 以 Go 迁移产物为黄金** | 空库上跑 Go 后端自动迁移 → `pg_dump --schema-only` 得到黄金 DDL（含全部索引/唯一约束/默认值）→ 按模块切分为 SQL 迁移文件，admin 启动时应用（幂等）。`migrate_create()` 因无索引不用。CI 用同一份 DDL + SQLite 方言子集跑单测（对位 Go 的 sqlite_compat_test）。种子数据：`postgresql-demo-data.sql` 两边同灌；空库引导种子（`pkg/constants/default_data.go`）移植为 Rust 常量表。 |
| D5 | **任务队列：新建 `rushwind-apalis-redis`** | 对位 asynq（Redis 后端、多队列优先级、cron 表驱动、任务 payload AES-GCM）。若适配器受阻，退路为进程内 cron 调度器（仅覆盖系统任务：租户到期扫描、审计归档、站内信 fan-out、脚本任务）——前端可观测面只是 sys_tasks 的 CRUD 与 start/stop/restart，队列内部语义不直接进契约。 |
| D6 | **seaorm 适配器加审计钩子** | `rushwind-storage-seaorm` 增加可选「语句渲染回调」（sea_query Statement 构建完成处调用），admin 的 Auditor 实现消费之：移植 `MaskSQL`（字面量脱敏）+ `ExtractTables`（表名/数据分类）→ `AuditEvent` → `sys_data_access_audit_logs`。框架改动面小（一处 hook 点），语义与 Go 的 driver 包装器对位。 |
| D7 | **`TenantScopeRepo` 装饰器** | admin 内建 `Arc<dyn Repository>` 装饰器：从 `QueryCtx.viewer`（由 `Authenticated` claims 构造）+ 租户 claim 生成强制谓词（tenant_id 等值 + 数据范围谓词），注入所有 list/count/update/delete/create；空范围 fail-closed。对位 ent privacy 的不可绕过性靠「仓储只能经该装饰器拿到」的模块封装保证。 |
| D8 | **脚本与实体钩子** | 引擎用 rushwind-script-lua/javascript；业务模块桥接 + 出站白名单 + 单 VM 串行 + 配额在 admin 脚本宿主实现；热重载经 broker-redis；实体钩子经移植的进程内 eventbus（before 同步否决 / after 异步）。 |
| D9 | **邮件** | lettre（SSL/STARTTLS/明文三模式、PLAIN 认证），仅用于找回密码验证码与 notification_channel 邮箱渠道。 |
| D10 | **不做** | gRPC server、WebSocket、csv 导出、pprof、zanzibar 鉴权、GORM 备用后端（`gorm_backend`）、OAuth 线上端点（proto 有但 BFF 未暴露）、Swagger UI（可选尾项）。 |

---

## 5. 合规性验收基准（贯穿所有阶段）

1. **差分测试**：同一套 Postgres/Redis/MinIO（隔离实例/库），Go 与 Rust 两后端同种子、同请求回放；比较器做 protojson 感知归一（解析为 JSON 树语义比较；map 无序、数组有序；`id`/时间戳归一；其余必须相等）+ HTTP 状态 + `reason` + 对前端有语义的响应头（Set-Cookie 属性、Content-Type）。
2. **权限矩阵**：种子用户 × 角色（平台管理员/租户管理员/普通/自定义范围）× 租户（正常/到期/READONLY/套餐受限）× 全部 197 路由 → 期望 (status, reason) 表（复用 go 仓 `.zcode/tmp/matrix_*` 脚本思路产品化）。**矩阵先行**：每模块合并前必须先有该模块的矩阵用例。
3. **浏览器 E2E**：三前端（react 走 vite 代理；vue-element/vue-vben 直连 7788/7789）对 Rust 后端跑页面冒烟 + 关键流（登录含验证码、401 刷新重放、SSE 通知、文件上传下载、树表、批量操作）。
4. **金样**：错误信封（每 reason 一条）、cookie 属性、分页/排序/过滤语法（算子矩阵全量）、protojson 序列化边缘（枚举字符串、int64 字符串化、bytes base64、map、oneof、FieldMask `{"paths":[...]}`、空消息 `{}`）。
5. **CI**：proto 同步 checksum 门（与 go 仓 `api/protos` 比对，漂移即红）、cargo fmt/clippy/test、差分子集、SQL 迁移与黄金 DDL 的 diff 门。

---

## 6. 目标架构（rushwind-admin workspace，布局对位原版仓：backend/ + frontend/ + docs/）

```
rushwind-admin/
├── backend/                  # Cargo workspace 根（对位 go 仓 backend/）
│   ├── Cargo.toml            # members: crates/* + app/admin/service + pkg/*
│   ├── api/
│   │   ├── protos/           # 从 go 仓同步（sync 脚本 + checksum 门）
│   │   ├── buf.lock          # buf 依赖 commit 钉定：google/api、pagination、redact、validate、gnostic…
│   │   │                     # （与上游 go-wind-admin 的 buf.lock 同源，再同步时比对 commit）
│   │   └── sync-protos.sh    # 契约同步与校验
│   ├── app/admin/service/    # 对位 go 仓 app/admin/service/（服务 crate）
│   │   ├── cmd/server/main.rs        # 入口（JWT 引擎 + 生命周期；对位 cmd/server/main.go）
│   │   ├── internal/server/rest_server.rs   # 装配（挂载 + 逐路由层组合 + HttpEdge；对位 internal/server/rest_server.go）
│   │   └── configs/          # 运行时配置面（对位 configs/）
│   ├── pkg/                  # 对位 go 仓 pkg/（共享包，独立 crate）
│   │   └── middleware-auth/  # 鉴权门（对位 pkg/middleware/auth）
│   ├── api/
│   │   ├── protos|buf.lock|MANIFEST  # 契约树（sync-protos 同步 + checksum 门）+ 依赖钉定
│   │   └── admin-api/        # 契约 crate（对位 go 仓 api/gen 生成绑定落点）：build.rs 一体产出
│   │                         #   prost+pbjson 类型（well-known 外部化）+ protoc 注解闭包
│   │                         #   descriptor/pool + rushwind-gen-http 生成面（路由表/绑定计划/
│   │                         #   服务 trait/错误表/mount 双路由发射；AUTH_FREE 免鉴权表单源
│   │                         #   src/auth_free.rs，lib 与 build.rs 共include）
│   ├── app/admin/service/    # 服务实现（internal/service/*，null 桩逐个替换中）
│   ├── pkg/middleware-auth/  # 鉴权门（对位 pkg/middleware/auth）
│   └── testbed/
│       ├── admin-diff/       # 差分回放器 crate（比较器/矩阵 runner/语料；与台架同址）
│       └── corpus|exemptions|compose # 差分台架（豁免集 + 精选语料 + 报告）
│   └── migrations/           # 黄金 DDL 切片 + 种子（后端启动应用）
├── frontend/                 # 结构占位（前端零改动复用 go 仓三个前端，不复制代码）
└── docs/                     # development-plan、binding-spec、operator-matrix …
```

框架仓（D:\RustProject\rushwind）新增通用机制（2026-09-14 起）：
`rushwind-http-binding`（form 绑定器 / CT 解析 / 预绑定层 bindgate / 四字段信封——描述符池参数化，无语料耦合）、
`rushwind-http::cors_compat`（gorilla/handlers@v1.5.2 CORS 一比一移植）、
`HttpEdge::with_cors_compat`（兼容层选择）、
`CorsOptions::without_allow_methods/headers`（tower-http 未设置语义开关）。

请求生命周期（对位 Go，2026-09-14 现状）：

```
axum(生成路由, 逐路由层组合: bindgate 最外 + gate(仅门控路由) 内层)
  → bindgate(rushwind-http-binding): CT 解析→json/form body 绑定→query 绑定→BoundMessage 扩展
  → gate(pkg/middleware-auth): JWT RS256 验签, 失败→401/UNAUTHORIZED 信封, 成功→claims 扩展
  → glue(rushwind-http-binding): 取 BoundMessage→路径变量绑定→静态转换→服务 trait 调用
  → 服务实现(当前全部 null 桩: 500/Unknown)→EmitUnpopulated 序列化→响应
  外层: HttpEdge(recovery→request-id→logging→cors_compat(gorilla 规则)→timeout)
  [待接] authz-rbac(sys_apis 策略)/access-token checker(会话吊销)/TenantScope/审计/字段权限/真实服务
```

---

## 7. 分阶段计划

体量口径：Go 手写 63.9k 行的等价复刻，其中约 60% 可由生成器/底座吃掉，剩余为模块实现与安全语义对齐。以下「里程碑」按单人全职 + AI 辅助的相对节奏估算（M=里程碑，1M≈1–2 周）。

### Phase 0 — 地基与生成器（约 4–6 M）★ 决定全局成败

- [ ] 仓库脚手架：workspace、CI、`api/protos` 同步脚本 + checksum 门、third_party vendor。
- [ ] `admin-api`：protox→prost+pbjson 全量类型（含 well-known），protojson 序列化边缘金样（枚举/整型字符串化/bytes/map/oneof/FieldMask/空消息）。
- [ ] `protoc-gen-rust-http` v1：路由表 + 绑定计划 + service trait + 错误构造器 + operation id + 免鉴权表 + sys_apis 同步行；redact/PGV 规则表 v2 再加。
- [ ] 绑定器运行时 + KratosStatus 信封 + CORS/超时/请求ID 装配（对照 Go `CreateRestServer` 的启用开关集）。
- [ ] 黄金 DDL 流水线：Go 空库迁移 → pg_dump → 切片 → 启动应用；种子数据装载。
- [ ] 差分测试基建 v0：双后端起停、请求回放、归一比较器、auth 辅助（先覆盖 language 模块作样板）。
- [ ] 算子矩阵文档 + 矩阵测试骨架。
- **验收**：language 模块（6 端点，最简单 CRUD）在差分与浏览器双绿灯；空库自动建表+种子可引导。

### Phase 1 — 认证与会话内核（约 3–4 M）

- [ ] 登录链：验证码（rust-utils captcha + RedisStore，base64 形态对齐）→ AES-CBC 密码解密 → bcrypt + dummy-hash 恒定时间 → 失败归一 INVALID_PASSWORD（防枚举）→ 登录策略/限流（cache-redis）。
- [ ] 令牌：RS256 签发（claims 键名 uid/tid/cid/did/roc/ds|dss+dsu/hfs/ouid/ipa/ita/jti 逐键对位）、`at:/rt:/us:/bl:` Redis 键族、吊销 Lua 原子脚本、refresh cookie 三态（设/清/TLS 自适应）。
- [ ] refresh-token 端点（401 语义区分）、logout、MFA（TOTP + 挑战缓存 + mfa_operation_id 闸门）、forgot/reset-by-code（明文口例外 + SMTP lettre）。
- [ ] online_session 四端点（键族列举/自助/强踢）。
- **验收**：三前端完整登录/登出/刷新/401 重放队列 E2E；矩阵：未登录全路由 401、登录后免鉴权白名单 8 端点行为一致；cookie 属性金样。

### Phase 2 — 鉴权/数据访问横切（约 3–4 M）

- [ ] `with_authn`/`with_authorization` 全量接线（生成路由表驱动）；RBAC 策略装载（sys_roles+sys_apis→set_policies，资源=path 模板、动作=method）+ 判定日志装饰器（sys_policy_evaluation_logs）。
- [ ] 租户门（sys_apis.business_module × plan_modules × 租户状态/READONLY 只读放行 GET/HEAD/OPTIONS）。
- [ ] `TenantScopeRepo`（tenant 谓词 + viewer 数据范围谓词 + 树单元展开 + 空集 fail-closed）。
- [ ] fieldperm（hfs 读清/写剥离 + update_mask 路径剔除）与 redact 静态脱敏（仅 user/authentication 模块启用，规则表来自生成器）。
- [ ] 审计中间件五件套（64KB 快照、GeoIP/UA、accumulator/sink 防递归）+ seaorm 审计钩子 + MaskSQL/ExtractTables 移植。
- **验收**：权限/租户/数据范围三维矩阵（平台管理员/租户管理员/普通/自定义范围 × 正常/到期/只读/套餐受限 × 197 路由）对 Go 全量一致；五张审计表行内容（脱敏文本、表名、时延、行数）字段级对齐。

### Phase 3 — CRUD 模块流水线（约 8–12 M，最长尾）

按依赖分五批，每模块 = schema/迁移 + repo 映射（Timestamp/枚举转换器）+ trait 实现 + 注册 + 差分/矩阵/页面冒烟三绿：

| 批 | 模块（端点数） | 特殊点 |
|---|---|---|
| A 平台层 ~56 | language(6)、dict_type(5)、dict_entry(5, i18n map→子表)、config(5)、api(7, OpenAPI 自同步)、menu(6, tree)、permission(6)、permission_group(5, tree)、role(5, +role_metadata)、notification_channel(6) | api 模块的 sys_apis 同步改由生成路由表驱动（等价替换 kin-openapi 解析） |
| B 身份 ~47 | user(7, redact+fieldperm 双启用)、user_profile(7, 含 me/*)、org_unit(5, tree+物化路径+数据范围)、position(5)、tenant(9)、plan(5)/plan_module(5)/plan_quota(4) | user 模块是字段权限/脱敏/掩码三者交织的最险处，矩阵最厚 |
| C 审计只读 ~10 | api/login/operation/permission/data_access 审计列表(2×4+2)、policy_evaluation_log(2) | 只读 + 分页 + 字段映射 |
| D 运维 ~34 | task(10, 含 start/stop/restart + apalis-redis 适配器 + 4 类系统任务 + payload AES-GCM)、script(8)+script_log(3)（脚本宿主：模块桥接/热重载/实体钩子/出站白名单/配额）、access_key(7, 含 AK/SK→JWT 签发 HMAC)、online_session(4, 若未在 P1 完成)、redis_cache_monitor(1)、server_monitor(1) | 任务与脚本是语义对齐高危区，逐 handler 差分 |
| E 内容 ~23 | file(5)/file_transfer(3)（multipart、预签名、HMAC 图片代理、range/disposition/acceptMime）、internal_message(6+5+4, SSE 发布) | SSE 独立 7789 + 长连接 + 流一致性校验 |
| 聚合 ~7 | admin_portal(3: routes/perm-codes/initial-context)、dashboard(4) | 树形菜单按权限裁剪、上下文聚合 |

- **验收**：每模块合并门槛 = 模块端点差分 100% 一致 + 该模块矩阵全绿 + 对应前端页面冒烟（三前端中至少 react 全量、另两端抽样）。

### Phase 4 — 收尾与对齐（约 2–3 M）

- [ ] 全量差分回归（197 路由 × 全矩阵）、三前端全页面 E2E 巡检。
- [ ] 部署对齐：docker-compose（7788/7789、pg/redis/minio）、Dockerfile、配置文件 schema 对位（configs/*.yaml 语义等价）。
- [ ] 空库引导（default_data 种子移植）、Swagger UI `/docs`（可选）、性能冒烟（分页/索引命中对比）。
- [ ] 文档：ARCHITECTURE.md（含与 Go 侧语义差异清单，对位 rushwind 的 porting-status 惯例）、模块新增指南（对位 add-crud-module skill）、运维手册。

### 里程碑总览

```
P0 地基/生成器      ████████░░░░░░░░░░░░  (4-6M)
P1 认证内核              ██████░░░░░░░░░░  (3-4M)
P2 鉴权/数据横切              ██████░░░░░░  (3-4M)
P3 模块流水线 A→E                  ████████████████████  (8-12M)
P4 收尾/验收                                    █████  (2-3M)
```

串行总计约 **20–29 里程碑（5–7 个月量级，单人全职）**；P0+P1+P2 完成后即具备「前端可登录并看到空数据后台」的可用切片，之后按批次持续交付。批内模块相互独立，可并行摊开。

---

## 8. 风险登记册

| # | 风险 | 缓解 |
|---|---|---|
| R1 | 生成器保真不足（绑定/序列化边缘与 Kratos 微妙差异） | 逐端点差分金样；先做 language 样板模块再放量；绑定计划生成逻辑直接对照 `protoc-gen-go-http` 产物逐条翻译 |
| R2 | 算子矩阵缺口 → 列表过滤静默失效 | 矩阵测试全量覆盖前端别名表；不支持算子必须显式报错且与 Go 一致 |
| R3 | 错误信封/状态码不一致 → 前端提示退化为 unknown、401 逻辑误触发 | 每 reason 金样；矩阵断言 (status, reason) 二元组 |
| R4 | 安全语义回归：租户门/数据范围/字段权限任一环漏掉即越权 | 矩阵先行（P2 建全量骨架，模块合并前必须补齐该模块用例）；TenantScopeRepo 封装为唯一仓储入口 |
| R5 | DDL 漂移（索引/唯一约束缺失 → 种子失败、幂等 fan-out 失效） | 黄金 DDL diff 门进 CI |
| R6 | Cookie/CORS/SSE 只在真实浏览器下可验证 | 三前端浏览器 E2E 为硬门槛 |
| R7 | 任务（asynq→apalis）与脚本（gopher-lua→mlua、goja→boa）行为差异 | 逐 handler 差分；配额/串行语义单测钉死；退路方案见 D5 |
| R8 | 审计管道语义（脱敏文本、表名提取词法差异） | 采样对拍：同一 SQL 集合双向比对 MaskSQL/ExtractTables 输出 |
| R9 | 长尾进度（197 端点 × 50 表） | 生成器 + 模块模板 + 每模块三绿门槛；批次独立可并行 |
| R10 | Postgres 生产 / SQLite CI 方言差 | 迁移与差分一律跑 PG（容器）；SQLite 仅单测子集，对位 go 仓做法 |

---

## 9. 立即启动清单（Phase 0 首两周）

> 进度回填（2026-09-14，Phase 0 会话一）：
> - 布局修正：仓库结构对位原版仓——后端全部置于 `backend/`（Cargo workspace 根 +
>   `backend/api` 契约树 + `backend/crates`），`frontend/` 为零改动占位（前端继续用
>   go 仓三个应用，见 frontend/README.md）。
> - `docs/binding-spec.md` 已落盘：Kratos v2.9.2 全链路线上语义钉死（路由/绑定/
>   EmitUnpopulated 响应/错误信封/form 解码全表），后续所有差分断言以此为准。
> - **已完成**：workspace 与 crate 链（admin-descriptor / admin-gen-codegen /
>   admin-gen / admin-proto / admin-runtime v0）全部编译绿灯。admin-proto 对 17 个
>   数据包（16 admin 模块 + pagination）产出 prost 类型 + pbjson serde（注解包经
>   描述符过滤排除，`skip_protoc_run` 防 protoc 覆写过滤集）；admin-gen 每次构建
>   从全量描述符**确定性再生**：203 条路由（与 Go 侧注册数完全一致，含
>   additional_bindings）、441 条 reason→HTTP 状态表、198 个服务 trait 方法、逐路由
>   绑定计划（双拼写字段路径、分组标量种类、枚举/map/list/oneof 结构、proto3-
>   optional 合成 oneof 如实记录）。第 3/4 条的 language 样板目标被直接全量达成。
> - **关键工具链事实**：protox 序列化描述符时**丢弃自定义选项字节**（google.api.http
>   / errors.code 全失）——admin-descriptor 因此改用 protoc（与 Go 侧 buf→protoc 同
>   源）产描述符，`protoc` 成为构建硬依赖（CI 需装 protobuf-compiler）。
> - BOM：源仓 2 个 proto 带 UTF-8 BOM（protoc 容忍、protox 拒绝），同步脚本已做
>   复制/哈希双侧 BOM 归一。
> - 会话二增量（同日）：依赖重排——StatusError + pool 落入 admin-descriptor（消除
>   admin-gen↔admin-runtime 依赖环），生成器改引用 admin_descriptor::StatusError；
>   admin-runtime 落地：信封编码 envelope::error_response（固定四字段手写实现——池中
>   无 errors.Status 描述符且形态为常量；orphan 规则禁止跨 crate IntoResponse，故为
>   自由函数）、status_lookup（error_status 表查询，未知→500/空 reason 对位 FromError
>   Unknown；codec_error=400/CODEC；internal_error=500/空 reason）、jsoncodec（body：
>   DynamicMessage 反序列化默认 DiscardUnknown；响应：transcode_from +
>   skip_default_fields(false)，prost-reflect 需 features=["serde"]）。
>   序列化金样 2/2 锁定：presence 字段（proto3 optional/消息/oneof 成员）未设置时
>   EmitUnpopulated 仍然省略（零值 Language → "{}"）；裸字段发射默认
>   （ListLanguageResponse → {"items":[],"total":"0"}，64 位整数字符串化）。
>   ——原"绑定器+信封编码"两项中的信封编码已完成，绑定器仍为最高优先待办。
> - 会话三续（同日）：binder 行为测试 11/11 绿（双拼写、oneof 成员/二次设置拒绝、
>   FieldMask 归一、repeated 追加语义与 [] 后缀、Go ParseBool 拼写集、嵌套
>   Timestamp、Struct map 双键语法 + Value 元素、非白名单消息叶子拒绝、
>   未知键/空值静默、多值拒绝——全部对位 binding-spec §2.2）；生成器 fixture
>   单测落地（经 protoc 产带注解描述符——protox 丢选项字节故夹具也走 protoc——
>   断言路由行/错误表条目/绑定叶子（含 oneof 记录与双拼写）/trait 签名/mount
>   发射）。全工作区 build+test 绿（14 项测试）。剩余队列：免鉴权 8 端点拆分与
>   with_authn/authz 接线（需 rushwind-http/authn-jwt/authz-rbac 集成）、
>   差分台架、算子矩阵文档、CI。
> - 会话五增量（同日）：免鉴权拆分 + 装配落地。生成器：AUTH_FREE 表
>   （rest_server.go AddWhiteList 1:1；按**操作**分类，additional_bindings 随主
>   绑定同侧）+ mount_<service> 双路由线程（router_pub/router_gate，各侧仅按需
>   mut 以保发射树无警告）+ nulls 模块（41 桩，全部走
>   status_lookup::internal_error 的 Unknown 形态）。发射产物核验：公开 8 /
>   门控 195 = 203，8 个公开绑定与白名单操作一一对应。admin-gen 新增
>   tests/whitelist.rs 语料守卫（双向：8 对全在、无多余、绑定计数含
>   additional_bindings）。admin-server 装配 crate：鉴权门为**自研中间件**
>   （gate.rs）而非 with_authn——rushwind-http 的失败包络是其自有形状，与
>   Kratos 四字段包络不符，零改动契约要求后者；校验走 rushwind-authn-jwt RS256
>   引擎，失败走 envelope::error_response + tables::error_status
>   ("admin.service.v1","UNAUTHORIZED")=401，消息按 Go auth.go 分支：
>   缺 bearer→"missing bearer token"、一切校验失败→"access token expired"
>   （Go 侧 IsValidAccessToken 布尔坍缩）；成功注入 claims 扩展对位 NewContext
>   （下游消费待 RequestContext 阶段）。HttpEdge：CORS（9 origins/6 methods/
>   6 headers/credentials）+ timeout 10s 逐字镜像 server.yaml rest 块；绑定
>   :7788 经 AxumServer + App 生命周期。冒烟四例全部对位：公开路由 500 桩包络
>   （数值 code）/无 token 401/"missing bearer token"/伪 token 401
>   "access token expired"/CORS 预检（名单内回 ACAO+ACAM+ACAC，名单外无
>   ACAO）。**冒烟抓到信封真实回归**：`{status}` 把 StatusCode Display 出
>   "500 Internal Server Error" 文本——修复为数值字段，并新增 tests/envelope.rs
>   金样 3 用例（数值 code、空 reason、serde_json 转义）钉死。全工作区
>   16 项测试绿（11 binder + 2 序列化金样 + 1 信封金样 + 1 生成器 fixture +
>   1 白名单守卫）。已知缺口（待后续阶段）：authz-rbac（sys_apis 策略装载，
>   存储阶段）、服务端 access-token checker（会话吊销，存储阶段）、真实服务
>   实现替换 null 桩（Phase 1+）、claim 保真（RequestContext 阶段）、SSE :7789。
>   fmt/clippy 组件本机缺失（rustup 未装），CI 必须安装并把 fmt/clippy/test +
>   protoc + sync 门做成流水线。
> - 会话三增量（同日）：binder 收尾完成（std base64、well-known 叶子全量：Timestamp
>   RFC3339Nano、Go ParseDuration 端口、8 包装器降级标量、FieldMask 逗号+snake、
>   Value 字符串包装、Struct 严格 protojson——注意 Struct 与 body 解码不同、
>   未知字段报错；列表绑定改为追加语义对位 list.Append；bind_form 错误直接
>   构造 CODEC/400）。依赖环二次消解：RouteWire（纯数据，admin-descriptor）
>   承载 glue 所需路由事实，error_status 表查询移入 admin-gen 手写 tables.rs，
>   admin-runtime 不再依赖 admin-gen。mount 胶水落地：glue::handle 实现
>   body→query→vars→静态转换→服务调用→序列化的完整生命周期（含 form_urlencoded
>   的 Go url.Query() 同语义查询解析），生成器发射 mount_<service>/mount_route_N
>   （203 条，axum routing::on + MethodFilter；GET 过滤器含 HEAD 为已知微分歧）。
>   全工作区编译绿灯。免鉴权 8 端点拆分与 with_authn 接线仍未做（属 authn 囫量）。
> - 会话二续：binder 骨架落地并编译通过——路径游走全语义（点分嵌套、双拼写解析、
>   map 双语法与后缀[]、oneof 二次设置拒绝、多值拒绝、未知键/空值静默跳过）、分组
>   标量强制转换、Go ParseBool 端口、枚举按名/按号解析。两个桩待补：std base64
>   解码（需 base64 依赖）、well-known 叶子构造（Timestamp/Duration/包装器/
>   FieldMask/Value/Struct——后接差分金样）。另需修正项：get_field_mut 在 Rust
>  borrow 语义下的嵌套游走重构（当前递归经 &mut Value::Message）。
> - 会话五续（同日）：算子矩阵 v1 落盘（docs/operator-matrix.md）。三向矩阵
>   （28 个 go-crud 枚举 × 前端别名拼写集 × gorm/pg SQL × rushwind
>   wire.rs→seaorm 路径）+ 待建别名层规范（admin 服务层组件：键分割须按
>   MakeFieldFilter 语义自实现——三段键取**末段**为算子，与 rust-utils
>   query_parser 固定取 parts[1] 相悖，不可复用；映射=direct+fold 合集与
>   wire.rs 的 proto 枚举面完全同构，建议提取共享表；丢弃集={like,not_like,
>   notlike,json_contains,array_contains,exists,未知别名}——gorm 对这些
>   case **缺席即静默丢弃**（LIKE/NOT_LIKE/JSON_*/ARRAY_*/EXISTS 经实证
>   0 命中）；EXACT≡EQ、Icontains 族折叠 Ilike+wrap 与 gorm pg 的
>   ILIKE+包装逐一对位）；DatePart 载荷在 gorm 活性路径惰性（仅测试引用）。
>   线上活性面实证：三前端共用同款 pagination.ts，实际发射集 = **{裸键 EQ,
>   __contains}**（ID 字段豁免正则、tenant �清理、orderBy 缺省
>   ['-created_at']、noPaging 推导均有记录）。分歧登记 D1-D5（丢弃 vs 报错、
>   iexact、正则/全文、like/not_like、错误面）为差分豁免集种子。本会话无
>   代码变更，16 项测试维持绿。rust FilterExpr Op 实测 **18** 个变体
>   （计划旧文写 17，已在矩阵注明 NotBetween 为不可达冗余）。
> - 会话七增量（同日，差分驱动的三轮修正）：①通用机制下沉框架仓（用户指示）——
>   `rushwind/crates/rushwind-http-binding`（form 绑定器从 admin-runtime 整体迁入 +
>   CT 解析（ContentSubtype 裸切片端口 + 注册子类型集参数化：参照系二进制为
>   json/proto/x-www-form-urlencoded，导入分析与线上双钉）+ 预绑定层 bindgate（body
>   含 CT 查表 + query 绑定，产出 BoundMessage 经请求扩展传递）+ 四字段信封与
>   StatusError/internal_error/codec_error 构造器）；`rushwind-http::cors_compat`
>   （gorilla/handlers@v1.5.2/cors.go 一比一移植：ACAM 仅对 defaultCorsMethods
>   {GET,HEAD,POST} 之外的方法发射且值为单方法、ACAH 反射通过项、defaultCorsHeaders
>   永许不反射、origin 拒绝→无头/OPTIONS 截断空 200、Vary>1 origins、expose/max-age
>   永不发射、canonical_header_key 端口）+ `HttpEdge::with_cors_compat` +
>   `CorsOptions::without_allow_methods/headers`（tower-http 未设置语义）。
>   ②结构对齐 go（用户指示）：backend/app/admin/service/{cmd/server/main.rs,
>   internal/server/rest_server.rs, configs/} + backend/pkg/middleware-auth/（鉴权门，
>   对位 pkg/middleware/auth）；旧 crates/admin-server 与 backend/configs 删除；
>   workspace members=["crates/*","app/admin/service","pkg/*"]。
>   ③生成器：mount 发射改 wrap 组合（bind 层逐路由最外、gate 仅门控路由在内层——
>   对位参照系 Bind/BindQuery 先于 ctx.Middleware 的实测顺序）；handler 签名改
>   Request（扩展携带预绑定消息）；gorilla 遮蔽分析（shadowed 标志入表 + 挂载跳过，
>   全语料恰 1 条：route 16 GET /admin/v1/apis/walk-route 被更早注册的
>   GET /admin/v1/apis/{id} 吸收——axum 静态段优先的补救；admin-gen 守卫测试硬钉）。
>   ④RouteWire 去 'static 生命周期参数（纯 'static 数据）；admin-runtime 瘦身为
>   glue（取扩展消息→路径变量绑定→静态转换→服务调用→序列化）+ jsoncodec 响应面。
>   ⑤差分结果：runner 修复（reqwest no_proxy——系统代理污染首轮数据；报告目录
>   自动创建）；装配差分 84 Fail→2：80 条门控 body 路由缺 CT 探针两侧 400 逐字节
>   一致（预绑定层对位成立）、114 条门控非 body 两侧 401 一致（gate 对位成立）、
>   CORS 预检两例（允许/拒绝 origin）对位通过（compat 移植生效）；余 2 项即
>   route 16（遮蔽分析修复 + runner 归 router-shadow 豁免）与 curated 的
>   gate-invalid-dicts 打了两侧皆无路由的路径（改打 /admin/v1/apis）。待两侧后端
>   再次在线后复跑确认 Fail=0（本轮 go 容器被环境回收，用户指示暂不再动容器）。
>   ⑥注：中途一轮"logout 有时 400 有时 401"的疑云系探针工具误——urllib 对带
>   data 的请求自动注入 form Content-Type，参照系行为始终自洽。
>   ⑦测试面：27 项全绿（11 binder + 1 envelope + 2 golden + 1 whitelist +
>   1 新增 shadow 守卫 + 1 fixture + 10 semantic）。信封/envelope 金样随信封迁移
>   rushwind-http-binding（其 3 用例经 admin-runtime tests 调用路径覆盖）。
>   ⑧文档：binding-spec §1.3（先注册先匹配 + 遮蔽集）、§2.1（注册子类型与消息
>   形态）、§2.3/§2.4（预绑定层实现与唯一排序分歧）、§6（gorilla CORS 全表）、
>   §7（豁免集扩容：head-on-get / path-bind-post-auth（含 router-shadow 落点）/
>   §1.3 三类路径形态）；plan §6 布局重绘。
> - 会话七续增量（同日，CI 门）：`.github/workflows/ci.yml`（ubuntu+windows 矩阵：
>   checkout tx7do/rushwind 并链为本仓兄弟目录以满足相对路径依赖 → dtolnay
>   stable + clippy/rustfmt 组件 → rust-cache → protoc 安装（apt/choco）→
>   `sync-protos.sh --check` 契约门（CI 无源仓时漂移检测自跳过）→ fmt 门
>   （`cargo fmt -p` 按**本仓 8 包**圈定，不越界管框架仓——框架仓由其自有 CI
>   管辖）→ `clippy --workspace --all-targets -- -D warnings` → test）。
>   **门先行本地全量验证**（本机 scoop rust 其实自带 rustfmt/clippy——在
>   `<sysroot>/bin`（`C:\Users\yangl\scoop\apps\rust\<ver>\bin`）而未挂 PATH，
>   加入即用；此前"组件缺失"判断作废）：首轮即揪出 4 处 clippy（collapsible_if /
>   iter_cloned_collect / needless_borrow / let_and_return，全修）+ 1 处 deny 级
>   never_loop（binder 首段循环永不第二次迭代——重构为直取根段 + 递归下沉，语义
>   等价，11 binder 测试全绿）+ 框架仓 1 处 doc 懒续行。fmt --all 全仓（含框架仓）
>   达标。**重要修正**：`content_subtype` 此前把 `;` 的搜索起点放在 `/` 之后——
>   上游 `strings.Index` 是**全值**搜索（`;` 在 `/` 前则子类型为空串必不命中，
>   即 `;application/json` 必须按未注册 CT 拒 400 而非当 json 解析）；该错误恰被
>   新跑通的框架仓内单测钉住（此前从未单独跑过该 crate 测试），已按上游源码
>   （kratos v2.9.2 internal/httputil/http.go:21-34）改正并加字节级边界防 panic。
>   workflow YAML 经 go yaml.v3 解析验证；sync 门本机演练通过（OK×2）。
> - 会话八增量（2026-09-15，crates 结构重整——下沉与删除）：**backend/crates 6→3**。
>   ①glue/codec/wire 下沉框架仓：admin-runtime 的生命周期尾（handle）与响应编解码
>   （serialize_response，EmitUnpopulated）整体迁入 `rushwind-http-binding` 新模块
>   `glue`/`codec`/`wire`（均改为**池参数化**——handle/serialize_response 显式接收
>   &DescriptorPool；RouteWire 迁为 wire::RouteWire）；**admin-runtime crate 删除**，
>   binder/envelope/golden 三测试迁 admin-gen/tests（26 项测试维持绿）。声明顺序注：
>   glue 消费 rushwind_authn::AuthClaims 扩展（http-binding 增 rushwind-authn 依赖，
>   框架内单向无环）。②生成器迁框架仓并参数化：admin-gen-codegen →
>   `rushwind/crates/rushwind-gen-http`（**不开新仓库**——对位 protoc-gen-go-http
>   住在 kratos 仓内的先例，与 rushwind-http-binding 同仓锁步演进）；部署旋钮
>   CodegenConfig { proto_module_path, pool_expr, auth_free }（生成物中的
>   glue::handle 调用第一参注入 pool_expr，mount 函数签名不变，装配层零改动）；
>   AUTH_FREE 表移入 admin-gen（admin_gen::AUTH_FREE），fixture 测试随迁并
>   自足（tests/third_party 内置 annotations/http/errors 三 proto）+ 新增断言钉死
>   impl 收口括号序列与 pool_expr 线程（一次真实回归：emit_nulls 丢 impl 收口
>   `}}`，双方法 fixture 测不出、全语料 41 服务即炸——已修+守卫）。③
>   admin-descriptor 并入 admin-proto：admin-proto/build.rs 现产**两份产物**——
>   protoc→annotated_descriptor.bin（含选项字节，DESCRIPTOR_BYTES/pool() 的源）
>   + protox→过滤集→prost/pbjson 类型；protox 全量闭包 descriptor.bin 输出删除
>   （无消费者）。**admin-descriptor crate 删除**。④装配层引用换线（state.rs
>   StatusError 别名指 rushwind_http_binding::envelope；rest_server.rs
>   admin_proto::pool + rushwind_http_binding::wire::RouteWire；
>   authentication_service.rs 同）。⑤CI fmt 门包列表 8→5（admin-gen/admin-proto/
>   admin-diff/admin-service/middleware-auth）；workspace 依赖剪除 heck/
>   form_urlencoded（生成器/绑定器均已框架化）。⑥顺手修复并发会话遗留的
>   role_service.rs 三处编译错（proto Role 无 remark 字段×2 + bind_tenant_admin
>   Ok(Empty{})→Ok(())）与其余 clippy 项（dead_code 白名单注明 storage-phase
>   接线点）。两仓 build/clippy(-D warnings)/fmt/test 四门全绿。
> - ⚠ 并发编辑事故记录：本会话进行期间另一会话（服务实现线）在同一工作树上活动，
>   一度把 3.9GB 的 dict_type_service.rs 损坏文件自愈重写、后又按旧结构重建
>   admin-descriptor/admin-runtime（孤儿半成品）；用户停止该会话后本会话重新删除
>   并验证。教训：**同仓两会话并发写需用户显式仲裁**。
> - 会话八续（同日，crates/ 目录消解）：admin-gen 并入 admin-proto 并更名
>   **`admin-api`**，落位 `backend/api/admin-api`（对位 go 仓把生成绑定放
>   backend/api/gen 的布局，契约 crate 与其 protos 同址；当初拆两 crate 的动机
>   ——admin-runtime 时代依赖环——已随其删除消失）；admin-diff 迁
>   `backend/testbed/admin-diff`（与所驱动的 compose/豁免集同址，资产路径本就
>   CLI 参数化、零代码改动）；**backend/crates/ 目录删除**。AUTH_FREE 表单源
>   `src/auth_free.rs`（lib include! 重导出 + build.rs include! 消费——pub const
>   不能在 fn 作用域 include）。生成物路径改 `crate::proto` / `crate::pool()`
>   （生成面与类型面同 crate，重命名免疫）。workspace members = ["api/admin-api",
>   "app/admin/service", "pkg/*", "testbed/admin-diff"]；CI fmt 门 4 包；21 个
>   引用文件 sed 换线（admin_proto::/admin_gen:: → admin_api::）。四门绿
>   （26 项测试 + sync-protos 契约门）。
> - **会话八（服务层实现，2026-09-15）**：41/41 服务全部真实实现（零 null 桩）。
>   鉴权链 1:1：AES-128-CBC 前端密码（`f51d66a73d8a0927`，IV=密钥）+ bcrypt（恒时
>   假校验防枚举）+ 登录限流（5 次/15min，IP+用户名 Lua 原子）+ 验证码（6 位无歧义
>   字符集 + Redis 10min + PNG 渲染）+ 租户解析/登录策略（黑 CIDR/时间窗/设备）+
>   标识符反查（email/mobile）→ RS256 令牌对（UUIDv7 jti）+ Redis `at/rt/us:{ct}:
>   {uid}:{jti}` 白名单 + 刷新 Lua"验证即吊销"轮换 + HttpOnly cookie 对 + MFA
>   TOTP（SHA1/6 位/30s/±1，`enc:` AES-GCM 密文落库）+ AK/SK（SHA-256 恒时比对，
>   机器令牌）。服务面：用户/角色/菜单/接口（SyncApis 从 **内嵌 OpenAPI 资产**
>   `cmd/server/assets/openapi.yaml` 重建）/权限（SyncPermissions 菜单→权限码推
>   导）/租户（WithAdmin 事务开通）/门户/个人资料/字典×3/组织（物化路径子树迁
>   移）/职位/套餐×3/配置/登录策略/在线会话（Redis 扫描）/6 类审计查询/dashboard/
>   监控×2/脚本×2/通知渠道/任务（5 类注册类型 + ControlTask 按类型名）/消息×3
>   （收件箱 + SSE 推送）/文件×2（50MiB + MIME 白名单 + sha256 + 本地对象镜像）。
>   **data 层**：`internal/data/scope.rs`（Viewer：Noop/User/System 三态，对位
>   TenantPrivacy/EnforceTenant——租户判定收敛 repo，不再散落服务）+
>   `internal/data/repos/`（20 个仓储文件，一仓一文件对位 `*_repo.go`，
>   `paged_list` 对齐 PagingRequest）。**审计写入中间件**（audit.rs，对位
>   `pkg/middleware/logging`）：login（含风控引擎 FAILED+50/未知用户+10/20/
>   缺设备+10/缺 IP+5/内网−10，LOW/MEDIUM/HIGH）/api（跳过 Login+VerifyMFA）/
>   operation（仅写方法 + sessionOnlyOperations 跳过）/permission（target/action
>   解析 + body 目标名提取）；64KiB 请求体快照重建、request-id 三级回退。
>   **SSE :7789**（sse_server.rs）：token 三来源（Authorization/X-Token/?token=）
>   + `?stream=userId` 防跨订阅 + notification 事件（id=GUIDv4）；与 REST 同一
>   生命周期双 server。**配置驱动**：server.yaml（rest.addr/timeout/cors 全块 +
>   sse.addr/path）解析进 Config，rest_server/main 全部吃配置。
> - **下一步**（优先级序）：差分复跑确认 Fail=0——台架已建成（backend/testbed
>   docker-compose 双后端回放 + admin-diff 归一比较器 + 豁免集引用 operator-matrix
>   §5；run-2 分布 202 Ok / 2 根因消解 / 90 豁免 / 9 Pending，见会话七⑤；go 容器
>   被环境回收且用户指示未经允许不再动容器，故复跑顺延）→ golden DDL 管道 /
>   序列化金样扩展 / SSE :7789（Phase 0 尾项）。authz-rbac 接线与
>   access-token checker 依赖存储阶段（sys_apis / 会话表）。
> - **会话十二（2026-09-15，迁移框架化 + 现有环境联合测试，框架 4e222ef 已推 / admin b942f9d、50f9efe）**：
>   用户两条硬性指示：sea-orm-migration 必须接入（schema 所有权归本仓）；迁移能力写入
>   框架仓。框架新增 rushwind-storage-seaorm-migration：EntityTables::new(name, backend)
>   .table::<E>()…statement(stmt).build() → EntityTablesMigration；**PG 关键矫正**：Unsigned
>   列→Integer（sea-query 把 PG unsigned 渲染成 bigint 而 u32 解码只认 int4）、Enum 列→text
>   （fresh 库无 CREATE TYPE）、单自增 PK 同步矫正+serial。API 事实：sea-orm 2.0 的
>   ColumnType::Unsigned 是单元变体；MigrationTrait 是 async_trait 宏——impl 必须同标注且
>   manager 参数裸写 &SchemaManager。**联合测试**（citus :5432 + 宿主 Redis :6379，未动
>   docker）全绿：migrate 42 表 → seed → REST/SSE/apalis/cron 四 transport 同生命周期 →
>   门控 401 信封逐字节 → captcha → AES 登录（RS256 全套 claims）→ 数据面 200 → 双
>   cookie 轮换 → logout → 登出后 401 → openapi 200。联合测试揪出三真 bug：captcha 渲染
>   panic（set_chars 只设字母表不画图 + view 越界）；登录权限常量 system: 前缀应为 sys:；
>   seed::run 用 let _ 吞错改传播。运维事实：sqlx 只认 URL 形 DSN + ?sslmode=disable；
>   seed 管理员 admin/Abcd@1234（AES 前端层密文后登录）。
> - **会话十三（2026-09-15，SSE 通道对位——CORS 修复 + 跨用户泄漏修复，c506606 + 25e76bf
>   已推）**：根因=Rust SSE 无 OPTIONS 处理器、流响应无 CORS 头。全照源码推导（kratos
>   sse v1.3.8；bootstrap v0.0.4 不调 WithCORSAllowOrigin → corsAllowOrigin 恒 `*`）：
>   ①预检=鉴权前静态 204+4 头；②鉴权失败一律 401+http.Error 文本形态（body 的 code 带表
>   值 403 而状态行 401）；③成功路径五头；④事件帧顺序 id→data→event（axum 按 builder 调用
>   序拼帧）；⑤**真 bug**：Hub unfold 曾丢弃 user_id 过滤 → 跨用户站内信泄漏，修为不匹配
>   continue；⑥去掉 Go 没有的 keepalive ping；⑦路由接 server.sse.path。四路探针逐字节对位；
>   binding-spec 新增 §7。
> - **会话十四（2026-09-15，验证码视觉重做 + 裁剪包络，eb49246）**：去噪椒、每题随机深色
>   调色板（着色对象=全部纯黑像素，故只描边不上底）、水平正弦波、字形池∩字体字库交集
>   （font_default.json 缺 'L'，不过滤则答案偶发 <6）；包络拒绝重采样：字形运行宽超出
>   [96,232]px 即整题重画（拒率 6.6%、32 次预算耗尽 ~1e-37），输出 240×96 对齐前端
>   110×44 object-contain 盒；修掉裁剪窗切边字形的历史缺陷（旧 220 窗 17.6% 触边→零违例）。
>   工具教训：Read 不支持 PNG 视觉输入 → 像素统计 + 墨迹密度 ASCII 下采样做结构目检；
>   netstat 管道 grep 列间空白模式静默零匹配（用 findstr/宽松模式）；服务端起听 ~24-50s；
>   exe 锁挡 cargo 重链接（重建前 taskkill）；rand 0.10 改名 rng/random_range。
> - **会话十五（2026-09-16，CI 两连根修——c671430 + e90c655，均已推）**：①快照门连挂五轮
>   根因=本地 dev server 落的 .vite/deps 缓存被 hash_tree 哈希进清单（目录清空对被握句柄
>   静默失败）→ 本地过、CI 挂。修（sync-react.sh v4）：哈希宇宙=树上文件 − git 忽略路径
>   （git check-ignore --stdin -z；git 不可用硬失败）+ --check 失败打 diff 前 40 行。验证
>   方法论：git worktree add <tmp> HEAD 覆盖修复文件 = CI checkout 精确模拟。**通用铁律：
>   git-ignored 路径永不出现在 CI checkout——凡"本地扫全树"式清单门不排除 ignored 路径，
>   就是本地过 CI 挂的定时炸弹。**②ubuntu 挂遮蔽路由索引漂移根因=collect_protos 用未排序
>   read_dir 收 112 个 proto → 顺序原样进 descriptor → ROUTES 表序平台依赖（NTFS 名序 /
>   ext4 哈希序）。修：compile_files.sort()（PathBuf 字典序跨平台同排列）。终态：run
>   35051411873 双 job 全绿——本仓首次 CI 全绿。
> - **会话十六（2026-09-16，装配层向框架内聚 + 端到端阳性回归）**：服务侧手写
>   装配（CORS 循环、HttpEdge 尾段、parse_addr、server.yaml serde 镜像、
>   Go 时长解析、静态 cron 手工挂载）全部下沉 rushwind-bootstrap：框架新增
>   BindWire（host-any `":port"` 形式）与 DurationWire（秒数或时长串，吸收
>   parse_go_duration 及其测试），RoutePackRef 增 verbatim `settings` 节点透传
>   （框架提交 9f260ec，后续 4d3d524）。admin 的 server.yaml 重整为 bootstrap
>   装配文档（app + servers[]：http/edge{cors+compat}/route_packs、admin-sse、
>   admin-tasks 两个工厂 kind、cron 按名挂载三作业）；rest.rs 收敛为单个
>   route pack（bind 外层 + 鉴权门内层的 per-route 线序与审计层原地保留，
>   edge 归框架）；sse.rs/apalis.rs 改工厂注册，TaskQueue（enqueue 侧）与
>   ApalisServer（worker 侧）共享存储句柄拆分；config.rs 只余 data/auth/oss
>   三文件解析 + env 覆盖；rushwind-core/rushwind-http 依赖移除。框架侧顺手
>   修复 transport-cron 真缺陷：30s ticker 相位锁定启动时刻，启动相位不在
>   `:00/:30` 则任何作业永不触发（4d3d524，first deadline 锚定下一半分钟
>   边界）。门禁：框架 cron/bootstrap 两 crate fmt/clippy/test 绿；admin 四门
>   绿（fmt/clippy -D/test 40——2 个时长测试随解析器迁框架/deny ok）。端到端
>   阳性回归（宿主 citus+redis，17 探针全过）：门控 401 信封、CORS 预检三
>   形态（反射 ACAH/默认方法无 ACAM/越册 origin 空体 200 截断）、captcha→
>   Redis、AES 登录 + 双 cookie、数据面 200、openapi 410KB（经 pack settings
>   开关挂载）、SSE 四路（预检/无 token 401 文本/活性五头/越册 401+403 体）、
>   refresh 轮换、logout 吊销 401、cron→队列→worker 全链路（插入 PERIODIC
>   行后 `[scheduler] backup fired` 阳性）。运维事实两条：Docker Desktop 的
>   IPv6 回环端口代理挂死（`::1:5432` 超时、`127.0.0.1` 瞬通）——宿主联调
>   env 必须显式 IPv4，`localhost` 会被 Rust 解析优先走 ::1 导致池超时假象；
>   另发现更名前的 `gwa-admin-server.exe` 僵尸进程占 7788/7789（已清）。
> - **会话十六续（同日，授权层落地）**：go 参照语义核实——部署配置
>   `authz.type: noop`（kratos-authz 三引擎 noop/casbin/opa 中取放行引擎），
>   认证中间件内含 TenantAccessChecker（仅 tid>0：状态 OFF 拒、READONLY 套餐
>   到期只读放行 GET/HEAD/OPTIONS、按 path-template+method 查 sys_apis 归类
>   business_module、套餐模块白名单，全链 fail-closed），authz.Server 按角色
>   逐个评估首允即短路 + 每次评估落 sys_policy_evaluation_logs 行（含
>   permission/policy 反查，60s TTL 缓存；traceparent→X-Request-Id、
>   X-Real-IP→XFF 首段）。Rust 落地：crates/auth 增 TenantAccessChecker/
>   AuthorizationEvaluator 两 trait + 门内两阶段（401 之后、FORBIDDEN 信封
>   走错误表），路径模板取 axum MatchedPath（axum 增 matched-path feature）；
>   服务侧 authorizer.rs 实现两 impl（noop 首角色放行 + 留痕行 + 反查缓存；
>   本 schema 无 sys_permission_policies 表，反查的 policy 引用恒空——go 侧
>   亦无管理端点，惰性数据）。验证：租户矩阵探针 14/14（夹具：tenant_code
>   登录域、ACTIVE 角色绑定、sys:access_backend 权限；OFF/到期只读写拒读通/
>   白名单/无套餐/未注册路由 fail-closed 全对位 + 留痕行计数与形状）+
>   原 17 探针回归全过。新事实：登录流按请求体 tenant_code 划域（不带则
>   平台域），跨租户标识符一律防枚举回 INVALID_PASSWORD。
> - **会话十六续3（2026-09-17，MinIO 直连 + 前端全页面走查，e880afd 已推）**：
>   文件服务三端点接入 rushwind-oss-s3：五内容桶各一引擎实例（path-style，connect 时幂等
>   create_bucket）；上传内容嗅探覆写客户端 MIME + 签名公开图链（HMAC-SHA256 crypto_key、
>   1 年期）；新增公开签名图片代理 `/admin/v1/file/image`（挂 pack 内 gate 外，对位参照的
>   无 Operation 手工路由）；RUSHWIND_OSS_ENDPOINT 环境覆盖供宿主联调。**修真配置 bug**：
>   MinioSection 字段名与 yaml 键 upload_host/download_host 不匹配（dead code 期未暴露）。
>   探针 8/8：multipart 400 对位、JSON 上传（bytes 走 base64）、图链字节一致、篡改签名 403、
>   信封 base64 回环、删后 404。**前端全页面走查**（本仓 react 起独立实例 5899——5888/5889
>   为并行会话的 go 前端）：29 子页全渲染零崩溃，仪表盘真实统计 + 三图表，登录全链 UI 走通，
>   清存储后 refresh 静默恢复会话 ✓，用户/角色/权限点/语言/三类审计/策略评估日志全有真数据。
>   **发现 W1（高，上游前端）**：axios 401 刷新拦截器 refreshTokenQueue 的 Promise 无超时无
>   reject，触发后全部数据页渐进永久 loading；后端审计行逐笔 200、页面上下文裸 fetch 22ms、
>   手动 refresh 33ms——网络与后端无罪，修在上游（队列超时/reject + isRefreshing 复位）。
>   **W2（上游）**：multipart 上传死路径——kratos 无 form-data codec，参照后端同样 400
>   CODEC；生成客户端的 JSON 上传路径正常且本仓已验。W3：IAB 截图能力损坏，登录页品牌
>   DOM 验收过（logo/标题/租户编号框/Copyright 全 RushWind），像素级验收待补。
> - **会话十七（2026-11-05/06，ce8ab49 vintage 全量接受 + 十域完整移植 + 走查，
>   350b3e5…59779ce 共 18 提交，本地 main 待推）**：框架 rev eaa813e1→e33b2c53
>   （jsonwebtoken 11 摘 rsa Marvin-Attack、sqlx 0.9、bootstrap 拆分零断点）。
>   **vintage 一体接受**（契约+三前端+品牌层同提交）：protos 116→149（新增
>   ai×10/bi/print/monitor_alert 四域与 notification_preference/template、
>   task_monitor、sso、tenant_branding；oauth.proto 上游删除换 sso.proto；
>   LoginResponse 掉 refresh 字段改纯 cookie——buf breaking 仅此 3 处破坏，
>   显式接受），生成面 284 路由/59 trait/556 方法/546 错误；vue-vben 上游
>   monorepo→单包重构，overlay/替换表随迁（{{year}} i18next 转义一并适配），
>   白名单影子测试 16→52（AI/BI/print BFF 注册前移所致，影子本身未变）。
>   **逐域完整移植**（每域一提交门禁绿）：OIDC SSO 全流程（discovery 缓存/
>   state 单次防重放/邮箱映射+自动预置）与租户白标 → 通知偏好/模板（render
>   内核：未知占位符按名报错）→ 渠道 EXT settings 三件套 + 39 schema 注册表
>   （逐字转写 grep 验证）→ 监控告警（评估器：告警→恢复成对/冷却锚/采集失败
>   不清 firing）→ task_monitor（rushwind_apalis_jobs SQL 投影）→ 自助用量
>   → 角色权限继承（子⊆父写侧约束+防环，登录面无需展开——约束在编辑边界）
>   → user locale → BI 仪表盘（乐观锁 CAS+内置模板播种）→ 打印模板+
>   chromiumoxide HTML→PDF（分工边界同上游：前端分页/毫米布局，后端只打印）
>   → AI 域（9 表迁移；provider 加密落库+脱敏、chat 闭环、知识库 RAG
>   （pgvector 判存守卫：扩展缺席迁移安静跳过）、Agent 图引擎 1:1（五节点/
>   校验/模板/trace；条件表达式走受限求值器=上游 expr-lang 常用子集，超集
>   报错——语义差异已声明）、内容生成四场景、菜单语义索引；**NL2SQL 显式
>   NOT_IMPLEMENTED**：四重只读护栏必须整链落地，护栏是注入防线的本体）。
>   **走查（真实栈）**：admin-api 迁移+种子+登录（redis 读验证码配方）+
>   API 冒烟 18/19 绿（render/evaluate/409/加密落库全验证）+ react dev
>   （docker node:22 --network host，宿主 snap node 全废）浏览器走查全绿
>   （品牌/真实数据/BI 种子/模板回显/AI 提供商）。**两个真修复**：①vector
>   迁移在 PG 事务里 CREATE EXTENSION 失败即 abort 全链——改 pg_extension
>   判存守卫（扩展缺席静默跳过降级）+ 逐步迁移循环（失败报上一成功迁移名）；
>   ②matchit 拒绝 `{id}:copy` 混合段（AIP-136 verb-on-resource）启动 panic——
>   框架 gen-http b30e4bc 生成 is_verb_on_resource 守卫跳过该类注册（284 中
>   仅 bi Copy 1 条 404，trait/服务面完整），走查期 admin-api 靠 [patch] 指
>   本地 rushwind 仓，**框架 push 后须撤 patch+bump rev**。**遗留**：出站
>   投递里程碑（SMTP/WEBHOOK/SMS/EXT 真实 sender+台账实投+告警周期扫描，
>   通知域 heritage 欠账）、NL2SQL 里程碑、差分栈对拍（vben pnpm build 冒烟
>   亦待跑）。

1. [x] 仓库骨架 + workspace（backend/ 结构就位）；CI 已建（.github/workflows/
       ci.yml：fmt/clippy/test + protoc 安装 + sync-protos --check 门，双 OS 矩阵）。
2. [x] `backend/api/protos` 同步脚本 + MANIFEST checksum 门（BOM 归一，含源仓漂移
       检测，已验证生效）；third_party vendor 脚本（六模块锁定版本，PROVENANCE.md
       记录来源）。
3. [x] `admin-proto` crate：protox→prost/pbjson 类型生成（全量数据包，构建绿灯）。
       序列化金样测试待补。
4. [x] `admin-gen` 生成器：路由表 + 绑定计划 + 服务 trait + 错误表 + mount 胶水
       （含免鉴权 8/195 拆分与 null 桩，语料守卫测试钉死）——全量语料达成。
       gnostic 免鉴权标注扩展、redact/PGV 规则表待后续增量。
5. [ ] 差分台架：docker-compose 起 pg/redis/minio + go 后端（7788）+ rust 后端
       （预留），回放器雏形 + 归一比较器。
6. [x] 算子矩阵文档 v1：docs/operator-matrix.md（三向矩阵 + 别名层规范 +
       分歧登记 D1-D5；线上活性面实证 = {EQ, CONTAINS}）。

> 本计划为活文档：每阶段收口后回填实际偏差与决策修订。
