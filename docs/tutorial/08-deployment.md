# 08 · 部署上线

> 前置：[02 从零跑起来](./02-get-it-running.md)。读完你会知道：配置怎么组织、密钥怎么换、启动序列是什么、上线前要过哪些检查。

## 1. 配置模型：内嵌 + 环境覆盖

服务配置内嵌在二进制里（`apps/admin/assets/`，`include_str!` 编译期打包），启动时环境变量**优先**覆盖——单二进制即可部署，改配置不用改文件：

| 文件 | 内容 |
|---|---|
| `server.yaml` | 装配文档（见 §2） |
| `auth.yaml` | 鉴权面：JWT 算法与双钥、authz 引擎类型 |
| `data.yaml` | PostgreSQL 连接、Redis 连接、迁移开关 |
| `oss.yaml` | MinIO endpoint / 桶凭据 / 上传下载 host |
| `jwt_public_key.pem` | 校验公钥（与 auth.yaml 一致） |

环境变量覆盖表（全部前缀 `RUSHWIND_`）：

| 变量 | 覆盖什么 |
|---|---|
| `RUSHWIND_DATABASE_SOURCE` | 数据库连接（**必须 URL 形式**；本机开发加 `?sslmode=disable`） |
| `RUSHWIND_DATABASE_MIGRATE` | 启动迁移开关（true/false） |
| `RUSHWIND_REDIS_ADDR` / `RUSHWIND_REDIS_PASSWORD` | Redis |
| `RUSHWIND_OSS_ENDPOINT` | MinIO endpoint |
| `RUSHWIND_AUTH_JWT_PRIVATE_KEY` / `RUSHWIND_AUTH_JWT_PUBLIC_KEY` | RS256 双钥（PEM 全文注入，覆盖 auth.yaml） |
| `RUSHWIND_PASSWORD_RSA_PRIVATE_KEY` | 口令传输信封的 RSA 私钥（PEM，单行 `\n` 转义可注入；**生产必须替换**——不注入则沿用随源码公开的开发私钥） |
| `RUSHWIND_CAPTCHA_REQUIRED_FAILURES` | 渐进式验证码阈值：同一 ip+用户名登录失败达此次数后要求验证码（默认 3，限 1–4） |
| `RUSHWIND_ACCESS_TOKEN_EXPIRES_SECS` / `RUSHWIND_REFRESH_TOKEN_EXPIRES_SECS` | 令牌有效期 |

## 2. server.yaml：装配文档

一个生命周期、四个 transport——部署形态直接读这份文档就能还原：

```yaml
app:  { name: admin, version: 0.1.0 }
servers:
  - kind: http            # REST :7788
    bind: ":7788"
    edge:
      timeout: 10s
      cors:               # gorilla 兼容层；origins 按部署域名改
        credentials: true
        compat: true
        origins: [ ... ]  # 前端域名白名单
    route_packs:
      - name: admin-surface
        settings: { enable_swagger: true, enable_redoc: true }
  - kind: admin-sse       # SSE :7789 /events
  - kind: admin-tasks     # 任务队列 worker
  - kind: cron            # cron 生产者（三个系统作业）
```

## 3. 启动序列与进程形态

```text
migrate（database.migrate 开关）→ seed（幂等播种）→ REST :7788 + SSE :7789 + worker + cron
```

- 中间件不可达时**启动即退出**——把"配置错了"拦在启动期，不带病运行；
- `enable_swagger` / `enable_redoc` 生产建议关闭（文档端点免鉴权）；
- 反向代理注意：SSE 是独立端口（:7789）的长连接流，网关侧不要缓冲响应体、超时要放宽；前端经 Cookie 跨域携带 refresh 令牌，代理必须保留 `Set-Cookie` 与 `Origin` 相关头。

## 4. 生产密钥（必做）

`auth.yaml` 内嵌的 JWT 双钥是**演示密钥**，随源码公开，生产必须更换：

```bash
openssl genpkey -algorithm RSA -pkeyopt rsa_keygen_bits:2048 -out jwt_private_key.pem
openssl pkey -in jwt_private_key.pem -pubout -out jwt_public_key.pem
```

经 `RUSHWIND_AUTH_JWT_PRIVATE_KEY` / `RUSHWIND_AUTH_JWT_PUBLIC_KEY` 注入，或替换镜像内的 assets 后重建。同批检查：`data.yaml` / `oss.yaml` 的演示口令、种子管理员 `admin` 的初始口令（首登即改）。

refresh Cookie 的 `Secure` 标记按部署 TLS 自适应——生产务必走 HTTPS，否则 Cookie 语义降级。

## 5. 前端生产构建

```bash
cd frontend/admin/react
pnpm install && pnpm build    # 产物 dist/
```

生产构建启用 CSP、X-Frame-Options、HSTS 等安全响应头；API 基址按 `.env.production` 的环境变量指向部署后端（默认演示配置，部署时改）。前端与后端是零改动契约：后端 CORS 白名单里加上你的前端域名即可（`server.yaml` 的 `edge.cors.origins`）。

## 6. 备份与归档

- 数据库定时全量备份是**任务系统的一个任务类型**（`backup`）：在任务管理页配置 cron 即得定时 pg_dump 语义的备份作业，默认保留份数轮换由任务参数控制；
- 审计侧的 `audit_log_archive` 系统作业（每日 03:30）当前只做投递占位：导出与清理尚未实现，JSONL 归档是规划项（[第 7 章](./07-audit-compliance.md)），库内留存策略是现阶段的唯一生效面；
- 对象存储（MinIO）按桶前缀生命周期策略另行配置。

## 7. 上线检查单

- [ ] JWT 双钥已更换（演示钥下线），口令/MinIO 凭据全部改掉
- [ ] `server.yaml` CORS origins 改为生产前端域名
- [ ] swagger/redoc 关闭（或置于内网/鉴权代理后）
- [ ] 全链 HTTPS（Cookie `Secure` 生效、口令传输加密有意义的前提）
- [ ] 中间件地址与凭据经环境变量注入，未落在镜像层
- [ ] `database.migrate` 策略明确（首次部署后可关，迁移收敛到发布流程）
- [ ] 备份任务已配置并试跑一次恢复
- [ ] 种子管理员口令已改，MFA 已绑定
- [ ] SSE 端口经网关放行且未被缓冲

## 8. 本章小结

- 单二进制 + 内嵌配置 + 环境覆盖，部署面收敛到一份装配文档；
- 演示密钥是随源码公开的，上线检查单第一项永远是换钥；
- 四 transport 形态决定了网关配置的两处特殊点：SSE 长连接与 Cookie 透传。

—— 教程完。回头可按 [tutorial/README.md](./README.md) 的角色路线重读，或进 [docs/README.md](../README.md) 的工程档案深读各子系统。
