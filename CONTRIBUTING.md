# 贡献指南

感谢你对 RushWind Admin 的关注！本文档说明如何向本项目提交代码、文档与问题反馈。在开始之前，请先阅读 [README.md](./README.md) 了解项目定位与技术栈。

## 快速上手

1. **Fork 并克隆仓库**

   构建依赖（rushwind 框架、rust-utils）以 Git 依赖按 revision 钉死，clone 本仓即可构建（依赖解析见 [README](./README.md#依赖解析)）；仅当本地迭代框架仓时才需要 checkout 框架仓：

   ```bash
   git clone https://github.com/<your-name>/rushwind-admin.git
   # 可选（框架贡献者工作流）：git clone https://github.com/tx7do/rushwind.git
   ```

2. **搭建开发环境**

   - Rust stable（workspace `rust-version = 1.81`，含 rustfmt / clippy 组件）
   - buf（`curl -fsSL https://buf.build/install.sh | sh`，或 GitHub releases 单二进制；构建期注解闭包编译）
   - bash（Windows 推荐 Git Bash，用于同步与台架脚本）

3. **创建分支**

   请基于最新的 `main` 创建特性分支，不要直接在 `main` 上开发：

   ```bash
   git checkout main
   git pull upstream main
   git checkout -b feat/your-feature
   ```

## 开发约定

### 契约优先

proto 是唯一 API 契约，经 `sync-protos.sh` 从上游契约源同步进本仓。本仓不直接修改 `backend/api/protos/`：

```bash
bash backend/api/sync-protos.sh          # 从上游同步并重建 MANIFEST
bash backend/api/sync-protos.sh --check  # 校验门（与 CI 一致）
```

上游 proto 发生变更时：重新同步 → 按需更新 [docs/binding-spec.md](./docs/binding-spec.md)（wire 语义对齐基准）→ 跑差分台架回归，确认字节级兼容后再提交。

### 代码生成

本项目大量依赖构建期代码生成，**请不要手工编辑生成产物**：

| 产物 | 来源 | 说明 |
|------|------|------|
| 路由面 / 绑定计划 / 服务 trait / 挂载胶水 | `rushwind-gen-http`（构建期，rushwind 仓） | 从 buf 注解描述符确定性生成，落位 `backend/api/admin-api/` |
| proto 类型与 protojson serde | `backend/api/admin-api/build.rs` | prost + pbjson，从 `backend/api/protos/` 编译 |
| 免鉴权白名单表 | `backend/api/admin-api/src/auth_free.rs` | 免鉴权端点白名单，单源维护 |

改生成器逻辑请到 rushwind 仓的 `rushwind-gen-http`，其 fixture 测试与本仓守卫测试（白名单、遮蔽路由等）会双向钉住行为。

### 装配面（下游稳定门面）

本仓是脚手架：fork 的业务模块（`backend/business/*`、`backend/apps/*`）消费 `admin-api` 的库面做装配。以下入口按**下游稳定契约**维护——改动任何一项视为破坏性变更，须在 PR 描述与升级说明中显式声明：

- `admin_api::config::Config` 与 `AppState::connect` / `AppState` 的 pub 字段
- `admin_api::server::rest::pack`、`admin_api::server::edge`（`EdgePack` / `edge_router` / `pack_with` / `make_wrap` 的签名与层序语义——bind 外、门内是 wire 契约）
- `admin_api::server::sse::factory`、`admin_api::server::apalis`（`TaskQueue` / `cron_jobs` / `worker_factory`）
- `admin_api::migration::run`、`admin_api::seed::run`（含幂等语义）
- 导出的宏：`repo_shell!`、`query_by_id!`、`stamp_update!`（展开形状与 `$crate` 解析路径）
- 错误助手与审计面：`admin_api::state::{status_error, not_found, operator_of, tenant_of, db_err}`、`admin_api::audit::AuditFace`

守卫面：`backend/business/module-template` 与 `backend/apps/platform` 本身就是装配面的下游消费者——它们编译失败即装配面被破坏，CI 的 workspace 构建把这条守卫常态化。业务模块的写法见教程 [09 写下游](./docs/tutorial/09-write-your-first-module.md)。

### 代码规范

- **Rust**：提交前请本地通过四道质量门（与 CI 一致）：

  ```bash
  cd backend
  cargo fmt -p proto -p auth -p admin-api -p admin-diff -p biz-proto-build -p module-template -p platform -- --check
  cargo clippy --workspace -- -D warnings
  cargo test --workspace
  ```

- **命名与风格**：新代码应与所在文件既有风格保持一致（缩进、注释密度、命名习惯）；crate 命名按功能命名，不带 `wire` / `kratos` 字样。

### 提交规范

本项目采用 [Conventional Commits](https://www.conventionalcommits.org/zh-hans/)，提交信息格式：

```
<type>(<scope>): <subject>
```

| type | 用途 |
|------|------|
| `feat` | 新功能 |
| `fix` | 缺陷修复 |
| `refactor` | 重构（不改变外部行为） |
| `perf` | 性能优化 |
| `style` | 代码格式（不影响逻辑） |
| `docs` | 文档 |
| `test` | 补充测试 |
| `build` | 构建系统、依赖 |
| `chore` | 杂项、脚手架 |
| `ci` | CI 配置 |

- `scope` 可选，对应受影响的模块（如 `auth`、`tenant`、`codegen`）
- `subject` 使用中文或英文均可，简明描述，结尾不加句号
- **示例**：`feat(auth): 鉴权门失败消息按场景区分两分支`

大改动请添加正文说明动机与影响范围，必要时附 Issue 链接。

## 提交 Pull Request

1. 确保本地四道质量门（fmt / clippy / test / proto 同步校验）全部通过
2. 若改动涉及契约（proto 同步、binding-spec、绑定语义），确认已更新 binding-spec 并跑过差分台架
3. PR 标题遵循上述提交规范
4. PR 描述请说明：动机、改动内容、自测情况、**是否影响契约兼容基线**
5. 一个 PR 只解决一个问题，便于评审与回退

### 评审标准

- 是否手改了生成产物或 `backend/api/protos/`
- 是否破坏契约兼容基线（以 [docs/binding-spec.md](./docs/binding-spec.md) 与差分台架结论为准）
- 是否绕过生成器手写路由 / 绑定逻辑
- 是否引入新的硬编码密钥 / 凭据（应走配置；演示密钥不得用于生产语义的新代码路径）
- 是否补齐了相应测试（行为对齐类改动应有金样或差分佐证）
- 文档是否同步更新

## 反馈问题

- 缺陷与新功能建议请直接开 Issue
- 安全漏洞请按 [SECURITY.md](./SECURITY.md) 流程**私下上报**，勿直接开公开 Issue

## 致谢

你的每一次贡献——无论代码、文档还是问题反馈——都让 RushWind Admin 更好。感谢你的参与。
