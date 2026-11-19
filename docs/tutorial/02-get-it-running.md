# 02 · 从零跑起来

> 前置：[01 架构全景](./01-architecture-overview.md)。读完你会得到：一个能登录、能调试的本地环境（中间件 + 后端 + 前端）。

## 1. 环境要求

| 工具 | 版本 / 说明 |
|---|---|
| Rust | stable（workspace `rust-version = 1.81`），需带 rustfmt / clippy 组件 |
| buf | **必须**在 PATH 上——注解闭包由 `buf build` 编译（安装：`curl -fsSL https://buf.build/install.sh \| sh`，或 GitHub releases 单二进制） |
| bash | 运行同步 / 台架脚本（Windows 推荐 Git Bash） |
| Docker | 20.0+（本地中间件） |
| Node.js + pnpm | Node ≥ 20.19.0，pnpm ≥ 10.0.0 |

**依赖解析说明**：构建不依赖任何本地兄弟仓——框架（rushwind）与工具库（rust-utils）以 Git 依赖按 revision 钉死（15 个 rushwind-* crate，见 [01 章 §1.2](./01-architecture-overview.md)），clone 本仓即可构建。只有两种情况需要本地 checkout 框架仓：给框架本身提改动、或本地双仓联调（workspace 层用未提交的 `[patch]` 段指向本地路径，属于框架开发者工作流）。

### 1.1 版本从哪核验、怎么装

- **Rust 1.81**：`backend/Cargo.toml` 的 `[workspace.package] rust-version = "1.81"`——低于它的工具链直接拒绝编译。`rustup default stable` 即可（stable 已远超 1.81）；rustfmt/clippy 组件由 `rustup component add rustfmt clippy` 补齐（质量门 §6 要用）。
- **buf 为什么必须**：构建引擎直接消费 `buf build` 的描述符产物，Rust 侧序列化器会丢弃自定义选项字节——选项字节正是路由与错误码的载体（[03 章 §2](./03-codegen-chain.md) 展开机制）。没装 buf 的症状是 `cargo build` 失败在 build script 阶段、报错以 `buf:` 开头。验证：`buf --version`。
- **Node / pnpm 的确切基线来自三份前端各自的声明**（版本取交集即可一劳永逸）：
  - `vue-element/package.json` engines：`node ^20.19.0 || >=22.12.0`，`packageManager` 钉 `pnpm@10.34.9`；
  - `vue-vben/package.json` engines：`node >=20.11.0`、`pnpm >=9.12.0`，`packageManager` 钉 `pnpm@11.18.0`；
  - react 快照无 engines 硬钉，随上两者取稳态。
  一个 pnpm ≥ 10（corepack 启用：`corepack enable`）即可三快照通吃。
- **bash**：Windows 上推荐 Git Bash——`sync-protos.sh`、前端同步脚本、台架脚本都是 bash；CI 的 ubuntu/windows 矩阵同样按 bash 跑。

### 1.2 Windows 构建特别提示：AWS_LC_SYS_PREBUILT_NASM=1

Rust 侧有一条你没直接声明的传递依赖链：JWT 引擎链 `rushwind-authn-jwt → jsonwebtoken 11.1.0 → aws-lc-rs → aws-lc-sys 0.47.0`。`aws-lc-sys` 在 Windows MSVC 上从源码构建时要走 NASM 汇编——本机没装 NASM 的话，构建会在 `aws-lc-sys` 的 build script 处失败（报错指向汇编器/构建脚本，而非任何你写过的代码）。

**处置**：让 aws-lc-sys 使用预编译的 NASM 目标文件——把环境变量前置到每个 cargo 命令（或写进用户级环境变量一劳永逸）：

```bash
# Git Bash / WSL
export AWS_LC_SYS_PREBUILT_NASM=1
cargo run -p admin
```

```powershell
# PowerShell
$env:AWS_LC_SYS_PREBUILT_NASM = "1"
cargo run -p admin
```

```bat
:: cmd.exe
set AWS_LC_SYS_PREBUILT_NASM=1 && cargo run -p admin
```

本文其余 cargo 命令均不再重复此前缀；Windows 上照此自行带上。同一链路上的另一颗雷已被 workspace 替你拆掉：`[profile.dev] debug = "line-tables-only"`（MSVC PDB 4GB 上限会令全量调试信息链接爆 `LNK1140`，见 [01 章 §1.1](./01-architecture-overview.md)）——正常照抄命令即可，不要手动往 dev profile 加回全量调试信息。

## 2. 起本地中间件

后端启动即连接 PostgreSQL 与 Redis，缺任一样启动即退出；MinIO 是软依赖——没有 oss 配置时对象存储面不启用，服务照常启动：

```bash
docker run -d --name rw-pg  -p 5432:5432 -e POSTGRES_PASSWORD=*Abcd123456 -e POSTGRES_DB=rushwind_admin postgres:16
docker run -d --name rw-redis -p 6379:6379 redis:7 redis-server --requirepass "*Abcd123456"
docker run -d --name rw-minio -p 9000:9000 -e MINIO_ROOT_USER=root -e MINIO_ROOT_PASSWORD=*Abcd123456 minio/minio server /data
```

三个容器的凭据刻意与内嵌 yaml 烤死的值逐字一致（PG 密码 `*Abcd123456`、库名 `rushwind_admin`；Redis 密码 `*Abcd123456`；MinIO `root` / `*Abcd123456`）——这样后文的环境变量覆盖只有地址是"新信息"，其余凭照旧。注意 Redis 的 `--requirepass` 不能省：内嵌配置带密码，redis 客户端按密码发 AUTH，无密码的 Redis 会答 WRONGPASS/NOAUTH 形态的错误。

> ⚠ **Windows + Docker Desktop 的 IPv6 陷阱**：Docker 的 IPv6 回环端口代理可能挂死——`::1:5432` 超时而 `127.0.0.1:5432` 瞬通。下面给后端的环境变量一律写 `127.0.0.1`，不要写 `localhost`（Rust/tokio 解析 `localhost` 会优先走 `::1`，表现为连接池超时的假象）。

### 2.1 凭据对照表：容器 ↔ 内嵌配置

后端的中间件地址烤在 `apps/admin/assets/{data,oss}.yaml` 里，指向**容器主机名**（`postgres:5432` / `redis:6379` / `minio:9000`）——那是容器内互联的名字，本机直跑解析不到，所以必须用环境变量覆盖地址（§3）。逐项对照：

| 中间件 | 容器（yaml 烤死值） | 本机直跑覆盖 | 凭据（两边一致） |
|---|---|---|---|
| PostgreSQL | `host=postgres port=5432 dbname=rushwind_admin` | `RUSHWIND_DATABASE_SOURCE=postgres://...@127.0.0.1:5432/rushwind_admin` | `postgres` / `*Abcd123456` |
| Redis | `redis:6379` | `RUSHWIND_REDIS_ADDR=127.0.0.1:6379` | 密码 `*Abcd123456` |
| MinIO | `minio:9000` | `RUSHWIND_OSS_ENDPOINT=127.0.0.1:9000` | `root` / `*Abcd123456` |

### 2.2 这不是 testbed 的 compose

仓里另有一份 `backend/testbed/docker-compose.yml`——**那是差分回归台架的栈，不是本地开发栈**（[03 章 §4](./03-codegen-chain.md) 展开）。读它的头部注释就知道关键差异：它拉起 pg/redis/minio（bitnami 镜像、库名 `gwa`）**加上 Go 参照后端**（`127.0.0.1:27788/27789`），且基础设施三件**一律不发布宿主端口**——`docker compose up` 之后 `127.0.0.1:5432` 是关着的，拿它喂本地开发后端只会得到连接超时。本地开发就用上面三条 `docker run`；testbed compose 留给契约/绑定改动的逐字节回归。

## 3. 起后端

```bash
cd backend
cargo run -p admin        # 二进制 admin，监听 REST :7788 + SSE :7789（业务模块面同进程）
```

- 首次构建需数分钟；**启动到监听约 24–50 秒属正常**——启动序列包含迁移（61 张表：admin 面 60 张 + fork 面 1 张，`database.migrate` 门开启时）和种子数据（内置菜单、权限点、平台参数、默认语言、平台管理员）。
- 配置内嵌在 `apps/admin/assets/`（`server.yaml` / `auth.yaml` / `data.yaml` / `oss.yaml`），其中中间件地址指向容器主机名（`postgres:5432` 等）——**本机直跑必须用环境变量覆盖**：

```bash
export RUSHWIND_DATABASE_SOURCE="postgres://postgres:*Abcd123456@127.0.0.1:5432/rushwind_admin?sslmode=disable"
export RUSHWIND_REDIS_ADDR="127.0.0.1:6379"
export RUSHWIND_REDIS_PASSWORD="*Abcd123456"
export RUSHWIND_OSS_ENDPOINT="127.0.0.1:9000"
cargo run -p admin
```

注意两点：`RUSHWIND_DATABASE_SOURCE` 必须是 URL 形式（sqlx 不认 libpq key=value 串）；本机开发库通常关 TLS，追加 `?sslmode=disable`。全部环境变量清单见[第 8 章](./08-deployment.md)。

### 3.1 配置四 yaml 逐键注解

四份 yaml 经 `include_str!` 编进二进制（`config.rs` / `main.rs`），改它们要重新编译。逐文件逐段：

**`server.yaml` —— 装配文档（生命周期驱动，`main.rs` 的 `SERVER_YAML`）**

| 段 | 键 | 含义 |
|---|---|---|
| `app` | `name: admin` / `version` | 进程身份，进日志与指标 |
| `servers[0]`（http） | `bind: ":7788"` | REST 监听地址 |
| | `edge.timeout: 10s` | 请求预算：单请求全链路 10s，超时按该层规则回信封 |
| | `edge.cors.credentials: true` | 前端 refresh token 走 HttpOnly Cookie（`withCredentials=true`），预检必须带 `Access-Control-Allow-Credentials` |
| | `edge.cors.compat: true` | 选择 gorilla 兼容发射层（`rushwind_http::cors_compat`）——发射规则与参照系逐字节一致，tower-http 复现不了 |
| | `edge.cors.headers/methods/origins` | 允许的头（含 `X-Captcha-Id`/`X-Captcha-Value`）、动词（GET/POST/PUT/DELETE/HEAD/OPTIONS）、源（三个演示域名 + 若干本地端口清单） |
| | `route_packs: [admin-surface, business-surface]` | 同一台 http server 上的两个路由包；`admin-surface` 带 `settings: enable_swagger/enable_redoc`（文档面开关），`business-surface` 不带（模块面无 OpenAPI 资产） |
| `servers[1]`（admin-sse） | `addr: ":7789"` / `path: "/events"` | SSE 独立监听与路径 |
| `servers[2]`（admin-tasks） | （无参数段） | 任务队列 worker，factory transport，无监听端口 |
| `servers[3]`（cron） | `endpoint: cron://admin` + `jobs: [tenant_expiry_scan, audit_log_archive, sys_tasks_periodic]` | cron 生产者；真实排程在 `server/apalis.rs`：`"0 * * * *"`（每小时）、`"30 3 * * *"`（每日 03:30）、`"* * * * *"`（每分钟，sys_tasks 表驱动） |

**`data.yaml` —— 存储面（`config.rs` 解析）**

| 键 | 值 | 含义 |
|---|---|---|
| `database.driver` | `postgres` | 驱动声明 |
| `database.source` | `host=postgres port=5432 user=postgres password=*Abcd123456 dbname=rushwind_admin sslmode=disable` | 容器网络 DSN（libpq 形态）——**本机直跑必须整串换成 URL 形态的 `RUSHWIND_DATABASE_SOURCE`** |
| `database.migrate` | `true` | 启动迁移门：true 时启动序列先跑迁移链（§3.2 第 5 步）；`RUSHWIND_DATABASE_MIGRATE`（`"true"`/`"1"`）可覆盖 |
| `database.debug / enable_trace / enable_metrics` | `false` | SQL 调试打印 / 追踪 / 指标开关 |
| `database.max_idle_connections / max_open_connections` | `25` / `25` | 池上限（`AppState::connect` 同步把 `max_connections`/`min_connections` 都设 25，连接超时 10s） |
| `database.connection_max_lifetime` | `300s` | 连接最长寿命 |
| `redis.addr / password` | `redis:6379` / `*Abcd123456` | 容器主机名 + 密码——本机直跑覆盖地址，密码沿用 |
| `redis.dial_timeout / read_timeout / write_timeout` | `10s` / `0.4s` / `0.6s` | Redis 拨号与读写超时 |

**`auth.yaml` —— 认证面（`config.rs` 解析 jwt 段）**

| 键 | 值 | 含义 |
|---|---|---|
| `authn.type` | `jwt` | `jwt` / `oidc` / `preshared_key` 三选一，当前部署走 jwt |
| `authn.jwt.method` | `RS256` | 非对称验签（对称 HS256 等用 `key`，RS256 下 `key` 留空） |
| `authn.jwt.access_token_expires` | `5400s`（1.5 小时） | 优先级：`RUSHWIND_ACCESS_TOKEN_EXPIRES_SECS` > 此值 > 代码默认 900s |
| `authn.jwt.refresh_token_expires` | `43200s`（12 小时） | 优先级：`RUSHWIND_REFRESH_TOKEN_EXPIRES_SECS` > 此值 > 代码默认 7 天 |
| `authn.jwt.private_key / public_key` | 内置开发密钥对（PEM） | 验签引擎在 `main.rs` 用公钥构建，签发引擎在 `state.rs` 用私钥构建——**私钥缺失启动即退**（`auth.yaml private_key missing`）。文件内注释给了 openssl 重新生成命令；生产必须换独立密钥对 |
| `authn.oidc / preshared_key`、`authz.*` | 占位段 | 当前部署未启用的可选面，保留声明形状 |

**`oss.yaml` —— 对象存储面（`config.rs` 解析，MinIO 段）**

| 键 | 值 | 含义 |
|---|---|---|
| `minio.endpoint` | `minio:9000` | S3 引擎的端点；`RUSHWIND_OSS_ENDPOINT` 只覆盖这一项（`config.rs` 明示"Host-side runs reach the published port"）。endpoint 非空即启用对象存储面（随仓 yaml 恒非空，故默认面总是启用，连不上 MinIO 只影响上传下载，不影响启动）；**想彻底禁用，把 yaml 里的 minio 段置空后重编译** |
| `minio.upload_host / download_host` | `minio:9000` / `minio:9000` | 上传/下载回址：文件服务的下载 URL 由 `download_host` 拼接（`services/file/transfer.rs`）——本机真要跑通上传下载，需把这两个键也改成 `127.0.0.1:9000` 后重编译 |
| `minio.access_key / secret_key` | `root` / `*Abcd123456` | 与 `docker run` 的 `MINIO_ROOT_*` 一致 |
| `minio.use_ssl` | `false` | 本地 MinIO 走明文 |

启动时按这五桶（`images`/`videos`/`audios`/`docs`/`files`）各建一个 S3 引擎并 best-effort 建桶；无 MinIO 段时文件服务落本地磁盘镜像（`./data/files/<bucket>`，元数据语义一致）。

**环境变量覆盖清单（本节用到的全集）**——每个变量优先于 yaml，yaml 优先于代码默认：

| 变量 | 覆盖点 | 备注 |
|---|---|---|
| `RUSHWIND_DATABASE_SOURCE` | `data.database.source` | 必须 URL 形态；本地开发库追加 `?sslmode=disable` |
| `RUSHWIND_DATABASE_MIGRATE` | `data.database.migrate` | `"true"` / `"1"` |
| `RUSHWIND_REDIS_ADDR` / `RUSHWIND_REDIS_PASSWORD` | `data.redis.addr` / `.password` | 地址写 `127.0.0.1`（IPv6 陷阱） |
| `RUSHWIND_OSS_ENDPOINT` | `oss.minio.endpoint` | 仅 endpoint；upload/download host 不随它变 |
| `RUSHWIND_AUTH_JWT_PRIVATE_KEY` / `_PUBLIC_KEY` | `authn.jwt.private_key` / `.public_key` | PEM 全文 |
| `RUSHWIND_ACCESS_TOKEN_EXPIRES_SECS` / `RUSHWIND_REFRESH_TOKEN_EXPIRES_SECS` | 令牌有效期 | 秒数 |
| `RUSHWIND_CAPTCHA_REQUIRED_FAILURES` | 渐进式验证码阈值 | 默认 3，`clamp(1, 4)` |
| `RUSHWIND_PASSWORD_RSA_PRIVATE_KEY` | 口令传输信封解密私钥 | 不设 = 内置开发私钥，启动时打警告（§3.2 第 2 步） |

### 3.2 启动序列逐步（含每步失败的典型报错）

`main.rs` 从上到下是一条固定序列，每步的失败形态都值得提前认识——报错文本即锚点：

1. **`Config::load()`** —— 解析三份内嵌 yaml（data/auth/oss）。失败形态：`parse data.yaml: ...`（配置损坏，重编译前先查 yaml 语法）。server.yaml 不在这里解析——它由装配器消费（第 8 步）。
2. **口令信封私钥检查** —— `using_dev_password_private_key()` 为真时向 stderr 打一行警告："RUSHWIND_PASSWORD_RSA_PRIVATE_KEY 未配置，口令传输解密仍在使用内置开发私钥"。本地开发**这是正常输出不是故障**；生产必须配置。
3. **JWT 验签引擎** —— 公钥取 `RUSHWIND_AUTH_JWT_PUBLIC_KEY` 或 auth.yaml，缺省再退到随仓 `assets/jwt_public_key.pem`；算法钉 RS256。失败形态：`jwt public key: ...`（PEM 损坏）。
4. **`AppState::connect`**（`state.rs`）—— 依序：Postgres 连接池（25/25、10s 超时）→ Redis 连接管理器 → RS256 签发引擎（私钥来自 auth.yaml 或 env，**缺失即退**）→ `TokenStore`（Redis 会话存储，带两个有效期）→ 可选的五桶 OSS 引擎。失败形态逐字对应：`postgres connect: <sqlx 错误>`（中间件没起/端口错/凭据错/没写 127.0.0.1）、`redis connect: <错误>`（地址/密码不一致——§2 的 `--requirepass`）、`auth.yaml private_key missing (RS256 minting requires it)`。
5. **迁移链**（`database.migrate` 门开时，`migration.rs`）—— 先 admin 链（`Migrator`：9 个实体派生段 + 13 个手写 DDL 迁移，60 张表），再 template 链（fork 面 1 张表）。执行是**逐步应用**（每次一个迁移），成功打 `[migrate] applied <迁移名>`；失败打 `[migrate] failed right after <上一个成功的迁移名>` 并以 `schema migrate: <错误>` 退出——PG 的 Query Error 只给事务内行号，这两行日志是定位哪个迁移哪条 DDL 的关键。已迁过的库直接跳过（迁移追踪表挡住重放，幂等）。
6. **种子**（`seed/mod.rs`）—— 固定顺序：语言（7 条，zh-CN 默认）→ 平台参数（3 个密码策略键）→ 角色（`platform:admin` / `template:tenant:manager`）→ 权限组（5）→ 权限点（5，角色授权按 code 动态解析）→ 菜单（30 条固定 ID）→ 通知规则（4 条事件路由）→ 内置 BI 模板（1 条）→ 平台管理员（`admin` / `Abcd@1234`）→ 启动期接口同步（把打进二进制的 OpenAPI 端点清单按 `(path, method)` MERGE 进 `sys_apis`）→ 16 张以固定 id 播种的表做序列对齐（否则首个 API 侧 INSERT 撞主键）。**全幂等**：每段先查计数，非空即跳过。
7. **任务队列句柄** —— `TaskQueue::new(&cfg.database_source, "default")`：worker 与 cron 生产者共用同一 Postgres 队列存储，业务模块直接复用。
8. **装配器组装** —— `Bootstrap::from_yaml_str(SERVER_YAML)`，`.route_pack("admin-surface", rest::pack(...))` + `.route_pack("business-surface", template::pack(...))`（同一 http server、同一 edge、同一审计层），`.cron_job(...)` × 3，`.server_factory("admin-sse", ...)` + `.server_factory("admin-tasks", ...)`。
9. **`build().await` → `app.run()`** —— 生命周期持有 OS 信号关停路径；四个 transport 就绪后进入监听，此刻起 24–50 秒的等待结束。

典型报错 → 处置速查：

| 报错（逐字锚点） | 根因 | 处置 |
|---|---|---|
| `postgres connect: ...` | PG 未起 / 地址写成 localhost / 密码不符 / 非 URL 形态 | §2 的 docker 命令核对 + IPv6 陷阱 |
| `redis connect: ...` | Redis 未起 / 密码不一致（requirepass） | `--requirepass` 与 `RUSHWIND_REDIS_PASSWORD` 两边一致 |
| `auth.yaml private_key missing ...` | 私钥段被清空且未设 env | 还原 auth.yaml 或 `RUSHWIND_AUTH_JWT_PRIVATE_KEY` |
| `schema migrate: ...` + `[migrate] failed right after <name>` | 某迁移 DDL 失败 | 按 `<name>` 定位迁移；本地库可 DROP 后重来 |
| 构建期即失败，`buf: ...` 开头 | buf 未装 / 不在 PATH | §1.1 安装并验证 `buf --version` |
| 构建期 aws-lc-sys 失败（Windows） | 本机无 NASM | §1.2 的 `AWS_LC_SYS_PREBUILT_NASM=1` |

## 4. 验证后端

```bash
# 1) 门控路由未带令牌 → 统一四字段错误信封（code/reason/message/metadata）
curl -s http://127.0.0.1:7788/admin/v1/users
#   {"code":401,"reason":"UNAUTHORIZED","message":"missing bearer token",...}

# 2) 内嵌 OpenAPI 规格（文档端点三件套均免鉴权）
curl -s -o /dev/null -w "%{http_code} %{size_download}\n" http://127.0.0.1:7788/q/openapi.yaml
```

浏览器打开 <http://127.0.0.1:7788/q/swagger-ui>（Swagger UI）或 `/q/redoc`（ReDoc）——由 `server.yaml` 的 `enable_swagger` / `enable_redoc` 开关控制，UI 静态资源走 CDN，需要外网。

第三个检查点探 **SSE 通道**（独立监听 :7789，连接鉴权独立于 REST 的门）：

```bash
# 无令牌探活 → 401：服务在线、门在工作（这正是预期形态）
curl -s -o /dev/null -w "%{http_code}\n" http://127.0.0.1:7789/events
# 带有效 access token → 事件流打开（-N 禁缓冲；token 三来源按序：
# Authorization: Bearer 头 → X-Token 头 → ?token= 查询参数）
curl -N -H "Authorization: Bearer <access token>" http://127.0.0.1:7789/events
```

登录本身不建议用 curl 直接打 `AuthenticationService/Login`（口令走 RSA 混合信封传输，curl 手拼信封得不偿失）——走前端登录页，见下节。若一定要 API 级冒烟，`/q/openapi.yaml` 里搜 `Login` 可看到请求形状。

## 5. 起前端并登录

```bash
cd frontend/admin/react
pnpm install
pnpm dev                   # :15888，dev 代理把 /admin 转发到 :7788
```

浏览器打开 <http://localhost:15888>，用种子管理员 **`admin` / `Abcd@1234`** 登录（租户编号留空 = 平台登录）。登录链路的验证码是**渐进式**的：默认没有验证码，同一 ip+用户名组合登录失败累计达到 `captcha_required_failures`（默认 3，可用 `RUSHWIND_CAPTCHA_REQUIRED_FAILURES` 调整）后，后续登录才要求图形验证码——缺头回 `CAPTCHA_REQUIRED`（登录页据此点亮验证码框）、错码回 `CAPTCHA_INVALID`、触发限流锁定回 `TOO_MANY_ATTEMPTS`（1161）、租户无效回 `INVALID_TENANT`（6）；登录成功后失败计数清零。通过验证码后依次是口令应用层加密传输与 JWT 签发。进仪表盘看到真实统计即打通。

若端口 15888 被占，vite 会自动顺延，以启动日志为准。代理目标写在 `frontend/admin/react/.env.development` 的 `VITE_PROXY`，默认已指向 127.0.0.1:7788，零配置直连本仓后端。

### 5.1 三快照与代理（.env.development 为准）

三套快照的 dev 端口与代理形状各写在自己的 `.env.development` / vite 配置里，逐份注解：

| 快照 | dev 端口（.env 键） | API 代理 | SSE 地址 |
|---|---|---|---|
| `react` | **15888**（`VITE_SERVER_PORT`） | `VITE_PROXY='[["/admin", "http://127.0.0.1:7788/"]]'`——按 `/admin` 前缀转发，**不 rewrite**（后端路由自带 `/admin/v1`） | `VITE_SSE_URL=http://localhost:7789/events` |
| `vue-element` | **15777**（`VITE_APP_PORT`） | `VITE_APP_BASE_API=/admin/v1` 作代理前缀 → `http://localhost:7788`（target 兜底在 vite.config.ts，同样不剥前缀） | `VITE_APP_SSE_URL=http://localhost:7789/events` |
| `vue-vben` | **15666**（`VITE_PORT`） | vite 配置里 `/admin` → `http://localhost:7788`（`changeOrigin`，完整路径原样转发） | `VITE_GLOB_SSE_URL=http://localhost:7789/events` |

三份注解里值得记住的三个点：

- **代理不做 rewrite 是刻意设计**：后端路由本身就挂在 `/admin/v1` 前缀上，剥前缀会全部 404（vue-element 的 vite 配置注释原话）。代理转发的请求对后端是同源的，所以 dev 态**不经过 CORS**——`server.yaml` origins 里的本地端口清单是给直连场景（如手工调试）用的，三快照的 dev 端口不在清单里不影响使用。
- **SSE 直连不走代理**：三快照的 SSE 地址都直写 `http://localhost:7789/events`——SSE 是独立监听端口，浏览器直连（这也是 `server.yaml` CORS 头里放行 `Authorization`/`X-Token` 的原因之一）。
- **react 的 `VITE_API_URL=/` 别删**：生成的 client path 形如 `admin/v1/captcha`（无前导 `/`），baseURL 为空时 axios 会按当前页面目录相对解析（如 `/auth/admin/v1/...`）导致路径错位——设为 `/` 才能拼出绝对路径交给代理（.env.development 注释原话）。同文件里的 `VITE_PASSWORD_RSA_PUB` 是口令信封公钥（公开值，与后端 `RUSHWIND_PASSWORD_RSA_PRIVATE_KEY` 同对）。

三快照启动命令同构：`pnpm install && pnpm dev`。**选一套跑即可**——三套功能同构、共用同一后端（[01 章 §1](./01-architecture-overview.md)）。

## 6. 质量门（改代码前先会跑）

```bash
cd backend
cargo fmt -p auth -p admin -p admin-diff -p biz-proto-build -- --check
cargo clippy --workspace -- -D warnings
cargo test --workspace
bash apps/admin/api/sync-protos.sh --check     # 契约同步校验门（与 CI 一致）
```

四道门与 CI 完全一致（ubuntu + windows 矩阵），提交前本地全绿是贡献前提（见 [CONTRIBUTING.md](../../CONTRIBUTING.md)）。Windows 上记得给 cargo 命令带上 §1.2 的 `AWS_LC_SYS_PREBUILT_NASM=1`（test/ clippy 同样要编译 aws-lc-sys）。

## 7. 常见故障排查表

| 症状 | 根因 | 处置 |
|---|---|---|
| `postgres connect:` 超时，但 `docker ps` 正常 | IPv6 陷阱：`localhost` 解析成 `::1` 挂死 | 环境变量一律写 `127.0.0.1` |
| `redis connect:` 报 WRONGPASS/NOAUTH 形态 | Redis 容器没设密码但环境变量给了（或反之） | §2 的 `--requirepass` 与 `RUSHWIND_REDIS_PASSWORD` 两边一致 |
| 构建失败，`buf: ...` 开头 | buf 未装 / 不在 PATH | §1.1；`buf --version` 验证 |
| Windows 构建失败在 aws-lc-sys | 本机无 NASM | §1.2：`AWS_LC_SYS_PREBUILT_NASM=1` 前置 |
| 启动监听报端口占用（7788/7789） | 上一个进程没退 / 端口被别的服务占了 | `netstat -ano \| findstr 7788` 找进程；或改 `server.yaml` 的 bind 后重编译 |
| `[migrate] failed right after <name>` | 某迁移 DDL 失败（常见：库里有旧结构残留） | 本地开发直接 `DROP DATABASE` 重建最省事；生产按迁移名定位 |
| 重启后没有重新播种 | 种子幂等是**设计行为**（每段先查计数，非空即跳过） | 想重置种子：DROP 库重来；只重灌某一段请手工清理对应表 |
| 登录突然要求验证码 | 渐进式验证码：同 ip+用户名失败累计达阈值（默认 3，锁定阈值 5 / 15 分钟） | 失败计数在 Redis；等 TTL 重置或清 Redis 对应键，成功登录即清零 |
| Swagger UI 空白 | UI 静态资源走 CDN | 联外网或改用 `/q/openapi.yaml` 裸规格 |
| 前端请求 502/ECONNREFUSED | VITE_PROXY 指向的 7788 没起 | 先起后端再看前端；代理目标在 `.env.development` |
| 前端端口不是文档写的数 | 端口被占时 vite 自动顺延（未设 strictPort） | 以启动日志为准 |

## 8. 本章小结

- 构建自足：框架依赖走 Git 钉版，只有 buf 必须自备；
- Windows 两条前置：`AWS_LC_SYS_PREBUILT_NASM=1`（aws-lc-sys 无 NASM 构建）与地址写 `127.0.0.1`（IPv6 陷阱）；
- 中间件三件：PG / Redis 缺一启动即退，MinIO 缺省时对象存储面不启用、服务照常起（文件服务落本地磁盘镜像）；本机直跑用 `RUSHWIND_*` 环境变量覆盖，凭据与内嵌 yaml 逐字一致；
- 配置四 yaml 是装配/存储/认证/对象存储四面的声明源（编译进二进制，改动要重编译），环境变量按"env > yaml > 代码默认"覆盖；
- 启动序列 = 配置 → 验签引擎 → 连接 → 迁移（admin 60 表 + fork 1 表，幂等可重放）→ 种子（幂等）→ 四个 transport，首启 24–50 秒是正常节奏；
- 验证三件：门控路由 401 信封、`/q/openapi.yaml` 200、`:7789/events` 无令牌 401 探活；
- 三快照 dev 端口 15888 / 15777 / 15666（各自 `.env.development` 为准），代理不 rewrite、SSE 直连；
- 种子管理员 `admin` / `Abcd@1234` 仅用于本地开发。

下一步：[03 · 契约与生成链路](./03-codegen-chain.md)。
