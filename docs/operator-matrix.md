# 算子矩阵 v1 —— 前端别名 × go-crud × rushwind FilterExpr

> 本文钉死 `query` 参数（Django 别名语法 JSON）在三个前端的别名拼写、go-crud 的
> 枚举映射、gorm 层（postgres 方言）的 SQL 形态、以及 rushwind 契约面
> （`rushwind-storage-proto/src/wire.rs` → `rushwind-storage-seaorm`）的对应路径，
> 并登记别名面的实现现状与分歧。差分台架的回放语料与豁免集以本文为准。
>
> 锚点：别名面实现位于 `rushwind/crates/rushwind-storage-seaorm-support/src/paging.rs`
> （消费 `Params.query`，经 rust-utils `query_parser::parse_filter_json_string` 逐键
> 解析后落 SQL 谓词；部署按 Cargo.lock 钉定的框架 rev 消费）；应用的 `PagingInput`
> 桥在 `backend/apps/admin/src/proto/mod.rs`；protojson 面在
> `rushwind/crates/rushwind-storage-proto/src/wire.rs`；go-crud
> `pagination@v0.0.14/filter/{operator_converter,query_string_converter}.go`；
> gorm 翻译层 `go-crud/gorm@v0.0.24/filter/filter_processor.go`。
> gorm 为 admin 后端活性 ORM 路径（`backend/app/admin/service/internal/data/gorm/*`）。
> 前端发射点归属见 §4（react/element 传输层在抽包后的 npm 包内，vben 在应用仓）。

## 0. 管线位置

**Go 侧（四段）**：URL `query` 参数 → `query_string_converter.Convert`
（顶层数组=AND 组、`$and`/`$or` 递归、键按 `__` 切分为 字段/算子[/datepart|JSON 路由]，
字段名 `ToSnakeCase`）→ `ConverterStringToOperator`（别名 → `paginationV1.Operator` 枚举；
输入经 `ToSnakeCase`+`ToLower` 归一；未知别名 → `OPERATOR_UNSPECIFIED`）→
`FilterCondition{Field, Op, Value|Values, JsonPath?, DatePart?}` →
gorm `Processor.Process` 的 switch（**缺席算子走 default=静默丢弃**；字段经
`isValidFieldExpr` 白名单，不过 → `AddError` → 查询失败）。

**Rust 侧现状（三面均在位）**：
① protojson 面——`FilterExpr` 消息经 pbjson 反序列化为 storage-proto 的
`Operator` 枚举，`wire.rs` 折叠为契约 `Op`（含包装）再由 seaorm 翻译；
② AIP 文本面——`filter` 参数经 `rushwind-storage/src/syntax/text.rs`。
③ **Django 别名面已实现**：`rushwind-storage-seaorm-support/src/paging.rs`
消费 `Params.query`，经 rust-utils `query_parser::parse_filter_json_string`
逐键解析（字段/算子/值回调），`filter_condition` 落 SQL 谓词；应用的
`PagingInput` 桥（`backend/apps/admin/src/proto/mod.rs`）把生成
`PagingRequest` 原样交给管线，List 服务与 repo 调用点不改签名。与本文
原登记「待建别名层」的偏差以 §2 实现现状为准。

## 1. 核心矩阵（按 go-crud 枚举逐行）

分类含义：**direct** = 三列一一对应、SQL 语义一致；**fold** = go 算子在 gorm
(pg) 层坍缩为某基础形态，rust 以 wire.rs 的折叠 + seaorm 的等价渲染对位；
**drop** = gorm 层缺席该 case、条件静默消失，rust 侧须同样丢弃（或现有路径有分歧）；
**reject** = go 实际执行而 rust 契约面报 unsupported —— 真实行为分歧，登记豁免或改 wire。

| 别名拼写（小写归一后逐字） | go 枚举 | gorm/pg SQL | rust 路径（wire.rs→seaorm） | 分类 | 判定 |
|---|---|---|---|---|---|
| `eq` `equal` `equals` | EQ | `f = ?` | `Op::Eq` → `=` | direct | 一致 |
| `ne` `neq` `not` `not_equal` `not_equals` `not-equal` | NEQ | `NOT (f = ?)` | `Op::NotEq` → 否定 | direct | 一致（NULL 行为两侧同排除） |
| `gt` `greater_than` `greater-than` | GT | `f > ?` | `Op::Gt` | direct | 一致 |
| `gte` `greater_than_or_equal` `greater_equals` `greater_or_equal` `greater-or-equal` | GTE | `f >= ?` | `Op::Gte` | direct | 一致 |
| `lt` `less_than` `less-than` | LT | `f < ?` | `Op::Lt` | direct | 一致 |
| `lte` `less_than_or_equal` `less_equals` `less_or_equal` `less-or-equal` | LTE | `f <= ?` | `Op::Lte` | direct | 一致 |
| `like` | LIKE | **无 case → 丢弃** | `Op::Like` → `LIKE ?` | drop | **分歧**：别名层须丢弃 |
| `ilike` `i_like` | ILIKE | `f ILIKE ?`（裸值=大小写不敏感等值） | `Op::Ilike` → `LOWER(f) LIKE LOWER(v)` | direct | 语义等价（ci-等值） |
| `not_like` `notlike` | NOT_LIKE | **无 case → 丢弃** | `Op::NotLike` → `NOT LIKE` | drop | **分歧**：别名层须丢弃 |
| `in` | IN | `f IN (…)`（值=JSON 数组字符串或 values 多值） | `Op::In` | direct | 一致（值通道按 §3 对齐） |
| `nin` `not_in` `notin` | NIN | `f NOT IN (…)` | `Op::NotIn` | direct | 一致 |
| `is_null` `isnull` | IS_NULL | `f IS NULL` | `Op::IsNull` | direct | 一致 |
| `is_not_null` `isnot_null` `isnotnull` `not_isnull` | IS_NOT_NULL | `f IS NOT NULL` | `Op::IsNotNull` | direct | 一致 |
| `between` `range` | BETWEEN | `f >= ? AND f <= ?`（两值） | `Op::Between` | direct | 一致 |
| `contains` | CONTAINS | `f LIKE '%v%'` | `Op::Contains` → seaorm 自包装 `%v%` | direct | 一致（包装在 seaorm 层） |
| `icontains` `i_contains` | ICONTAINS | pg：`f ILIKE '%v%'` | wire：`Icontains → Op::Ilike + wrap('%v%')` → seaorm LOWER+LIKE | fold | 一致 |
| `starts_with` `startswith` | STARTS_WITH | `f LIKE 'v%'` | `Op::StartsWith`（seaorm 包装 `v%`） | direct | 一致 |
| `istarts_with` `i_starts_with` `istartswith` | ISTARTS_WITH | pg：`f ILIKE 'v%'` | wire：`IstartsWith → Op::Ilike + wrap('v%')` | fold | 一致 |
| `ends_with` `endswith` | ENDS_WITH | `f LIKE '%v'` | `Op::EndsWith`（seaorm 包装 `%v`） | direct | 一致 |
| `iends_with` `i_ends_with` `iendswith` | IENDS_WITH | pg：`f ILIKE '%v'` | wire：`IendsWith → Op::Ilike + wrap('%v')` | fold | 一致 |
| `exact` | EXACT | `f = ?`（与 EQ **完全同义**，gorm 注释自认） | wire：`Exact → Op::Eq` | fold | 一致 |
| `iexact` `i_exact` | IEXACT | pg：`f ILIKE ?` 裸值（ci 等值） | wire：**unsupported 报错** | **reject** | **分歧**：Go 执行 / Rust 报错。可折叠（`Ilike` 裸值语义等价）但 wire.rs 现拒绝——待决策（§5-D2） |
| `regexp` `regex` | REGEXP | pg：`f ~ ?` | **unsupported 报错** | **reject** | **分歧**（§5-D3） |
| `iregexp` `i_regexp` `iregex` | IREGEXP | pg：`f ~* ?` | **unsupported 报错** | **reject** | **分歧**（§5-D3） |
| `search` | SEARCH | pg：`to_tsvector(f) @@ plainto_tsquery(?)` | **unsupported 报错** | **reject** | **分歧**（§5-D3） |
| `json_contains` | JSON_CONTAINS | **无 case → 丢弃** | **unsupported 报错** | drop/reject 冲突 | **分歧**：Go 静默丢弃 vs Rust 报错（§5-D1） |
| `array_contains` | ARRAY_CONTAINS | 同上 | 同上 | 同上 | 同上 |
| `exists` | EXISTS | 同上 | 同上 | 同上 | 同上 |

Rust 契约独有、无任何别名可达：`Op::NotBetween`（go 无 NOT_BETWEEN 枚举、
无别名拼写 → 不可达冗余，登记不动）。
`Operator::Unspecified`（未知别名的落点）：wire.rs 报错；Go 侧条件被附加后由
gorm default **静默丢弃** → 别名面对未知别名已按静默丢弃对位（§2 的 `_ =>`
落空臂）。

## 2. 实现现状（别名面：rushwind-storage-seaorm-support/src/paging.rs）

别名面已落地，落点与原登记不同：不在 admin 服务层，而在框架 crate 的分页
管线（`fetch_paged`/`apply`）。应用 List 服务把生成 `PagingRequest` 原样
传给 `repo_shell!` 的 `paged_list`，`PagingInput` 桥（admin 的
`src/proto/mod.rs`）解析出 `Params{query, orderBy, sorting, page, …}`，
管线对 `Params.query` 走 rust-utils `query_parser::parse_filter_json_string`
逐键解析后由 `filter_condition` 落 SQL 谓词。与 §1/原规范的偏差如实登记：

- **iexact 实际执行**：`FILTER_IEXACT` 命中 `lower_like`（`LOWER(col) LIKE
  LOWER(?)`，ci 等值）——原登记「unsupported 报错」（§5-D2 的 reject 面）
  在别名面不成立；protojson 面（wire.rs，按钉定 rev）仍拒绝该四别名，§1
  矩阵行保留。
- **regexp/iregex/search 及未知别名静默丢弃**：`filter_condition` 对
  `FILTER_REGEX | FILTER_IREGEX | FILTER_SEARCH` 与 `_ =>` 落空臂一律
  `return None`（条件消失、查询继续）——原登记「报错」不成立；gorm 层对
  absent case 同为静默丢弃，两侧对位（§5-D1/D3 在别名面随之消解）。
- **三段键取末段的 go-crud 语义未实现**：实现复用 rust-utils
  `query_parser::parse_filter_field`，其固定取 `parts[1]` 为算子——
  `f__x__op2` 按「算子=第二段」解析，datepart/JSON 路由的第三段语义缺席。
  活性面不受影响（前端追写的 `f__contains` 恰在 parts[1] 命中，见 §4 附注），
  其余三段键为休眠分歧。
- **丢弃集（静默）**：`like`/`not_like`/`notlike`/`json_contains`/
  `array_contains`/`exists` 无专属 FILTER_* 常量，全部落 `_ =>` 臂静默丢弃，
  与原规范的丢弃集一致。
- **键分割**：1 段（无 `__`）→ EQ（空算子臂）；2 段 `f__op` → 算子段；
  顶层数组=AND 组、值多值经 `split_query_values` 进 IN/NOT IN/RANGE。
- **数值列守卫**：数值列遇非数值字面量（前端回显 "undefined"）丢条件而非
  毒化查询；列类型读自实体 schema（number/bool/text），未知列按 text 绑定。
- **过滤轴无列白名单前置拦截**（排序轴有 `known` 检查只按实体列排序）：
  未知过滤列会带着 SQL 报错使查询失败——与 gorm `isValidFieldExpr`
  白名单失败的「查询失败」结局对位，机制不同（前置 AddError vs SQL 错误）。
- **分页/排序**：orderBy JSON 数组或逗号串、sorting 结构化列表、缺省
  `id` 升序兜底；total 与行集共享同一谓词集（过滤计入 total）。

## 3. 结构特性矩阵

| 特性 | Go（query_string_converter + gorm） | rust 现状/别名面行为 |
|---|---|---|
| 顶层数组 | = AND 组；子数组扁平化进当前 AND 组 | `parse_filter_json_string` 顶层数组逐键回调（AND 语义）；别名面同 |
| `$and`/`$or` 键 | 互斥（同持两者→错误）；值须数组；递归成 ExprType_AND/OR 组 | 无递归处理：该拼写按字面字段名进 `to_snake_case`（别名面不支持该结构；顶层 AND 组用数组表达） |
| JSON 路径键（`a.b`） | EQ+JsonPath 条件；gorm 侧 `JsonbFieldExpr` 列表达式 + 白名单 | 无对应；键原样走 `to_snake_case`（无 JSON 路由语义），未知列落 SQL 错误（§2） |
| 多值（值=JSON 数组） | → `Values`（IN/NIN/BETWEEN 消费） | `split_query_values` 多值进 IN/NOT IN/RANGE |
| 三段键 datepart | DatePart 载荷；**gorm Process 不消费（惰性，仅测试引用）** | 别名面取 parts[1] 为算子（§2），datepart 第三段语义缺席；差分豁免登记 |
| 空字段/空算子段 | 丢弃该键 | 同（`parse_filter_field` 空字段不回调） |
| 字段白名单不过 | gorm `AddError` → 查询失败 | 过滤轴未知列带 SQL 错误失败查询（机制对位，见 §2）；排序轴 `known` 检查静默跳过 |
| 值字符串化 | `AnyToString` | 同（数值/布尔保持原类型按 wire.rs `value_to_contract`） |
| `orderBy` / `fieldMask` / `page`/`pageSize`/`noPaging` | 走 proto 字段绑定（本仓生成绑定计划），不属别名层 | 已覆盖（绑定计划 + 管线分页/排序） |

## 4. 线上活性面（三前端实测）

前端进入「抽包时代」后的发射点归属（2026-11 实测）：react 与 vue-element
的传输层在随仓快照外置的 npm 包内——react 应用仓的 `src/api/client.ts` 从
`@go-wind/admin-react` 引 `requestApi`（query 组装随之进包），vue-element
同构、包为 `@go-wind/admin-core`；vue-vben 的发射点在本仓应用内
`frontend/admin/vue-vben/src/utils/query.ts` 的 `makeQueryString`（仍对
字符串值追写 `__contains`）。三端不是同一份代码，但 query 发射语义同源
（同一 go-crud 约定的移植），`makeQueryString` 实际发射：

- **裸键 EQ**：数值/布尔值；字符串但字段名命中 ID 豁免正则
  `/(_id$|Id$|ID$|^id$)/`；键已带算子后缀（`hasOperatorSuffix`，
  其表 = go-crud operatorMap 键集的小写镜像）→ 原样透传。
- **`__contains`**：其余全部字符串值（模糊匹配约定）。
- `needCleanTenant` 时剔除 `tenant_id`/`tenantId`；清理后空对象 → 不发 query。
- `orderBy` 缺省 `['-created_at']`（JSON 数组）；`noPaging` = paging 缺失时为 true。

⇒ **线上活性算子集 = {EQ（裸键）, CONTAINS（`__contains`）}**。矩阵其余行均处
防御/休眠面：只有手工构造或前端改版才会触达，由差分回放语料负责钉死两侧一致。

附注（无对位影响）：前端守卫只做 `toLowerCase`，go 侧还过 `ToSnakeCase`
（如 `notEqual`→`not_equal` 在 Go 命中、在前端守卫表 miss）→ 前端对这类拼写会
追加以 `__contains` 结尾的三段键，Go 侧三段语义取末段算子=contains——该行为对
两个后端字节相同，不构成跨后端分歧，但回放语料应包含一例以钉住。

## 5. 分歧登记（差分豁免集种子）

- **D1（丢弃 vs 报错）**：`json_contains`/`array_contains`/`exists` 与未知别名
  ——Go 静默丢弃、rust 契约面报 unsupported。别名层按"静默丢弃"对位 Go；
  protojson 面（wire.rs 报错）与 Go 的 protojson 面差异另记（Go 的 protojson 面
  同样进 gorm switch → 同样丢弃，故 wire.rs 的报错是**跨后端真实分歧**，若
  FilterExpr 消息可从请求体到达需豁免或改 wire）。
- **D2（iexact）**：Go 执行（pg ILIKE 裸值=ci 等值），wire.rs 拒绝。技术上
  `Iexact → Ilike`（裸值）为语义精确折叠；是否推动 wire.rs 修改属 rushwind 仓
  决策，默认按拒绝+豁免登记。
- **D3（正则/全文）**：`regexp` 族与 `search` —— Go 在 pg 上真实执行
  （`~`/`~*`/tsvector），rust 无对应算子。活性面前端不发射；默认拒绝+豁免登记。
- **D4（`like`/`not_like`）**：go 枚举存在但 gorm 丢弃；rust 有真实 Op。别名层
  必须丢弃（§2 丢弃集），否则产生 Go 不存在的过滤行为。
- **D5（错误面）**：字段白名单/校验失败的两通道错误（gorm AddError vs
  StorageError）在状态码与包络上的差异 —— 存储阶段差分钉死后回填。

## 6. 与计划的联动

- 本文档满足 Phase 0 清单第 6 项（算子矩阵 v1）。
- §2 实现现状即服务层 `query` 参数的落地契约（别名面在框架分页管线，应用侧零转换组件）。
- §5 种子并入差分台架的豁免集（台架落地时引用本文节号）；D1/D3 的豁免面以
  §2 偏差登记为准（别名面静默丢弃已对位，protojson 面仍按钉定 rev 拒绝）。
