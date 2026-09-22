# 绑定与线上序列化规格（binding spec）

> 本文档是 rushwind-admin 对 Go 后端线上行为的**逐字节对齐基准**，所有语义均钉死到源码。
> 适用范围：go-wind-admin 的 REST 面（Kratos v2.9.2 运行时 + protoc-gen-go-http v2.9.2 生成物）。
> 差分测试（`admin-difftest`）以本文档为准绳；与本文档冲突即视为回归。
>
> 事实源（本地可查）：
> - 生成物：`D:\GoProject\go-wind-admin\backend\api\gen\go\**\*_http.pb.go`、`*_errors.pb.go`
> - 运行时：`C:\Users\yangl\go\pkg\mod\github.com\go-kratos\kratos\v2@v2.9.2\`
>   （`transport\http\{server.go,codec.go,context.go,router.go,binding\bind.go}`、`encoding\json\json.go`、`encoding\form\proto_decode.go`、`errors\errors.go`）
> - 路由匹配：gorilla/mux v1.8.1（go.mod 锁定）

## 0. 总则

Go 后端的 HTTP 面由 Kratos v2.9.2 原生运行时驱动：

```
mux.Router (gorilla/mux, StrictSlash=true)
  → Server.filter()（注入 Transport{operation=pathTemplate}，超时 ctx）
  → 生成的 handler（i_*_http.pb.go）
       ctx.Bind       → DefaultRequestDecoder   (codec.go:61)   body → protojson(DiscardUnknown)
       ctx.BindQuery  → DefaultRequestQuery     (codec.go:56)   query → form codec
       ctx.BindVars   → DefaultRequestVars      (codec.go:46)   path vars → form codec
       http.SetOperation（写 operation 元数据，供审计/中间件匹配）
       ctx.Middleware (context.go:94)  → middleware.Chain(srv.middleware.Match(operation))(...)
       out, err := h(...)              → 服务实现（错误原样上抛）
       ctx.Result(200, reply)          → DefaultResponseEncoder (codec.go:84)
  → handler 返回 err ≠ nil → srv.ene → DefaultErrorEncoder (codec.go:107)
```

要点：
- **middleware 按 operation（= 路由模板字符串，如 `/admin/v1/dict/langs`）匹配**，与具体路径参数值无关（server.go:287-291、context.go:94-99）。
- 成功/失败两条编码路径都用**同一个 codec 选择器**：成功按 `Accept`，失败按 `Accept`；均回退 `json`（codec.go:121-129）。前端不发 `Accept`，恒为 `json` codec。
- `json` codec（`encoding/json/json.go`）：proto.Message → `protojson.MarshalOptions{EmitUnpopulated: true}`；解析 → `protojson.UnmarshalOptions{DiscardUnknown: true}`。
- **所有响应（成功与错误）的 Content-Type 恒为 `application/json`**（`httputil.ContentType("json")`，无 charset）。

## 1. 路由模型

### 1.1 注册形态（生成物 i_*_http.pb.go:47-55）

```
r := s.Route("/")
r.<METHOD>("<path>", _<Svc>_<Method><N>_HTTP_Handler(srv))   // 逐条注册，含 additional_bindings 的每条绑定
```

- `N`：同名方法去重编号（跨 service 累计计数，`methodSets`）。
- `sd.Methods`（含 additional_bindings 展开）逐条注册；`sd.MethodSets`（同名去重、**后注册者覆盖**）生成接口与 operation 常量。

### 1.2 路径模板形态（实测全量扫描，211 条）

- 字面段 + 普通变量段 `{name}`（80 条含变量）。
- **无** `{name:regex}`、**无** `*`/`**` 通配、**无**点路径变量。
- 9 条冒号动词后缀（字面段含 `:`）：`sync:perms`、`tasks:{control,restart,start,stop,type-names}`、`tenants:{exists,with-admin}`、`users:exists`。
- `:exists`/`:with-admin` 等是 mux 的**字面段**（mux 只把 `{}` 当变量语法）。

axum 0.8 的路由语法（`{name}` 参数、`:` 字面量）与上述形态**一一对应**，无需变换。
rushwind-transport-axum 使用 axum 0.8（`crates/rushwind-transport-axum/Cargo.toml:15`）。

### 1.3 gorilla/mux v1.8.1 匹配语义（Rust 端对齐基准）

- `{name}` → 匹配 `[^/]+`，捕获为路径参数（与 axum `{name}` 等价）。
- **匹配优先级：先注册者先匹配（mux.Match 按注册序线性扫描，返回首个命中）**。`{var}` 模式段匹配任意段——因此**更晚注册的字面路径若与更早注册的同方法 `{var}` 模式同形，则被该模式路由吸收、自身不可达**。axum/matchit 相反：**静态段恒优先**。对语料做 gorilla 遮蔽分析（生成器 `shadow_flags`）：恰好 1 条被遮蔽绑定（route 16，`GET /admin/v1/apis/walk-route`，被同文件更早注册的 `GET /admin/v1/apis/{id}` 吸收；集合由 admin-api 守卫测试硬钉）。**对齐手段**：被遮蔽绑定不挂载——两侧请求都落在吸收它的 `{var}` 路由上；该路由的路径变量对字面段取值必然畸形，落入 §2.3 的路径绑定排序分歧（豁免类 `router-shadow`）。
- 方法不匹配 → mux `MethodNotAllowedHandler`；未匹配 → `NotFoundHandler`（Go 后端均接 `http.DefaultServeMux` → 404）。axum 对应 405/404，行为近似。axum 的 `MethodFilter::GET` 同时接受 HEAD（已知分歧，豁免类 `head-on-get`）。
- `StrictSlash(true)`：`/path/` ↔ `/path` 相互 301 重定向（server.go:189）。**已知分歧**：axum 无此行为（尾斜杠 404）。
- mux 默认对请求路径做 `cleanPath`（`//`、`/../` 归一）并 301。**已知分歧**：axum 不做。
- mux 在**解码后**的 `URL.Path` 上匹配：`%2F` 解码成 `/` 会切断段 → 404。axum 在编码路径上匹配参数段再解码 → 可能命中。**已知分歧**。

三条已知分歧均不在前端真实请求形态内（前端路径由模板拼接、无尾斜杠/编码斜杠/点段）。差分比较器将这三类请求归入「豁免集」并在报告中标注，不计失败；若后续要在字节级也复刻，需在 Rust 边缘加显式重定向层（P4 决策）。

## 2. 请求绑定

### 2.1 Body（`DefaultRequestDecoder`，codec.go:61-81）

- codec 按 `Content-Type` 子类型选择（`ContentSubtype`：取 `/` 与 `;` 之间的原始切片，`;` 从**整个值的首字节**起搜（`strings.Index` 全值搜索），`;` 位于 `/` 之前 → 子类型为空串必不命中；**无大小写折叠**——注册表大小写敏感，`application/JSON` 不命中 `json`）。解析循环遍历**全部** Content-Type 头，首个解析成功的值胜出；全不命中 → 400。该 400 的消息回显 `Header.Get`（仅首个值，缺头时空串）。
- **该参照部署二进制注册的子类型（2026-09-14 经导入分析与线上双重钉死）**：`json`（protojson）、`proto`（二进制 wire）、`x-www-form-urlencoded`（form codec：`url.ParseQuery` 产出键值对后走与 query 绑定相同的 populate 机制）。
- **未注册子类型** → `errors.BadRequest("CODEC", "unregister Content-Type: <值>")` → HTTP 400、reason=`CODEC`。该查表发生在读 body **之前**——空 body 也 400。
- body 为空（len==0）→ 直接成功、消息保持零值。
- `json` codec 解析：protojson `DiscardUnknown: true`（未知字段忽略、不报错）。解析结果**赋入**请求消息（protojson 的 unmarshal-into 语义）。
- 解析失败 → `errors.BadRequest("CODEC", "body unmarshal <err>")` → 400/CODEC。
- `google.protobuf.Empty` 请求（`{}` 或空 body）→ 零值消息。
- Rust 侧（`rushwind-http-binding`）：json/form 两路全量实现；`proto` 子类型**透传不校验不绑定**（无 wire 解码端口，休眠分歧——前端恒发 json）；form 的 `url.ParseQuery` 非法转义报错 vs `form_urlencoded` 有损解析为已知休眠分歧。

### 2.2 Query / 路径变量（form codec：`encoding/form/proto_decode.go` 全文钉死）

`DefaultRequestQuery`（query 全集）与 `DefaultRequestVars`（mux 捕获变量转 `url.Values`）都走
`binding.BindQuery` → `encoding.GetCodec("form").Unmarshal(url.Values.Encode(), msg)`
（bind.go:13-18；`Encode()` 产生 `k=v&k=v`，值再被 form 解码器逐字段解析）。解析错误 → 400/CODEC（bind.go:15）。

字段解析（populateFieldValues / getFieldDescriptor / parseField）：

| 语义 | 行为 |
|---|---|
| 键拆分 | 按 `.` 拆嵌套路径（`storageObject.bucketName` → StorageObject.bucketName；中间消息经 `Mutable` 逐级分配） |
| 字段名解析 | **双拼写**：先 `ByName`（proto 原名，snake_case），再 `ByJSONName`（json_name，camelCase）——两种拼写都接受 |
| 未知字段 | 静默忽略（`return nil`，整条键放弃） |
| map 字段 | 两种键语法：`map[key]`（括号式）与 `m.k`（单点式，且仅当整键只有一个 `.`）；`field[]` 后缀剥掉后当列表字段 |
| 叶子值：bool | `strconv.ParseBool` |
| 叶子值：整数/浮点 | `strconv.Parse{Int,Uint,Float}`（按字段位宽，Go 语义：溢出/非法 → err → 整个绑定失败 400/CODEC） |
| 叶子值：enum | 先按枚举值**名**查；失败再按 `ParseInt(…,10,32)` 转数值查 `ByNumber`；两者都失败 → 错误 |
| 叶子值：string | 原样 |
| 叶子值：bytes | `base64.StdEncoding` 解码 |
| 叶子值：`google.protobuf.Timestamp` | `time.ParseInLocation(time.RFC3339Nano, value, time.Local)`（**本地时区**解析后转绝对时刻） |
| 叶子值：`google.protobuf.Duration` | `time.ParseDuration` |
| 叶子值：wrappers（Double/Float/Int64/Int32/UInt64/UInt32/Bool/String/Bytes） | 标量转换（Bytes 先 std 后 url base64） |
| 叶子值：`google.protobuf.FieldMask` | **按逗号拆分**，每段 `jsonSnakeCase`（camel→snake）后加入 Paths（`{"paths":[…]}` protojson 形态由此而来） |
| 叶子值：`google.protobuf.Value` | `structpb.NewValue(string)`（把字符串包成 string Value） |
| 叶子值：`google.protobuf.Struct` | `protojson.Unmarshal(value)`（把 JSON 文本解析为 Struct） |
| 叶子值：其他消息类型 | `unsupported message type` 错误 → 400/CODEC |
| 空字符串值 | `populateField` 里静默跳过（`if value == "" return nil`） |
| 多值给单值字段 | `too many values for field` → 400/CODEC |
| oneof 二次设置 | `field already set for oneof` → 400/CODEC |
| repeated 字段 | 每个 query 值解析后 `Append`（列表字段多值合法） |

### 2.3 绑定顺序（生成 handler 固定序列）

1. `ctx.Bind(&in)`（body：POST/PUT 且注解声明 body）
2. `ctx.BindQuery(&in)`（query；Create/Update/BatchCreate 与 Get/Delete 一样调用——**body 与 query 可叠加，query 后写覆盖 body 同名字段**）
3. `ctx.BindVars(&in)`（路径变量；仅模板含 `{}` 时生成）

三步均在 `ctx.Middleware`（鉴权链）**之前**执行；任何一步失败 → 直接上抛 → 400/CODEC 信封（`{"code":400,"reason":"CODEC","message":"…","metadata":{}}`）——即：**绑定/解码失败先于 401 到达**（差分 2026-09-14 两轮实证：80 条门控 body 路由的缺 CT 探针两侧同为 400，114 条门控非 body 路由两侧同为 401）。

**Rust 侧实现**（`rushwind-http-binding::bindgate`，装配为**逐路由的最外层**，见 services/admin-api/src/server/rest_server.rs 的 `wrap` 组合）：body（含 CT 查表）+ query 的绑定在层内完成，产出 DynamicMessage 经请求扩展传递给 handler。**唯一排序分歧**：路径变量绑定——axum 中间件拿不到路由捕获值，只能在鉴权层之后于 handler（框架 `rushwind-http-binding::glue` 生命周期尾）内执行；当「路径变量值畸形」且「令牌缺失/无效」同时发生时两侧分歧（参照 400 先行、复刻 401 先行；令牌有效时两侧同为 400）。豁免类 `path-bind-post-auth`。

### 2.4 Rust 实现决策

- 绑定器本体：`rushwind-http-binding::binder`（§2.2 全表语义的 1:1 移植，2026-09-14 迁入框架仓）。
- 生成器（框架 `rushwind-gen-http`，admin-api 构建期调用）从 descriptor 产出每方法的**绑定计划**：路径变量名列表（proto 字段路径）、body 模式（无/`*`）、query 叶子字段枚举（含嵌套点路径、map/list 标记、字段类型）。
- 预绑定层（bindgate）按计划执行 2.1/2.2 的 body+query 语义；glue（`rushwind-http-binding::glue`，2026-09-15 起自 admin-runtime 下沉框架仓）只做：取扩展内已绑定消息 → 路径变量绑定（§2.3 分歧）→ 静态 prost 转换 → 服务调用 → 响应序列化。
- 类型转换错误 → `StatusError::new(400, "CODEC", message)`。
- 业务层只接触静态类型；动态面（prost-reflect）只在「错误信封与响应序列化」需要 schema 时使用。

## 3. 响应序列化（`DefaultResponseEncoder`，codec.go:84-104）

- `ctx.Result(200, v)`：状态恒 **200**（生成物写死）；`v == nil` → 无 body。
- 序列化：`json` codec → `protojson.MarshalOptions{EmitUnpopulated: true}`：
  - **未填充标量 → 显式默认值**（`""`、`0`、`false`）；未填充消息字段 → `null`；未填充 repeated/map → `[]`/`{}`；
  - 枚举 → 枚举值名字符串（未填充 → 首个枚举值名，如 `XXX_UNSPECIFIED`）；
  - 64 位整数 → 字符串；bytes → std base64；Timestamp/Duration → RFC3339Nano/秒串（protojson 标准）；
  - `google.protobuf.Empty` → `{}`。
- Content-Type：`application/json`。
- **pbjson（标准 protojson）省略未填充字段——与 Go 相反。** Rust 响应路径因此不走 pbjson 序列化，改走：
  `静态 prost 结构 → encode_to_vec（二进制）→ prost-reflect DynamicMessage::decode（运行时 DescriptorPool）→ prost-reflect protojson 序列化 + serialize_defaults(true)`。
  prost-reflect 的 protojson 实现以 protojson 一致性为目标，`serialize_defaults` 对应 `EmitUnpopulated`。该等价性在 admin-api 的序列化金样测试中逐条钉死（枚举/整型字符串化/bytes/oneof/map/Timestamp/Empty）。

## 4. 错误信封（`DefaultErrorEncoder`，codec.go:107-118 + errors/errors.go）

- `se := errors.FromError(err)`：
  - `*errors.Error` 原样通过；
  - 普通 Go error → `New(UnknownCode=500, "", err.Error())` → **HTTP 500，reason 为空串**。
- HTTP 状态 = `int(se.Code)`（**直接用错误构造时传入的值**）。
- body = `se`（protojson EmitUnpopulated）——Status 消息四字段全量输出：
  `{"code":<int>,"reason":"<string>","message":"<string>","metadata":{…}}`
  （字段序 = errors.proto 字段序 1..4；`metadata` 未设置时输出 `{}`；`reason`/`message` 为空时输出空串。）
- Content-Type：`application/json`。
- reason↔code 表由 `*_error.proto` 的 `(errors.code)` 注解驱动：生成器把每个枚举值的注解值抽成静态表
  （例：`admin_error.proto` → `AdminErrorReason_BAD_REQUEST=0 → 400`、`UNAUTHORIZED=100 → 401`、`NETWORK_CONNECT_TIMEOUT_ERROR=3200 → 599`）。
  生成构造器形态（`*_errors.pb.go:24-26`）：`errors.New(<annotation>, "<ENUM_NAME>", fmt.Sprintf(...))`。
- 前端消费面：`reason`（本地 i18n 文案键）+ HTTP 状态（401 触发刷新/重放）。`message` 除登录页提示外不展示。
- Rust 侧：`rushwind-http-binding::envelope::StatusError { status: i32, reason: &'static str, message: String }`
  实现 axum `IntoResponse`：状态 = code，四字段信封手写编码（字段号序 + `metadata:{}` 尾，`code` 载数值——金样钉死）。
  全部 reason 走静态表；非表内错误一律 500/空 reason（对位 `FromError` 的 Unknown 分支）。

## 5. Operation / 审计

- `Transport.operation = pathTemplate`（路由模板串，非实际 URL）——中间件匹配与审计记录共用。
- Rust：生成的路由表条目携带 `operation_id`（`"/<pkg>.<Svc>/<Method>"`，生成器产出）与 `path_template`，随请求注入审计上下文。
- 免鉴权白名单（rest_server.go:80-100 登记的 8 个方法：Login、GenerateCaptcha、VerifyCaptcha、RefreshToken、VerifyMFAChallenge、AccessKeyServiceIssueToken、ForgotPassword、ResetPasswordByCode）是**装配期配置**而非生成物——Rust 装配器按 (method, pathTemplate) 集合划分 public/authed 子树（对应 rushwind-http `with_authn` 的白名单合并模式）。

## 6. CORS（server.yaml `rest.cors` → gorilla/handlers@v1.5.2）

参照系的 CORS 不走 tower-http，而是 `kratos-bootstrap/rpc@v0.1.3/rest.go:49-60`：server.yaml 四块配置（origins×9、methods×6、headers×6、allow_credentials）经 `handlers.Allowed*` 构造 **gorilla/handlers v1.5.2 的 CORS 处理器**，作为 server 级 filter 挂在 mux 外层。其发射规则与 tower-http 根本不同：

| 场景 | gorilla/handlers@v1.5.2（cors.go:60-135 实录） |
|---|---|
| Origin 不在允许列表 | **任何头都不发**；非 OPTIONS 请求原样透传到内层，OPTIONS 请求被截断为**空 200** |
| OPTIONS 且无 `Access-Control-Request-Method` 头 | 裸 400，无 CORS 头 |
| ACRM 方法 ∉ 配置 methods | 裸 405，无 CORS 头 |
| ACRH 中任一头 ∉（defaultCorsHeaders ∪ 配置 headers） | 裸 403，无 CORS 头（defaultCorsHeaders = Accept/Accept-Language/Content-Language/Origin，**永远允许**） |
| 预检通过 | ACAH = 通过的请求头（canonical 化、逗号连接，defaultCorsHeaders **不回显**）；**ACAM 仅当 ACRM ∉ {GET,HEAD,POST}（包内 defaultCorsMethods）时才发，值为该单个方法**；随后公共段：ACAC（若 credentials）、`Vary: Origin`（仅当配置 origins >1，**Set 语义覆盖既有 Vary**）、ACAO = 命中的 origin（未配置 origins 时为 `*`）；响应 = 空 body + optionStatusCode(200) |
| 非预检且 Origin 允许 | 请求透传到内层，响应附 ACAC/Vary/ACAO（同上） |
| expose / max-age | **从不发射**（bootstrap 未接这两个选项） |

Rust 侧：`rushwind-http::cors_compat`（1:1 移植，含 `canonical_header_key` 端口与 `Set` 的头覆盖语义），装配走 `HttpEdge::with_cors_compat`（rest_server.rs）。差分 `cors-preflight` 两例（允许/拒绝 origin）2026-09-14 对位通过。

## 7. SSE 通道（:7789，kratos-transport/transport/sse@v1.3.8 + bootstrap/transport/sse@v0.0.4）

SSE 是独立 transport server（`sse_server.go` → `sse.NewSseServer(cfg.Server.Sse, WithSubscriber/WithAuthorize)`），与 §6 的 gorilla CORS **完全无关**——其 CORS 行为是该包 `http.go` 内的静态硬编码（`corsAllowOrigin` 默认 `*`，bootstrap 包装层从不调用 `WithCORSAllowOrigin`）。线上行为全集（源码推导，2026-09-15 Rust 侧对位实现并四路探针验证）：

| 场景 | 线上行为（transport sse/http.go 实录） |
|---|---|
| `OPTIONS /events`（任意头，鉴权之前） | 204 No Content + 固定四头：`Access-Control-Allow-Origin: *`、`Access-Control-Allow-Methods: GET, OPTIONS`、`Access-Control-Allow-Headers: Content-Type, Authorization, X-Token, Last-Event-ID`、`Access-Control-Max-Age: 86400` |
| 鉴权失败（缺 token / 签名过期 / 白名单吊销 / 黑名单 / stream 缺失·非数字·≠uid） | **一律 401**（`isForbidden` 对生成错误永假：包内 sentinel 是 stdlib error，`kratos Error.Is` 的 `errors.As` 反向不命中）+ `http.Error` 形态：`Content-Type: text/plain; charset=utf-8` + `X-Content-Type-Options: nosniff` + body 为 go-error 文本行 `error: code = {表值} reason = {reason} message = {msg} metadata = map[] cause = <nil>` + 换行（表值：UNAUTHORIZED→401、FORBIDDEN→403——即黑名单与 stream 失配的 body 带 `code = 403` 而状态行仍是 401）；**无任何 CORS 头**（SSE 头段尚未执行） |
| 鉴权通过 + `?stream=`=uid | `prepareHeaderForSSE`：`Content-Type: text/event-stream`、`Cache-Control: no-cache`、`Connection: keep-alive`、`Access-Control-Allow-Origin: *`、`Access-Control-Allow-Headers: Content-Type`，随后 200 + 事件流（`transfer-encoding: chunked` 为两端涌现的流式框架） |
| 事件帧 | 固定顺序 `id:`（GUIDv4）→ `data:`（recipient protojson）→ `event: notification`，空行终止；无 retry 字段、无 keep-alive ping（空转静默） |
| 订阅语义 | stream = userId：一用户多设备共享一条流，仅收本人 recipient 事件；`auto_stream: true` 下流随订阅自动建 |

Rust 侧：`services/admin-api/src/server/sse.rs`——`events_preflight`（OPTIONS 静态应答）+ `sse_error`（401 纯文本形态，code 字段取 `error_status` 表值）+ 流响应头三枚插入（ACAO/ACAH/Connection 叠加在 axum `Sse` 自带的 content-type/cache-control 之上）+ `Event` builder 调用序 id→data→event（axum 按调用序拼帧，与上述顺序对位）+ Hub 广播按 uid 过滤（等价 per-stream 订阅）。路由路径接 `server.sse.path`（缺省回退 `/`）。2026-09-15 四路探针（预检 / 无 token / 成功路径头集 / stream 失配体）逐字节对位通过。

已知分歧（差分台架未覆盖 SSE——:7789 不在 sweep 语料，本节为源码推导 + 探针验证的契约记录）：
- **方法面**：参照 mux 对全部方法走同一 handler（POST 也能开流）；axum 侧 GET（含 HEAD）+ OPTIONS 之外返 405（无 CORS 头）——前端只用 GET，不可达。
- **未配置 path 的回退**：参照 `/` 是 mux 前缀 catch-all；axum 侧为精确 `/`——嵌入配置恒为 `/events`，不可达。
- 头名小写（hyper 恒小写 vs Go canonical MIME 大小写）与 chunked/CL 框架差异为全 API 平台涌现属性，非 SSE 独有。

## 8. 差分测试基线（本文档的验收面）

| 类别 | 断言 |
|---|---|
| 路由 | 211 条 (method, pathTemplate) 全集对齐；任一请求路径两后端命中同一模板或同返 404/405 |
| 绑定 | 每方法一组金样：query 全字段、嵌套点路径、map 双语法、`[]`、FieldMask 逗号、Timestamp、空值跳过、未知键忽略、类型错误 → 400/CODEC |
| 响应 | 每响应消息类型一组 EmitUnpopulated 金样（含空消息 `{}`、null 消息字段、默认标量、枚举名字符串、64 位整数字符串化） |
| 错误 | 每 reason 一条金样（status + envelope 字节）；未知 reason 场景 → 500/空 reason |
| 头 | Content-Type 恒 `application/json`；CORS 三头（ACAO/ACAM/ACAC）按 §6 彄差 |
| 豁免集 | §1.3 三类路径形态分歧（尾斜杠/cleanPath/%2F）、`head-on-get`（axum GET 过滤器含 HEAD）、`path-bind-post-auth`（§2.3 路径变量绑定的排序分歧，含 `router-shadow`——遮蔽路径落点必然触发的实例） |

> 维护约定：本规格与 Go 侧任何一方变更（升级 Kratos、改 protos）时，先改本文件、再动代码、后跑差分。
