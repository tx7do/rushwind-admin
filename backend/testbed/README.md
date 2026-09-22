# 差分台架（differential rig）

同一语料打两个后端——Go 参照实现（docker 内，`127.0.0.1:27788`）与 Rust 复刻
（宿主 `cargo run`，`127.0.0.1:7788`）——按案例契约比较并产出 JSONL 报告。
语料的判定契约与豁免集锚定 `docs/binding-spec.md` 与 `docs/operator-matrix.md` §5。

## 组件

| 组件 | 位置 | 说明 |
|---|---|---|
| docker-compose | `docker-compose.yml` | bitnami pg/redis + minio（内网名与 go 仓烤死配置一致，宿主不发布端口）+ `go-admin`（go 仓 backend/Dockerfile 多阶段构建，仅绑 127.0.0.1:27788/27789） |
| 回放器 | `backend/crates/admin-diff` | 语料（生成 sweep + 精选）、判定、报告 |
| 豁免集 | `exemptions.json` | class → 依据；四类：`head-on-get`、`path-bind-post-auth`、`router-shadow`（见下） |
| 精选语料 | `corpus/curated.json` | 门拒绝（伪 bearer）、编解码失败（公开 body 路由：CT 缺失/非注册值）、CORS 预检 |
| 报告 | `reports/`（git 忽略） | 每案例一行 JSON + stdout 直方图；Fail 附分歧证据 |

## 语料与判定契约

sweep 从 `admin-gen` 的路由表（211 条，与 Go 注册一一对应）自动生成，
分类按序短路（先生者生效）：

- **遮蔽路由**（恰 1 条：route 16 `GET /admin/v1/apis/walk-route`，`router-shadow`
  → `EnvelopeShape` + 豁免）：gorilla mux 先注册先匹配，该路由被更早注册的
  `GET /admin/v1/apis/{id}` 吸收；axum 静态段优先则反之。生成器已对遮蔽集
  跳过挂载（两侧都落在 {var} 路由上），探针只钉死该等价落点的信封形态；
  axum 由此被迫的「路径变量后绑定 + 缺 token」形态归 `path-bind-post-auth`
  豁免。守卫测试（admin-gen `tests/whitelist.rs`）硬钉遮蔽集内容。
- **门控路由**（194 条，`sweep-gated`）→ `EnvelopeExact`：两侧必须回逐字节相同的
  401/`UNAUTHORIZED`/`missing bearer token` 信封。无凭证探针同时钉死两侧的
  鉴权门语义（含 80 条 body 路由：预绑定层使坏 CT/坏 body 的 400/CODEC 先于
  401 应答，与 Kratos Bind 先于 ctx.Middleware 一致）。
- **公开路由**（8 条，`sweep-public`）→ `Pending`：Rust 侧是 null 桩（500/空
  reason），Go 侧是真实实现——两侧只记录不判定，直到对应模块落地后改契约。
- **HEAD 探针**（GET 路由，89 条，`head-on-get`）→ `Routing` + 豁免：axum 的
  `MethodFilter::GET` 接受 HEAD、gorilla mux 回 405 的已登记微分歧，探针仅量化。
- 路径变量一律替换为 `1`。

精选语料：`gate-invalid`（伪 bearer → 401/`access token expired`，Exact）、
`codec-body`（公开 body 路由的坏 JSON/错型字段 → 400/`CODEC`，Shape：状态 +
code + reason 必须一致，message 文本因两侧解析器措辞不同而归一）、
`cors-preflight`（ACAO/ACAM/ACAC 三头存在性与取值一致性，名单内与名单外各一）。

## 运行

```bash
# 1. 起 go 侧 + 基础设施（go 镜像首次构建需数分钟）
cd backend/testbed
docker compose up -d --build

# 2. 起 Rust 侧（宿主 :7788；服务包 backend/services/admin-api）
cd ../
cargo run -p admin-api

# 3. 回放（在 backend/ 下；报告写入 testbed/reports/report.jsonl）
cargo run -p admin-diff -- --go http://127.0.0.1:27788 --rust http://127.0.0.1:7788
```

默认参数：`--corpus testbed/corpus --exemptions testbed/exemptions.json
--out testbed/reports/report.jsonl --wait 180`（等待两端就绪的秒数；任一端
未就绪则全案例记 `Unreachable`，退出码 2）。

## 评审规则

- `Fail` = 超出已登记豁免的分歧：先对照 go 侧源码定性（是复刻侧 bug → 修
  复 + 回归测试；是 go 侧行为此前未被钉死 → binding-spec/operator-matrix
  补录 → 视语义决定对齐或进豁免集）。
- 新豁免必须附文档锚点（哪个 spec 章节 / 哪条登记），否则视为逃避。
- 报告只记状态码与长度；Fail 详情携带双方证据（信封全文，均为短文本）。

## 已知边界

- **容器纪律**：未经用户明确允许不得启动/触碰 docker 差分栈（2026-09 会话指令）。
  最近一次 run-2 分布为 202 Ok / 2 Fail（均已根因消解：urllib 探针伪影 +
  gorilla 遮蔽）/ 90 豁免 / 9 Pending；Fail=0 的确认复跑顺延到栈可重启时。
- Rust 侧业务实现全部为桩（`sweep-public` 与一切数据面断言待模块落地）；
  SSE（:7789）尚未接入差分。
- `Routing` 判定只区分 matched/unmatched（404/405 = unmatched），不比较
  错误方法探针（除已豁免的 HEAD）；go 侧 gorilla 与 axum 的 404/405 细节
  差异待后续语料扩充。
- 差分流量只打 127.0.0.1；compose 不发布基础设施端口。

## 探针方法学陷阱（复跑前必读）

- **系统代理**：本机 `HTTP_PROXY=127.0.0.1:7890` 会污染 reqwest/curl/urllib
  流量（首轮差分的幽灵 400/404 即源于此）。runner 已用 `.no_proxy()` 硬编码；
  手工探针一律用 python `urllib.request.ProxyHandler({})`（git-bash 的 curl
  即便 `--noproxy` 也不可靠）。
- **urllib 自动 Content-Type**：python urllib 对任何带 `data` 的请求自动注入
  `Content-Type: application/x-www-form-urlencoded`——会伪造「CT 探针」结果
  （曾制造「logout 有时 400 有时 401」的假疑云，参照系行为始终自洽）。带 body
  的探针必须显式设 `Content-Type` 头。
