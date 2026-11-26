# 08 · 部署上线

> 前置：[02 从零跑起来](./02-get-it-running.md)。读完你会知道：配置怎么组织、每个键什么语义、密钥怎么换（runbook 级）、启动序列是什么、上线前要过哪些**可执行的**检查。

## 1. 配置模型：内嵌 + 环境覆盖

服务配置内嵌在二进制里（`apps/admin/assets/`，`include_str!` 编译期打包），启动时环境变量**优先**覆盖——单二进制即可部署，改配置不用改文件：

| 文件 | 内容 | 读取处 |
|---|---|---|
| `server.yaml` | 装配文档（见 §2） | `main.rs` 的 `include_str!` |
| `auth.yaml` | 鉴权面：JWT 算法与双钥、authz 引擎类型 | `config.rs` |
| `data.yaml` | PostgreSQL 连接、Redis 连接、迁移开关 | `config.rs` |
| `oss.yaml` | MinIO endpoint / 桶凭据 / 上传下载 host | `config.rs` |
| `jwt_public_key.pem` | 校验公钥兜底（auth.yaml 未配 public_key 时用） | `main.rs` 的 `include_bytes!` |

覆盖的优先级语义有个三级结构，值得背下来：

- **代码默认值 < yaml < 环境变量**。例：令牌有效期的代码默认是 access 900s / refresh 7 天，`auth.yaml` 把它改成 5400s（1.5h）/ 43200s（12h），环境变量再改就赢；
- **布尔只认 `true` / `1`**（`RUSHWIND_DATABASE_MIGRATE` 写 `yes` 不生效）；
- **空值处理分型**：字符串类（`RUSHWIND_REDIS_ADDR` 等）环境变量**设成空串也算覆盖**；密钥与 endpoint 类用"非空才覆盖"（`env_non_empty`），设空等于没设——不会把密钥清掉。

## 2. server.yaml：装配文档（逐键注解）

一个生命周期、四个 transport——部署形态直接读这份文档就能还原。下面是全量真实文件加注解：

```yaml
app:
  name: admin            # 进程名
  version: 0.1.0
# 进程级 panic 汇报：装配期插上默认 reporter，守卫随装配生命周期卸下。
# 省略本节同为开启——显式落节只为让运维找得到这个开关。
panic_reporting:
  enabled: true          # panic 打一行 target 为 rushwind.panic 的 tracing 日志；
                         # HTTP panic 附请求身份（request_id/method/path），任务投递附消息标识。
                         # 生命周期终态（panic/超时/绑定失败）另有 rushwind.bootstrap 一行。
servers:
  - kind: http           # transport ①：REST 主服务
    bind: ":7788"
    edge:
      timeout: 10s       # 请求预算：整条链（bind→gate→handler→审计）的墙钟上限
      cors:
        # 前端 refresh token 走 HttpOnly Cookie（withCredentials=true），
        # 预检响应必须带 Access-Control-Allow-Credentials: true
        credentials: true
        # compat 选择 gorilla 兼容层（发射规则与参照系逐字节一致）。
        compat: true
        headers:         # 预检放行的请求头——登录链的新头必须登记在这里
          - "X-Requested-With"
          - "X-Request-ID"
          - "Content-Type"
          - "Authorization"
          - "X-Captcha-Id"
          - "X-Captcha-Value"
        methods: [ "GET", "POST", "PUT", "DELETE", "HEAD", "OPTIONS" ]
        origins:
          # 生产前端三套域名 + 本地开发五个端口——部署时只保留你的域名
          - "https://vben.admin.rushwind.cloud"
          - "https://ele.admin.rushwind.cloud"
          - "https://react.admin.rushwind.cloud"
          - "http://localhost:5666"
          - "http://localhost:5777"
          - "http://localhost:5888"
          - "http://localhost:5667"
          - "http://localhost:5778"
    route_packs:
      - name: admin-surface      # admin 契约的 59 个服务面
        settings:
          enable_swagger: true   # /q/swagger-ui（生产建议关）
          enable_redoc: true     # /q/redoc（生产建议关）
      - name: business-surface   # 业务模块面：同 http server、同 edge，无 docs 设置节点
  - kind: admin-sse      # transport ②：SSE 通知服务
    addr: ":7789"        # 独立端口——网关放行与超时按长连接处理
    path: "/events"      # 订阅路径（前端 .env.production 的 VITE_SSE_URL 对齐它）
  - kind: admin-tasks    # transport ③：任务队列 worker（apalis-postgres，队列名 default）
  - kind: cron           # transport ④：cron 生产者
    endpoint: cron://admin
    jobs:
      - tenant_expiry_scan     # 每小时整点：过期租户 ON→EXPIRED
      - audit_log_archive      # 每日 03:30：归档投递占位（见 §8）
      - sys_tasks_periodic     # 每分钟滴答：重读 sys_tasks 的 PERIODIC 行按各自 cron 投递
```

三个 cron 都注册在 `apalis.rs::schedule_registry`（任务监控的 inspect 面读它），worker 按 job 名分发；`sys_tasks_periodic` 是"任务管理页配的任务为何不用重启就生效"的全部机制。

## 3. RUSHWIND_* 环境变量全量清单

loader（`config.rs` + `crypto.rs`）认的键就这 12 个，**生产必改**列给出改谁：

| 变量 | 覆盖什么 | 默认值 | 生产 |
|---|---|---|---|
| `RUSHWIND_DATABASE_SOURCE` | 数据库连接（**必须 URL 形式**；本机开发加 `?sslmode=disable`） | yaml：`host=postgres … dbname=rushwind_admin` | **必改** |
| `RUSHWIND_DATABASE_MIGRATE` | 启动迁移开关（true/1） | yaml `migrate: true` | 策略化（见 §9） |
| `RUSHWIND_REDIS_ADDR` | Redis 地址 | `redis:6379` | **必改** |
| `RUSHWIND_REDIS_PASSWORD` | Redis 密码 | yaml 演示口令 | **必改** |
| `RUSHWIND_OSS_ENDPOINT` | MinIO endpoint（宿主侧跑容器时用它指到发布端口，不落网络别名） | yaml `minio:9000` | 按拓扑 |
| `RUSHWIND_AUTH_JWT_PRIVATE_KEY` | RS256 签发私钥（PEM 全文注入，覆盖 auth.yaml） | auth.yaml 演示钥 | **必改**（§5） |
| `RUSHWIND_AUTH_JWT_PUBLIC_KEY` | RS256 校验公钥（同上；不设则用 auth.yaml / 兜底 pem 文件） | 演示钥 | **必改**（§5） |
| `RUSHWIND_ACCESS_TOKEN_EXPIRES_SECS` | access 令牌秒数 | yaml 5400（代码兜底 900） | 按安全策略 |
| `RUSHWIND_REFRESH_TOKEN_EXPIRES_SECS` | refresh 令牌秒数 | yaml 43200（代码兜底 7 天） | 按安全策略 |
| `RUSHWIND_CAPTCHA_REQUIRED_FAILURES` | 渐进式验证码阈值：同一 ip+用户名登录失败达此次数后要求验证码（**限定 1–4**，越界收敛） | 3 | 可调 |
| `RUSHWIND_PASSWORD_RSA_PRIVATE_KEY` | 口令传输信封的 RSA 私钥（PEM，单行 `\n` 转义可注入） | 仓内开发私钥 | **必改**（§5） |
| `RUSHWIND_CRYPTO_KEY` | `enc:` 静密钥（MFA secret 落库的 AES-256-GCM） | 未设=明文存储 | **必改** |
| `RUST_LOG` | tracing 过滤器（**标准 tracing 语法，非 RUSHWIND_ 前缀**）。缺省 `info`：每请求一行 access 日志（`rushwind.http`：method/path/request_id/status/latency）+ warn/error 全量；每条 SQL 语句也在此级别可见（`sqlx::query`），嫌噪可 `RUST_LOG=info,sqlx=off`；深挖单层用 `RUST_LOG=info,sqlx=debug,rushwind.admin.redis=debug` | `info` | 按需 |

三处边界要知情：

- **MinIO 的 access_key / secret_key 没有 env 覆盖键**——只能改 `oss.yaml` 后重建镜像（或挂载替换该文件）；只有 endpoint 有 `RUSHWIND_OSS_ENDPOINT`；
- `RUSHWIND_CAPTCHA_REQUIRED_FAILURES` 的合法域是 1–4：`clamp(1, 4)` 会把 0 和 100 都收敛进界，不报错；
- 换 JWT 私钥 = 全员下线（旧 access token 全部验签失败）——轮换窗口要选维护时段。

## 4. 启动序列与进程形态

```text
Config::load（yaml + env）→ 开发私钥告警检查 → JWT 校验引擎（RS256）
→ AppState::connect（Postgres 池 25/25、连接超时 10s；Redis 连接管理器；
   JWT 签发引擎——私钥缺失启动即退；五桶 MinIO 引擎逐桶建，冲突=已存在即好）
→ migrate（database.migrate 开关：admin 链 → 模块链）→ seed（幂等播种：admin → 模块）
→ TaskQueue（apalis-postgres，队列 default）→ Bootstrap：route_pack ×2 + cron ×3 + 工厂 ×2
→ build().await → run（生命周期持 OS 信号关停路径）
```

对应源文件 `apps/admin/src/main.rs`（百行上下，值得整个读一遍）。要点：

- 中间件不可达时**启动即退出**——Postgres/Redis 连不上、JWT 私钥缺失都在 `connect` 阶段 `Err` 返回，把"配置错了"拦在启动期，不带病运行；
- `enable_swagger` / `enable_redoc` 生产建议关闭（文档端点免鉴权，`/q/openapi.yaml` 会把全部契约面吐出去）；
- 反向代理注意：SSE 是独立端口（:7789）的长连接流，网关侧不要缓冲响应体、超时要放宽；前端经 Cookie 跨域携带 refresh 令牌，代理必须保留 `Set-Cookie` 与 `Origin` 相关头。Cookie 的 `Secure` 标记按请求头 `x-forwarded-proto` 自适应——**代理必须透传这个头**，否则 HTTPS 部署下 Cookie 语义降级。

## 5. 生产密钥轮换 runbook

### 5.1 JWT 双钥（RS256）

`auth.yaml` 内嵌的 JWT 双钥是**演示密钥**，随源码公开，生产必须更换：

```bash
openssl genpkey -algorithm RSA -pkeyopt rsa_keygen_bits:2048 -out jwt_private_key.pem
openssl pkey -in jwt_private_key.pem -pubout -out jwt_public_key.pem
```

经 `RUSHWIND_AUTH_JWT_PRIVATE_KEY` / `RUSHWIND_AUTH_JWT_PUBLIC_KEY` 注入（PEM 全文注入，换行用 `\n` 转义成单行即可），或替换镜像内的 assets 后重建。注入自检：私钥格式错会在启动的 `with_rsa_private_key_from_pem` 处直接失败退出——启动成功即格式正确。

### 5.2 口令传输信封私钥（P3 警示）

启动时有一行硬告警：

```text
⚠️  RUSHWIND_PASSWORD_RSA_PRIVATE_KEY 未配置，口令传输解密仍在使用内置开发私钥，生产环境必须替换
```

不看日志就上线 = 任何拿到源码的人都能解开你站上的口令信封（`crypto.rs::using_dev_password_private_key` 做的是密码学等价判定，不是有没有设环境变量）。轮换三步：

1. 生成一对新的 RSA-2048：`openssl genpkey -algorithm RSA -pkeyopt rsa_keygen_bits:2048 -out password.pem` 与 `openssl pkey -in password.pem -pubout -out password_pub.pem`；
2. 后端注入 `RUSHWIND_PASSWORD_RSA_PRIVATE_KEY`（单行 `\n` 转义 PEM）；
3. **前端同步换公钥**：`frontend/admin/react/.env` 的 `VITE_PASSWORD_RSA_PUB` 换成新公钥 base64 并重新构建——前后端是同对密钥的两半，只换一半登录链就断了。公钥本来就是公开半，它进前端仓库是设计使然。

传输层还有个 legacy 通道：无 `rsa-v1:` 前缀的载荷按 `base64(AES-128-CBC(key=IV=默认钥))` 解——默认钥 `f51d66a73d8a0927` 与前端 `.env` 的 `VITE_AES_KEY` 同值、随源码公开。rsa-v1 信封是主通道（前端 WebCrypto 发射），legacy 只为过渡期兼容；确认所有前端快照都走 rsa-v1 后，legacy 载荷自然不会再出现。

### 5.3 静密钥（MFA secret 落库）

`RUSHWIND_CRYPTO_KEY` 是 32 字节密钥的 env 来源：设置后 MFA secret 以 `enc:` 前缀 AES-256-GCM 落库；未设置则**明文**。在第一批用户绑定 TOTP **之前**定下这把钥——中途引入只会影响新写的行，密钥丢失 = 所有 `enc:` secret 无法解出，等于全员重新绑定。

### 5.4 同批检查

`data.yaml` / `oss.yaml` 的演示口令、种子管理员 `admin` 的初始口令（首登即改）。refresh Cookie 的 `Secure` 标记按部署 TLS 自适应——生产务必走 HTTPS，否则 Cookie 语义降级。

## 6. MinIO 与 oss.yaml 生产配置

```yaml
oss:
  minio:
    endpoint: "minio:9000"        # S3 引擎连接地址（path-style 寻址，MinIO 要求）
    upload_host: "minio:9000"     # 预留字段（当前无消费方）
    download_host: "minio:9000"   # 生成下载链接的主机——改成外部可达域名
    access_key: "root"            # 演示凭据，必改（无 env 覆盖键）
    secret_key: "*Abcd123456"     # 同上
    token: ""
    use_ssl: false
```

- **五分类桶**：文件服务按 MIME 嗅探结果路由到五个桶——`images / videos / audios / docs / files`（`OssBuckets::NAMES`，`image/*`→images、`video/*`→videos、`audio/*`→audios、文本/PDF/Office→docs、其余→files）。启动时逐桶 `create_bucket`（已存在冲突即视为成功，幂等），MinIO 侧无需预建；
- **host 语义**：`endpoint` 是服务端引擎连的地址（容器网络名即可）；`download_host` 拼进落库的下载链接 `{download_host}/{bucket}/{object}`——生产要改成浏览器可达的地址（如经反代暴露的 MinIO 域名）；嗅探看魔数不看客户端声明的 MIME，改后缀绕不过桶路由；
- 未配置 oss 段时退化为本地磁盘镜像（`./data/files/<bucket>/`），元数据语义一致——单机演示够用，生产别这么跑。

## 7. 前端生产构建

```bash
cd frontend/admin/react
pnpm install && pnpm build    # = vite build --mode production，产物 dist/
```

生产构建启用 CSP、X-Frame-Options、HSTS 等安全响应头，出处有两处，各管一半：

- **CSP meta**：`build/plugins/csp.ts` 在构建期向 `index.html` 注入 `Content-Security-Policy` meta——`script-src 'self'` 加上对页内内联脚本（主题预绘）逐个计算的 `sha256-` 白名单。dev 构建跳过（HMR 内联脚本会被拦白屏）。meta CSP 不支持 `frame-ancestors`，点击劫持防护仍需部署侧响应头；
- **响应头**：`scripts/deploy/nginx.conf`（仓内是静态站参考配置，:8080）——`X-Frame-Options: DENY`、`X-Content-Type-Options: nosniff`、`Referrer-Policy: strict-origin-when-cross-origin`、`Strict-Transport-Security: max-age=63072000; includeSubDomains`、meta 之外的 CSP 补充（`base-uri 'self'; object-src 'none'; frame-ancestors 'none'`）。**不要在这个头里重复声明 script-src**：多 CSP 策略浏览器取交集，meta 里的 sha256 白名单会被无哈希的重复声明重新收窄、内联主题脚本再度被拦。

`.env.production` 决定生产指向：

```ini
VITE_API_URL="/"
VITE_SSE_URL="/events"
```

两个都是同源相对路径——生产需要网关把 API 与 SSE 转发到后端（对照 dev 的 `VITE_PROXY='[["/admin", "http://127.0.0.1:7788/"]]'`）。nginx 反代形态（示例，接到你的静态站 server 块）：

```nginx
# REST 主面（含业务模块前缀 /template/ 等——每个业务模块一个前缀，照抄加行）
location ~ ^/(admin|template)/ {
    proxy_pass http://admin-backend:7788;
    proxy_set_header Host $host;
    proxy_set_header X-Forwarded-For $proxy_add_x_forwarded_for;
    proxy_set_header X-Forwarded-Proto $scheme;   # Cookie Secure 自适应就靠它
}
# SSE：独立端口的长连接，禁缓冲、放宽超时
location /events {
    proxy_pass http://admin-backend:7789;
    proxy_http_version 1.1;
    proxy_set_header Connection "";
    proxy_buffering off;
    proxy_read_timeout 3600s;
}
```

前端与后端是零改动契约：后端 CORS 白名单里加上你的前端域名即可（`server.yaml` 的 `edge.cors.origins`）。

## 8. 备份与归档

- 数据库定时备份走任务系统的 `backup` 任务类型：在任务管理页建 PERIODIC 任务、填 cron 即得定时作业。**如实说明**：worker 目前对 `backup`（同批还有 `broadcast_message` / `script_task`）只记录"任务已触发 + payload"，不执行真实备份动作——"任务面已接线、执行器是占位"，pg_dump 语义的备份作业是规划项。生产数据库备份请先落在 DB 侧工具（pg_dump 定时/WAL 归档），任务面留作触发入口；
- 审计侧的 `audit_log_archive` 系统作业（每日 03:30）当前只做投递占位：导出与清理尚未实现，JSONL 归档是规划项（[第 7 章](./07-audit-compliance.md)），库内留存策略是现阶段的唯一生效面；
- 对象存储（MinIO）按桶前缀生命周期策略另行配置。

## 9. 上线检查单（可执行）

每项带验证命令——绿的才算过：

- [ ] **JWT 双钥已更换**（演示钥下线）：`grep -c "BEGIN PRIVATE KEY" your.env` 确认注入的是新钥；启动日志无 JWT 相关错误
- [ ] **口令信封私钥已换**：启动日志**没有** `RUSHWIND_PASSWORD_RSA_PRIVATE_KEY 未配置` 告警行；前端 `VITE_PASSWORD_RSA_PUB` 与后端私钥同对（登录一次即验证）
- [ ] `RUSHWIND_CRYPTO_KEY` 已设置，且在用户绑定 MFA 前定下
- [ ] Redis / MinIO 凭据全部改掉（MinIO 凭据改 `oss.yaml` 重建镜像）；`server.yaml` CORS origins 改为生产前端域名：

  ```bash
  curl -i -X OPTIONS -H "Origin: https://your.frontend.example" \
       -H "Access-Control-Request-Method: POST" \
       http://<backend>:7788/admin/v1/captcha
  # 响应头应含 Access-Control-Allow-Origin: https://your.frontend.example
  ```

- [ ] swagger/redoc 关闭（或置于内网/鉴权代理后）：

  ```bash
  curl -I http://<backend>:7788/q/swagger-ui     # 期望 404/405 而不是 200
  ```

- [ ] REST 探活（免鉴权端点）：

  ```bash
  curl -s http://<backend>:7788/admin/v1/captcha | head -c 200
  # 期望 JSON：{"captchaId":"…","imageBase64":"data:image/png;base64,…"}
  ```

- [ ] SSE 端口经网关放行且未被缓冲：

  ```bash
  curl -i -N http://<backend>:7789/events
  # 未带 token：应立即收到错误响应（UNAUTHORIZED 语义）而不是挂死/404
  # ——端口与路径可达即探活成功
  ```

- [ ] 全链 HTTPS（Cookie `Secure` 生效、口令传输加密有意义的前提；反代透传 `x-forwarded-proto`）
- [ ] 中间件地址与凭据经环境变量注入，未落在镜像层
- [ ] `database.migrate` 策略明确：首次部署 true 拉起 schema，之后迁移收敛到发布流程（关掉开关、迁移随发布脚本跑）。幂等自证——重启一次，账本行数不变：

  ```bash
  # 两次启动之间跑两次，行数应一致：
  psql "$RUSHWIND_DATABASE_SOURCE" -c \
    "SELECT (SELECT count(*) FROM seaql_migrations) AS admin_chain,
            (SELECT count(*) FROM seaql_migrations_module_template) AS module_chain;"
  ```

  种子同为幂等（自然键/固定键判存，重复启动行数稳定）：

  ```bash
  psql "$RUSHWIND_DATABASE_SOURCE" -c \
    "SELECT count(*) FROM sys_menus WHERE module='DASHBOARD';"   # 前后对比，均为 2
  ```

- [ ] 备份已落在 DB 侧工具（任务面 `backup` 执行器是占位），并**试跑一次恢复**
- [ ] 种子管理员口令已改，MFA 已绑定（绑定前 `RUSHWIND_CRYPTO_KEY` 必须已设，见 §5.3）

## 10. 本章小结

- 单二进制 + 内嵌配置 + 环境覆盖，部署面收敛到一份装配文档；优先级是"代码默认 < yaml < env"，布尔只认 true/1，密钥类空值不覆盖；
- 演示密钥是随源码公开的，上线检查单第一项永远是换钥——JWT 双钥、口令信封私钥（带启动告警自检）、MFA 静密钥三把一起换；
- 四 transport 形态决定了网关配置的两处特殊点：SSE 长连接（:7789，禁缓冲）与 Cookie 透传（`x-forwarded-proto`）；
- 任务系统的 `backup` 与 `audit_log_archive` 都是"面已接线、执行器占位"——真实的备份与归档落 DB 侧工具，别被任务页的存在感骗了。

—— 教程完。回头可按 [tutorial/README.md](./README.md) 的角色路线重读，或进 [docs/README.md](../README.md) 的工程档案深读各子系统。
