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

proto 是唯一 API 契约。上游同步面经 `sync-protos.sh` 同步进 `backend/apps/admin/api/protos/`（字节快照 + MANIFEST 门）；fork 自有契约树在 `backend/apps/admin/api/fork/protos/`，自由编辑、不做字节门。两者共享一次 buf 编译、一个池、一套分页类型：

```bash
bash backend/apps/admin/api/sync-protos.sh          # 从上游同步并重建 MANIFEST（只动 protos/，fork 树不受影响）
bash backend/apps/admin/api/sync-protos.sh --check  # 校验门（与 CI 一致）
```

上游 proto 发生变更时：重新同步 → 按需更新 [docs/binding-spec.md](./docs/binding-spec.md)（wire 语义对齐基准）→ 跑差分台架回归，确认字节级兼容后再提交。

### 代码生成

本项目大量依赖构建期代码生成，**请不要手工编辑生成产物**：

| 产物 | 来源 | 说明 |
|------|------|------|
| 路由面 / 绑定计划 / 服务 trait / 挂载胶水 | `rushwind-gen-http`（构建期，rushwind 仓） | 每个源树一个 face，从 buf 注解描述符确定性生成；admin 面落 `crate::proto::gen`，模块面落 `crate::proto::gen::<模块>` |
| proto 类型与 protojson serde | `backend/apps/admin/build.rs` | prost + pbjson，从 `api/`（admin 树 + fork 树）一次编译，一池一类型 |
| 免鉴权白名单表 | `backend/apps/admin/src/auth_free.rs` | 免鉴权端点白名单，单源维护（fork 模块在此追加自己的条目） |

改生成器逻辑请到 rushwind 仓的 `rushwind-gen-http`，其 fixture 测试与本仓守卫测试（白名单、遮蔽路由等）会双向钉住行为。

### 所有权规则（一条线，替代整份门面声明）

后端只有两个房间：**上游同步面**与**下游自有面**。

- **上游同步面**——`backend/apps/admin/src/admin/`（41 个服务实现、数据层、路由边、审计、迁移种子）与 `backend/apps/admin/api/protos/`（契约快照）。上行同步波次只落这两个子树；fork **只读不写**。
- **下游自有面**——`backend/apps/admin/src/modules/`（业务模块，copy-rename `template` 起步）与 `backend/apps/admin/api/fork/protos/`（模块契约树）+ `main.rs` 里每个模块四行装配（`mod` 声明、迁移链、种子、route pack）。fork 的一切代码活在这里；上行**永不触碰**。

跨线消费是稳定的：模块经 `crate::admin::{state, audit, data}` 与 `crate::repo_shell!` / `crate::stamp_update!` / `crate::query_by_id!` 骑 admin 的路由边（`EdgePack` / `edge_router` / `pack_with`）、状态助手与审计层。这些入口的签名与层序语义（bind 外、门内是 wire 契约）按稳定契约维护——改动须在 PR 中显式声明。

守卫面：`apps/admin/src/modules/template` 与 `apps/admin/tests/` 的守卫测试本身就是这条线的常态化检查——它们编译失败即稳定面被破坏。业务模块的写法见教程 [09 写下游](./docs/tutorial/09-write-your-first-module.md)。

### 代码规范

- **Rust**：提交前请本地通过四道质量门（与 CI 一致）：

  ```bash
  cd backend
  cargo fmt -p auth -p admin -p admin-diff -p biz-proto-build -- --check
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

- 是否手改了生成产物或 `backend/apps/admin/api/protos/`
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
