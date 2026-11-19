# 03 · 契约与生成链路

> 前置：[02 从零跑起来](./02-get-it-running.md)。读完你会知道：proto 如何变成 admin 面的 291 条路由、491 条错误映射和 286 个服务方法（fork 面另有 5 条路由、6 条错误映射、5 个方法，同链路生成）；哪些代码永远不该手改。

## 1. 契约树的维护方式：同步 + 校验门

`backend/apps/admin/api/protos/`（admin 契约树）与 `backend/apps/admin/api/fork/protos/`（fork 模块契约树）共享同一个 buf 工作区——一次编译、一个池、一套分页类型。admin 树是唯一 API 契约，由脚本从**上游契约源**同步进本仓，并配 MANIFEST 校验门防止手改：

```bash
bash backend/apps/admin/api/sync-protos.sh          # 同步并重建 MANIFEST.sha256
bash backend/apps/admin/api/sync-protos.sh --check  # 校验门（与 CI 一致）
```

- 同步源的默认路径与覆盖方式见脚本头部说明（`backend/apps/admin/api/sync-protos.sh`）；
- 上游契约发生变更时：重新同步 → `buf breaking` 对主基线做破坏性检查 → 确认行为面（见 §4 差分台架）→ 提交；
- **不要手改** `api/protos/`；
- 契约树引用的注解模块（google.api 等）**不再进仓**：`api/buf.yaml` 把它们声明为 buf 依赖、`api/buf.lock` 按 commit 钉定（与上游 lock 同源，重新同步时会比对两把锁）。注解里携带两类关键信息——`google.api.http` 路由绑定与错误码注解（reason → HTTP 状态映射）——它们是整条生成链的输入。

## 2. 构建期双面编译

`apps/admin/build.rs` 把契约工作区编译成**两个面**（admin 树与 fork 模块树同区编译，每个源树再各出一个 gen face），这是理解本仓的关键一刀：

```text
backend/apps/admin/api/protos（admin 树）+ api/fork/protos（fork 模块树，+ buf 依赖的注解模块）
        │
        ├─ buf build → 全量闭包注解镜像（含 google.api.http / 错误码等自定义选项字节）
        │     └─ rushwind-gen-http（框架仓的生成器）解析描述符，确定性生成：
        │          · ROUTES 路由表（291 条，含 additional_bindings）
        │          · 错误状态表（491 条 reason → HTTP 状态）
        │          · 服务 trait（286 个方法，XxxServiceHandlers）
        │          · mounts 挂载胶水（每服务一个，pub/gate 双路由线程）
        │          · 逐路由绑定计划（body/query/path 变量怎么解）
        │
        └─ buf build（带 source info）→ 过滤后的描述符集 → prost + pbjson
              └─ proto 类型与 protojson serde（64 位整数字符串化、presence 语义）
```

两条闭包都是 buf build 的产物：注解闭包编译时不带 source info（描述符镜像按序产出、平台无关）；类型闭包带 source info——proto 注释经 prost 变成生成类型的 rustdoc，再过滤掉注解声明文件后交给 prost_build/pbjson_build。admin 树与 fork 模块树各出一个 gen face（同一闭包按树前缀切片），类型面则共用一个池。

为什么必须装 buf：构建引擎直接消费 `buf build` 的描述符产物——Rust 侧序列化器（如曾经的 protox 管线）会**丢弃自定义选项字节**，而选项字节正是路由与错误码的载体；生成链已不再依赖 protox，两条闭包统一走 buf build。这是环境要求里 buf"必须"的原因。

## 3. 生成面落位（crate `proto`）

| 符号 | 内容 | 谁消费 |
|---|---|---|
| `crate::proto::<module>::<service>::v1::*` | 请求/响应/实体类型（prost + pbjson） | service 层、绑定层 |
| `crate::proto::gen::services::XxxServiceHandlers` | 服务 trait（async-trait，dyn 兼容） | service 层实现 |
| `crate::proto::gen::mounts::mount_xxx_service` | 挂载胶水：接收 `(router_pub, router_gate, service, wrap)` 双路由线程 | `admin/server/rest.rs` 的 `mount_service!` |
| `crate::proto::tables` | ROUTES 路由表 + reason→状态表 | 绑定层、错误编码 |
| `crate::AUTH_FREE`（面内 `crate::proto::AUTH_FREE` 再导出） | 免鉴权端点白名单（8 端点，单源 `apps/admin/src/auth_free.rs`） | 鉴权门、守卫测试 |

`admin/server/rest.rs` 的 `mount_service!` 表是服务与挂载的唯一对接点——每个服务一行 `($mount, $Service)`，绑定计划决定 `wrap` 怎么组合层级。

> **已知例外（291 条中的 1 条）**：`POST /admin/v1/bi-dashboards/{id}:copy`
> 是 AIP-136 "资源上的自定义动作"形态——路径段里参数后跟 `:verb` 字面量。
> Go 参照的 mux 按字符前缀匹配支持它，而 axum 0.8 的 matchit 路由树拒绝
> 这种混合段（"Only one parameter is allowed per path segment"）。生成的
> mount 对该类绑定跳过 axum 树注册（`is_verb_on_resource` 守卫）：RouteSpec、
> 服务 trait 与实现完整保留，仅该 RPC 暂答 404，待 wire 层原生支持后恢复。
> 其余 `:verb` 路由（如 `monitor-alert-rules:evaluate`）是纯字面量段，
> 不受影响。

**前端同源**：React 版的 TypeScript 客户端（`frontend/admin/react/src/api/generated/`）同样从这套契约生成、随快照进仓——三份产物（Rust 类型、路由面、TS 客户端）共享同一契约源，不存在手写 API 层。

## 4. 差分回归台架：契约面的验收

`backend/testbed` 通过 compose 拉起中间件与 **Go / Rust 双栈后端**，`admin-diff` 对 291 条路由 + 121 条 HEAD 探针（GET 路由逐条自动 sweep）并**逐字节比对**响应，豁免显式登记于 `exemptions.json`（当前 3 类：路由遮蔽、鉴权后路径绑定、HEAD-on-GET）。

使用方式与"探针方法学陷阱"（代理污染、Content-Type 注入等，复跑前必读）见 [backend/testbed/README.md](../../backend/testbed/README.md)。

日常工作流：

- **动了绑定/序列化/错误信封相关代码** → 必须跑台架，以逐字节一致为通过标准；
- **只动了业务逻辑**（service/repo 内部）→ 单元与行为测试即可，台架不覆盖业务数据面；
- **上游契约漂移** → 重新同步 + 台架回归 + 更新 [binding-spec.md](../binding-spec.md)（wire 语义对齐基准）。

## 5. 本章小结

- 契约同步进仓 + MANIFEST 门：proto 面不可手改，漂移必须显式接受；
- 构建期一次编译产出两个面：注解描述符（路由/错误/绑定/trait）与 prost 类型（protojson）；
- 挂载对接只有一张表（`mount_service!` 逐行宏，`mount_items()` 收集），服务与路由的连接零手写；
- 台架逐字节比对是契约改动的验收面。

下一步：[04 · 第一个服务模块](./04-first-service-module.md)。
