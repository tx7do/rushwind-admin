# 服务演进路线（services-evolution.md）

> **定位**：业务域从"单体内的一个模块"长成"独立服务"的演进路线。它回答：什么时候该迈出下一级、每一级长什么样、迁移时单体里的每一件产物去哪里。第一级多服务的**现成参考实现是 [rushwind-cms](https://github.com/tx7do/rushwind-cms)**——本文不描述纸上架构，只描述已跑通的形态。
> **什么时候读**：某个业务域开始长出单体边界（独立伸缩曲线、独立部署节奏、非 Rust 消费方）时；规划多服务拓扑时；评估"要不要把模块拆出去"之前——**多数时候答案是"还不到"**。
> **前置**：[架构决策与演进路线](./architecture-decisions.md)（单应用定稿、按需打包两级路线）；[09 写下游](./tutorial/09-write-your-first-module.md)（模块解剖与四行装配）。

---

## 0. 演进阶梯：五级，逐级触发

| 级 | 形态 | 参照物 | 触发信号 | 不触发的默认 |
|---|---|---|---|---|
| **L1** | 单应用 + `src/modules` 模块 | rushwind-admin 现状 | — | **默认永远停在这里** |
| **L2** | 部署级 feature 门 | architecture-decisions §3.1 | 不同部署要不同的域组合 | — |
| **L3** | lib 分发（三 crate 分层） | architecture-decisions §3.3 | 跨仓/跨团队的二开消费者 | — |
| **L4** | **多服务**（域出进程边界） | **rushwind-cms** | 见 §1 触发信号 | — |
| **L5** | 规模化多服务 | 成熟基础设施组件逐项进场（服务发现 etcd/Consul、OTel 链路、Saga 补偿、策略引擎 Casbin/OPA） | 多团队、多环境集群、严格合规 | — |

L4 之前的一切复杂度都是预支的：L2/L3 不改变进程拓扑，L4 才第一次引入进程边界——**每一级只解决上一级解决不了的痛，不解决就不迈**。

---

## 1. 什么时候该到 L4（触发信号与反信号）

### 触发信号（满足其一方才考虑）

- **独立伸缩曲线**：某域的负载形态与整体显著偏离（如内容分发域读多写少、需要独立扩容与独立缓存层）；
- **独立部署节奏**：某域的发布频率远高于底座（底座冻结期域还要迭代，或反之）；
- **非 Rust 消费方**：域能力要被 Go/Python/移动端直连（gRPC 契约面成为刚需）；
- **独立数据版图**：域需要独立的数据库实例/保留策略/合规边界；
- **团队所有权**：域归另一条产品线/另一个团队，发布权必须解耦。

### 反信号（出现这些时，留在 L1–L3）

- "只想让代码分目录"——模块已经是；
- "只想按需部署"——L2 的 feature 门就是；
- "未来可能要多服务"——**预支的多服务 = 把分布式复杂度搬进单体开发流**（服务间契约、版本对齐、联调环境全是净增成本），触发器驱动，同 [architecture-decisions §3.3](./architecture-decisions.md) 的纪律；
- 团队没有多服务的运维面（服务发现、链路、部署编排的持有成本是常驻的）。

---

## 2. rushwind-cms：L4 的参考形态解剖

三服务拓扑，`backend/` 下一个 workspace：

```
backend/
├── api/                     # 契约树（buf 工作区 + MANIFEST.sha256 + sync-protos.sh —— 与 admin 同款门）
├── crates/
│   ├── proto/               # 契约 crate：一份 proto 同时产出 REST 面（rushwind-gen-http：
│   │                        #   路由/绑定/trait/mounts）与 gRPC 面（tonic-prost：
│   │                        #   全量契约 server/client）——类型一样，零重复
│   ├── auth/                # 鉴权门
│   └── store/               # 共享数据层
└── services/
    ├── core-service/        # 域内领域层：gRPC :6602，独占 PostgreSQL/Redis，不对外暴露
    ├── admin-api/           # 管理后台 BFF：REST :6600 / SSE :6601
    └── app-api/             # 前台 BFF：REST :6700 / SSE :6701
```

**五条形态规则**（每条都是决策，不是惯例）：

1. **core 是唯一的领域层**：全部业务逻辑在 core-service；admin/app 两个 BFF 是**脚本生成的 pass-through 代理**——BFF 不写业务，只做协议转换、鉴权门、SSE 通道与前端整形。领域逻辑全在 core 意味着"加一个 BFF"是纯生成动作，零逻辑搬运。
2. **契约双面出自同一份 proto**：REST 面与 gRPC 面由同一个契约 crate 生成，类型零重复——单体内"一池一类型"的教义在进程边界上的延续，信封转码缝依旧不存在。
3. **域服务不对外**：裸 gRPC 不出网。对外协议（REST/SSE）、四字段错误信封、CORS、鉴权全部收口在 BFF；core 只信内网 gRPC。
4. **前端零改动**：五个前端快照（管理后台 + 四款前台）对外契约不变，API 基址指向本仓后端即可——对外契约稳定是演进不变量，任何拓扑变更不得破坏它。
5. **同一套生成哲学**：MANIFEST 契约门、构建期路由表/绑定计划/错误状态表、HS256 鉴权门、protojson 信封——rushwind-admin 的开发者到 rushwind-cms 不需要换脑子，只需要换拓扑。

---

## 3. 迁移映射：单体的每件产物去哪里

从 rushwind-admin 的一个域（以 template/notice 为例）到 rushwind-cms 形态的映射：

| 单体内的产物 | L4 服务里的归宿 |
|---|---|
| `src/modules/<domain>/`（services、data、migration、seed） | `services/core-service/` 的领域层 |
| `api/fork/protos/<domain>/`（域契约树 + i_notice） | `api/protos/` 的域子树（core 契约面输入） |
| `EdgePack` route pack（server/edge.rs） | 拆两半：域方法 → core 的 gRPC server；对外整形 → BFF 的 REST 面 |
| `main.rs` 四行装配 | core 的装配 + BFF 的生成代理声明 |
| 鉴权门（EdgePack 内 bind/门层序） | **上收到 BFF**——门是网关职责；core 只做服务间身份 |
| `i_*` BFF 投影面 | **升维成整个 BFF 服务**——单体里 i_* 是 admin 面的投影；多服务里 BFF 本身就是投影（pass-through 生成即投影的机械化） |
| 独占的 PostgreSQL/Redis 连接 | core-service 独占（单体里模块与 admin 共库；拆出后数据版图随域走） |
| `repo_shell!` / 宏族 / state 助手 | `crates/store` + 契约 crate 的支撑面（architecture-decisions §3.3 的 admin-support 形状——目标形状映射；cms 现状 store crate 为 {auth, bootstrap, crypto, entities, paging}，不含宏族） |

**三 crate 分层是 L4 的直接前置**：architecture-decisions §3.3 抽出的 `admin-contract` 叶子，在 L4 就是域服务的契约依赖——L3 做得越实，L4 的迁移越接近"把 route pack 挂到新 main"。

---

## 4. 规模化（L5）的信号

当多服务数量超过"一个人能口述全部拓扑"时，才进入 L5 的议题：服务注册与发现（etcd/Consul）、OTel 链路追踪、跨服务一致性（Saga/分布式事务）、策略引擎外置（Casbin/OPA）。这些组件在 Go 生态均有成熟对应物，Rust 侧按需引入，**同样是触发器驱动**——没有多团队与多环境集群，它们全是净增运维成本。

---

## 5. 决策日志

| 日期 | 决策 | 状态 |
|---|---|---|
| 2026-11-19 | 演进阶梯定稿：L1 单体模块 → L2 feature 门 → L3 lib 三 crate → L4 多服务（rushwind-cms 形态）→ L5 规模化；逐级触发、默认停在 L1 | ✅ 定稿 |
| 2026-11-19 | L4 参考形态以 rushwind-cms 为准（core 纯域 + BFF pass-through 代理 + 契约双面 + 域服务不对外 + 前端零改动） | ✅ 定稿（参照既有实现） |
| — | 单体某域的实际迁移（首个 L4 案例） | 📋 等触发器 |
