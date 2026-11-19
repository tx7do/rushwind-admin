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

**依赖解析说明**：构建不依赖任何本地兄弟仓——框架（rushwind）与工具库（rust-utils）以 Git 依赖按 revision 钉死，clone 本仓即可构建。只有两种情况需要本地 checkout 框架仓：给框架本身提改动、或本地双仓联调（workspace 层用未提交的 `[patch]` 段指向本地路径，属于框架开发者工作流）。

## 2. 起本地中间件

后端启动即连接 PostgreSQL 与 Redis，缺任一样启动即退出；MinIO 是软依赖——没有 oss 配置时对象存储面不启用，服务照常启动：

```bash
docker run -d --name rw-pg  -p 5432:5432 -e POSTGRES_PASSWORD=*Abcd123456 -e POSTGRES_DB=rushwind_admin postgres:16
docker run -d --name rw-redis -p 6379:6379 redis:7
docker run -d --name rw-minio -p 9000:9000 -e MINIO_ROOT_USER=root -e MINIO_ROOT_PASSWORD=*Abcd123456 minio/minio server /data
```

> ⚠ **Windows + Docker Desktop 的 IPv6 陷阱**：Docker 的 IPv6 回环端口代理可能挂死——`::1:5432` 超时而 `127.0.0.1:5432` 瞬通。下面给后端的环境变量一律写 `127.0.0.1`，不要写 `localhost`（Rust/tokio 解析 `localhost` 会优先走 `::1`，表现为连接池超时的假象）。

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

## 4. 验证后端

```bash
# 1) 门控路由未带令牌 → 统一四字段错误信封（code/reason/message/metadata）
curl -s http://127.0.0.1:7788/admin/v1/users
#   {"code":401,"reason":"UNAUTHORIZED","message":"missing bearer token",...}

# 2) 内嵌 OpenAPI 规格（文档端点三件套均免鉴权）
curl -s -o /dev/null -w "%{http_code} %{size_download}\n" http://127.0.0.1:7788/q/openapi.yaml
```

浏览器打开 <http://127.0.0.1:7788/q/swagger-ui>（Swagger UI）或 `/q/redoc`（ReDoc）——由 `server.yaml` 的 `enable_swagger` / `enable_redoc` 开关控制，UI 静态资源走 CDN，需要外网。

## 5. 起前端并登录

```bash
cd frontend/admin/react
pnpm install
pnpm dev                   # :5888，dev 代理把 /admin 转发到 :7788
```

浏览器打开 <http://localhost:5888>，用种子管理员 **`admin` / `Abcd@1234`** 登录（租户编号留空 = 平台登录）。登录链路的验证码是**渐进式**的：默认没有验证码，同一 ip+用户名组合登录失败累计达到 `captcha_required_failures`（默认 3，可用 `RUSHWIND_CAPTCHA_REQUIRED_FAILURES` 调整）后，后续登录才要求图形验证码——缺头回 `CAPTCHA_REQUIRED`（登录页据此点亮验证码框）、错码回 `CAPTCHA_INVALID`、触发限流锁定回 `TOO_MANY_ATTEMPTS`（1161）、租户无效回 `INVALID_TENANT`（6）；登录成功后失败计数清零。通过验证码后依次是口令应用层加密传输与 JWT 签发。进仪表盘看到真实统计即打通。

若端口 5888 被占，vite 会自动顺延，以启动日志为准。代理目标写在 `frontend/admin/react/.env.development` 的 `VITE_PROXY`，默认已指向 127.0.0.1:7788，零配置直连本仓后端。

## 6. 质量门（改代码前先会跑）

```bash
cd backend
cargo fmt -p auth -p admin -p admin-diff -p biz-proto-build -- --check
cargo clippy --workspace -- -D warnings
cargo test --workspace
bash apps/admin/api/sync-protos.sh --check     # 契约同步校验门（与 CI 一致）
```

四道门与 CI 完全一致（ubuntu + windows 矩阵），提交前本地全绿是贡献前提（见 [CONTRIBUTING.md](../../CONTRIBUTING.md)）。

## 7. 本章小结

- 构建自足：框架依赖走 Git 钉版，只有 buf 必须自备；
- 中间件三件：PG / Redis 缺一启动即退，MinIO 缺省时对象存储面不启用、服务照常起；本机直跑用 `RUSHWIND_*` 环境变量覆盖，地址写 `127.0.0.1`；
- 启动序列 = 迁移 + 种子 + 四个 transport，首启 24–50 秒是正常节奏；
- 种子管理员 `admin` / `Abcd@1234` 仅用于本地开发。

下一步：[03 · 契约与生成链路](./03-codegen-chain.md)。
