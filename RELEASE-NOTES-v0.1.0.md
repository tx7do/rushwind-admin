# v0.1.0 — ce8ab49 vintage 全量同步 + 十域完整移植

第一个正式版本：与上游 go-wind-admin ce8ab49 (2026-11-05) 契约面完全对齐，
Rust 后端完成全部新域的完整移植（非桩），前端三套整树同步 + RushWind 品牌
覆写，真实栈走查（API 冒烟 18/19 + react 浏览器全页面）通过。

## 契约面

| 指标 | 数值 |
|---|---|
| proto 文件 | 149（21 个模块域） |
| REST 路由 | 284（含 additional_bindings） |
| 服务 trait / 方法 | 59 / 556 |
| 错误状态映射 | 546 |
| 免鉴权白名单 | 8 对 |

破坏性变化（buf breaking 显式接受，共 3 处）：oauth.proto 上游删除（由
sso.proto 取代）；LoginResponse 移除 refresh_token / refresh_expires_in
字段（refresh 改走 HttpOnly Cookie）。

## 新增域（本版本完整移植）

- **OIDC SSO 单点登录**：`SSO_OIDC_*` 环境开启；discovery 一小时缓存、
  state 十分钟单次防重放（Redis GETDEL）、code 换 token（client_secret_post）、
  userinfo email 映射本地账号（可 `SSO_AUTO_CREATE` 自动预置）；V1 边界：
  不触发本地 TOTP / 限流 / 登录策略。附带租户白标查询（防枚举，未命中与
  未配置同答 found:false）。
- **角色权限继承**：`sys_roles.parent_id` 成树；编辑边界校验（子角色权限集
  ⊆ 父角色已授予集、同租户、沿父链防环 ≤32）；运行时授权无需展开。
- **通知域增强**：用户偏好（静音时段支持跨零点窗口 + 分类退订）、通知模板
  （`{{var}}` 占位、试渲染、未识别占位符按名报错）、渠道 EXT 扩展机制
  （39 个扩展渠道 schema 注册表逐字移植，settings 整包 schema 校验 + 敏感
  字段脱敏回显 + 留空保留合并）。
- **监控告警**：五指标阈值规则（goroutine/内存/DB 连接/DB 连通/Redis 容量），
  告警→恢复成对通知语义、冷却锚定 last_alerted_at、采集失败轮不清 firing。
- **系统任务巡检**：调度条目 + 队列状态投影 + 最近失败明细（只读）。
- **BI 仪表盘**：设计器 schema 透传、乐观锁版本、内置模板禁删、复制派生、
  启动播种基线模板。
- **打印模板**：服务端 PDF 渲染（chromiumoxide 驱本机 Chrome/Edge，零边距
  PrintToPDF，"预览即所得"——前端分页/毫米布局，后端不重排）。
- **AI 全域**：提供商（OpenAI 兼容/本地模型，API Key AES-GCM 加密 + 脱敏
  hint）、对话/消息（上下文窗口 + 用量台账）、知识库（切片 500/50 向量化
  入库、pgvector 余弦检索、启动期向量列幂等补建）、Agent 应用（五节点图
  引擎：start/knowledge/llm/condition/end，模板 + 受限条件求值、执行轨迹
  与 token 用量落库）、内容生成（四场景）、用量台账与月度汇总。

## 前端

- react (React19+Antd)、vue-element (Vue3+Element Plus)、vue-vben (Vben5
  单包重构) 三快照整树同步至同一 vintage，RushWind 品牌覆写层适配新布局。
- 全部新域管理页可用：BI 仪表盘、通知模板/偏好/渠道扩展、监控告警、
  系统任务巡检、AI 助手（提供商/知识库/Agent 编排/用量统计/智能问数）。

## 基建与质量

- rushwind 框架 rev 钉 a344105（jsonwebtoken 11 摘除 rsa Marvin-Attack、
  sqlx 0.9、gen-http AIP-136 守卫）。
- 91 个单测（引擎/校验器/切片/映射大表/条件求值/渲染内核逐点钉死），
  fmt / clippy -D warnings / test / 契约同步四门禁全绿（ubuntu+windows）。
- 真实栈走查：迁移链 13 个全过、登录链（redis 验证码 + AES-CBC 密码）
  API 冒烟 18/19、react 浏览器全页面走查（品牌/真实数据/新页渲染）。

## 已知边界（显式声明，非缺漏）

- **智能问数（NL2SQL）**：NOT_IMPLEMENTED——四重只读护栏链（表白名单 /
  LIMIT 钳制 / 语句只读校验 / 租户后置 fail-closed）须整链落地后才放行
  生成 SQL，护栏是注入防线的本体。
- **文件入知识库**（UploadDocFile）：待文件域 multipart 通道。
- **出站投递**：SMTP / WEBHOOK / SMS / EXT 真实 sender + 台账实投 + 告警
  周期扫描为下一里程碑（通知域 heritage 欠账）；当前测试发送为配置在场
  预检。
- **`{id}:copy` 一条 AIP-136 绑定**（bi-dashboards 复制）暂答 404：axum
  0.8 的 matchit 不支持"参数+字面量"混合段，RouteSpec/trait/服务面完整，
  待 wire 层原生支持。

## 升级说明

- 数据库迁移链 `m20251105_000001..000010` 幂等追加（新表 + 既有表加列），
  启动自动应用；pgvector 扩展缺席时 RAG / 语义检索面自动降级，不影响启动。
- 登录响应不再返回 refresh token 字段：前端依赖 HttpOnly Cookie（三前端
  快照已同步适配）。
