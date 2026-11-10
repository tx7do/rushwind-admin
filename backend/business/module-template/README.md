# module-template —— 业务模块骨架

copy-rename 即开工的完整业务模块：自带契约树（`api/`）、构建期生成（`biz-proto-build`）、一个租户隔离的示例服务（notice 公告）、幂等种子（菜单 / 权限 / sys_apis 登记）、独立迁移链，以及骑在 admin 边缘栈上的 route pack。

## 目录解剖

```
module-template/
├─ api/                      # 模块自有 buf 工作区（契约树跟着模块目录走）
│  ├─ buf.yaml               #   deps 用 BSR 模块，commit 由 buf.lock 钉死
│  ├─ buf.lock               #   首次构建拉进模块缓存，之后离线可用
│  └─ protos/notice/service/v1/
│     ├─ notice.proto        # 领域契约：message + 请求/答复（字段号：1-99 业务 / 100+ 审计人 / 200+ 时间）
│     ├─ notice_error.proto  # Kratos 风格错误枚举（(errors.code) 钉 HTTP 状态）
│     └─ i_notice.proto      # BFF 契约：google.api.http 注解，真正生成路由的文件
├─ build.rs                  # 5 行 Build 表调 biz-proto-build（根路径 = 本 crate 的 api/）
├─ src/
│  ├─ lib.rs                 # 契约面（proto/pool/gen）+ 本地错误助手 + paging 转码 + pack()
│  ├─ auth_free.rs           # 免鉴权白名单（模板默认全 gated，空表）
│  ├─ data.rs                # 实体（biz_ 前缀表）+ repo_shell! 派生的租户 repo
│  ├─ services.rs            # NoticeService：Handlers trait 实现
│  ├─ seed.rs                # 幂等种子：菜单 / 权限 / sys_apis（按自然键，不占固定 id）
│  └─ migration.rs           # 独立迁移链（与 admin 链互不干扰）
└─ tests/gen_surface.rs      # 守卫测试：路由数 / 审计行 / 白名单 / 错误表 / 描述符池
```

## copy-rename 清单（fork 的前 30 分钟）

以改成 `module-erp`、示例域改 `warehouse`（仓储）为例：

1. **目录与 crate 名**：`backend/business/module-template/` → `backend/business/module-erp/`；`Cargo.toml` 的 `name = "module-template"` → `"module-erp"`。
2. **契约**：
   - `api/protos/notice/` 目录 → `api/protos/warehouse/`；
   - 三个 proto 文件的 `package notice.service.v1` → `warehouse.service.v1`（全文件替换 `notice.service.v1`）；
   - `i_notice.proto` 的 service 名、路由前缀 `/template/v1/…` → `/erp/v1/…`（前缀 slug 自定，与模块名一致即可）；
   - `notice_error.proto` 的枚举名 `NoticeErrorReason` → `WarehouseErrorReason`。
3. **build.rs**：`root_prefix: Some("notice/service/v1/")` → `"warehouse/service/v1/"`。
4. **Rust 侧全文件替换**：`crate::gen::services::NoticeServiceHandlers`、`crate::proto::notice::service::v1`、`mount_notice_service`、`notice_proto`、`NoticeService`、`NoticeRepo`、`notices`（实体模块名）——按新域重命名。
5. **lib.rs 常量**：`AUDIT_ROWS`（路径前缀 + operation_id）、`status_error/not_found`/`seed.rs` 里 `tables::error_status` 的包名、`BUSINESS_MODULE = "TEMPLATE"` → `"ERP"`。
6. **数据面**：实体 `table_name = "biz_notices"` → `"erp_warehouses"`（业务表一律 `<module>_` 前缀）；迁移链首 vintage 的名字改成自己的（`mYYYYMMDD_00000N_<module>_init`）。
7. **种子**：菜单目录/页面的 path、name、component；权限 code（`template:manage_notice` → `erp:manage_warehouse`）；角色绑定按需增删。

改名后 `cargo test -p module-erp`——守卫测试会把漏改的点全部点亮。

## 装配（fork 的 main.rs 长什么样）

看 `backend/apps/platform`——它就是模板的消费者：admin 迁移 → 模块迁移 → admin 种子 → 模块种子 → 注册两个 route pack（`admin-surface` + `business-surface`）。同进程、同一个鉴权门、同一个审计层。

## 放行给租户的两步（运营面）

1. **sys_apis**：自动——模块种子每次启动把路由表按 (path, method) upsert 进 `sys_apis`（`business_module = TEMPLATE`）。运行时鉴权门对未登记路由 fail-closed，这一步就是登记。
2. **套餐模块白名单**：手动——管理台 → 套餐管理，给目标套餐勾上 `TEMPLATE` 模块（写入 `sys_plan_modules`）。不勾的话：平台用户（tenant 0）直接可用，租户用户 403。这是刻意不种子化的业务决策。

## 惯例与红线

- 契约只改模块自己的 `api/`；**不要**把业务 proto 混进主仓 `backend/api/protos/`（那是上游同步冻结区，MANIFEST 字节门会拦）。
- 免鉴权端点：`src/auth_free.rs` 加 `(service_fq, method)` 对，同时在 rpc 上补 gnostic `security: {}` 文档标注（参照主仓 `i_authentication.proto`）。
- 加 rpc 时同步更新：`i_*.proto` → `AUDIT_ROWS` 字面量 → `tests/gen_surface.rs` 的路由数断言（守卫测试钉住三者一致）。
- 业务数据一律 `migration.rs` 自己的链 + `<module>_` 表前缀；不要往 admin 的 `sys_*` 表加列。
- 种子保持幂等（自然键查找，缺才插），**不要用固定 id**——admin 种子占用的 id 空间和序列推进都与你无关。

## 已知限制

- 菜单的 `module` 列在管理台 UI 侧是封闭枚举（`MENU_MODULE`），`TEMPLATE` 之类的业务值在模块筛选器里显示为 UNSPECIFIED——运行时字符串是开放的，只影响展示不影响功能。
- 模块 pack 不带 docs 面（Swagger/Redoc 挂在 admin-surface 下）；要给自己的契约出 OpenAPI，需要自嵌 spec 资产并换路由前缀（axum merge 同路径冲突）。
