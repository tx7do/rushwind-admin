# v0.2.0 — ID 64 位定版 + 出站通知/回收站/渐进验证码/update_mask 写面四大战役

v0.1.0 以来 30 个提交：契约 ID 定版（wire u64 / 落库 i64，有符号唯一教义）、
通知出站投递里程碑、租户回收站、渐进验证码、前端抽包时代两波同步（vintage
af495eb8a）、update_mask 字段级更新写面全量落地。前端三快照 499/558/606 文件，
后端 251 单测零服务依赖全绿。

## 契约面

| 指标 | 数值 |
|---|---|
| proto 文件 | 149（21 个模块域） |
| REST 路由 | 288（含 additional_bindings） |
| ID 定版 | wire `u64` / 实体层 `i64`（protojson 字符串 ID，两 wire 边界 cast 全消） |

破坏性变化：**ID 64 位波**——全部实体 ID wire 形态 `u32`→`u64`（protojson
本就以字符串传输，文本客户端无感；二进制消费方需重生成 stub）。租户回收站
三 RPC 经契约桥在 u64 面手工移植（a156e96），上游 proto 波（fc48f5b7）同步
后桥撤。

## 新增能力

- **通知出站投递里程碑**：SMTP / WEBHOOK（五种签名风格）/ SMS（阿里云 +
  腾讯云）真实 sender，测试投递入口接线台账，v0.1.0 的 heritage 欠账清偿。
- **租户回收站（issue #3）**：trash list / restore / purge 平台门禁 +
  冲突校验 + 全域 purge 级联；三前端快照同步（回收站页签骑新 RPC 面）。
- **渐进验证码（issue #1）**：登录免验证码，直到同 ip+username 失败次数
  达阈值（`RUSHWIND_CAPTCHA_REQUIRED_FAILURES`，默认 3，钳 1..4）；登录
  错误理由拆分（CAPTCHA_REQUIRED / INVALID / TOO_MANY_ATTEMPTS /
  INVALID_TENANT 各自映射，凭据三合一口径「租户、用户名或密码错误」）。
- **update_mask 字段级更新写面**：`FieldMask` 白名单门 + `set_masked!`
  宏 + 34 个 update handler 全量接入——无掩码=存量全量提交语义不变，有
  掩码=只许动选中字段，掩码选中 + 载荷 None = 显式清列；menu meta 点
  路径应用层合并（NULL 列 `{}` 兜底，上游 dcaa4fa9d 同语义）；snake/camel
  归一对表。
- **下游扩展面（脚手架）**：fork 业务模块一等公民——`module-template`
  拷贝改名骨架（自有 buf workspace + 五守卫测试）、`biz-proto-build` 模
  块内布局、`apps/platform` 自由装配二进制、`EdgePack`/`AuditFace` 路由
  打包边、宏 `$crate` 化导出；assembly 面（Config/AppState/rest::pack/
  edge/sse::factory/apalis/migration/seed + 导出宏）声明为下游稳定契约，
  tutorial 新增第 09 章（fork 工作流 + 上游追踪 SOP）。
- **AI Agent M2/M3 收官**：脚本节点校验/执行缝 + revision 版本化；
  PRINT 模块对租户开放。

## 前端（抽包时代）

- 三前端改吃 npm 包：react → `@go-wind/admin-react` 0.3.1、element →
  `admin-core` + `@go-wind/admin-element` 0.1.1、vben →
  `@go-wind/admin-vben` 0.1.5（源码形态包，官方覆写通道）；快照
  594/634/1050 → 499/558/606。品牌覆写三层重构：包 dist postinstall
  补丁（零残留断言，品牌串变形即当场炸）+ vben preferences/locale 官方
  覆写。
- **九条 issue 前端波**（上游 b88a75a1..84607088）：查询区默认折叠 + 流
  式表单分布（40 页显式 collapsed:false 删除）、用户管理左栏重排（当前
  选择→搜索→机构树）、组织单元左树右表三端形态、权限面分组树化
  （vxe transform:true）、登录文案三端统一。
- **UI 抛光波**（上游 af495eb8a，20 提交）：菜单 IconPicker（可搜索 +
  列表带图标）、PasswordToggle 统一 13 处密码眼、宿主 core.json 与包
  内置逐键深合并、API 注册表 method 彩色 tag、SplitPane 窄屏 720px 沉
  底、workspace 快捷链接前景水洗、vben 存储前缀去版本段（legacy
  localStorage 迁移 shim）、devtools null vnode 修复。
- **vben dev 加固**：optimizeDeps 双 i18n 实例排除（登录页裸 key）、路由
  过渡显式 duration（内嵌 webview 不派发 transitionend 兜底）、auth
  layout 去 `:key=fullPath`（out-in+KeepAlive 换页冻结）。

## 基建与质量

- **DB 层测试面**：sqlite 内存 + 进程内 mini-redis fixture
  （testsupport.rs + SqliteTestMigrator），CI 零服务依赖；**251 单测**
  （纯逻辑毯：audit 路径模板/风险引擎权重/常量时间比较/限流键；repo
  租户/全局臂；service handler 链；update_mask 门控/显式清值 71 测）。
  fmt / clippy -D warnings / test 全绿。
- **rust-utils 下沉**：otp/PasswordEnvelope/collect_subtree_ids/printrender
  四件沉工具箱，house 薄转发壳。
- **安全 P2 扫荡**：SSO 身份加固、限流键统一、租户读门、树遍历加固
  （visited 防环，删除级联同事务）。
- **同步流程补洞**：内嵌 openapi.yaml 纳入 proto 同步规程（启动期接口
  面数据源）；menu 同步自然键 MERGE（重复行收敛 + 子级保位，issue #7）；
  seed 序列对齐（固定 ID 后 pkey 撞击）。
- **路由层**：字面段 + 参数段混排 RPC（`{id}:copy`）在 gorilla 重整后
  打通（注册序坑：字面段必须先于参数段 shadow）。

## 已知边界（显式声明，非缺漏）

- **智能问数（NL2SQL）**：NOT_IMPLEMENTED——四重只读护栏链是注入防线
  本体，整链落地后才放行生成 SQL。
- **文件入知识库**（UploadDocFile）：待文件域 multipart 通道。
- **AI Agent revision 快照表**：可逆恢复随独立里程碑移植。
- **gRPC 面**：未提供（wire 面仅 REST/protojson）——框架 gen-grpc 待
  rushwind 侧里程碑。

## 升级说明

- 迁移链幂等追加（回收站列面 + 通知台账），启动自动应用；验证码计数在
  Redis，不涉表。
- 二进制契约消费方因 ID 64 位波需重生成 stub；protojson 文本面无感。
- 前端三快照升级后跑 `pnpm install`（brand 补丁 postinstall 幂等，零残
  留断言护防品牌串漂移）；vben 用户偏好键自动迁移（legacy 前缀 shim，
  外部包升级窗口过后可移除）。
- 新 env：`RUSHWIND_CAPTCHA_REQUIRED_FAILURES`（默认 3，钳 1..4）。
