# frontend

三套管理端前端（react / vue-element / vue-vben）与后端构成零改动兼容契约：
前端及其生成客户端（`protoc-gen-typescript-http` 产物，三份字节相同）不做任何修改
即可对接本仓后端（REST :7788）。线上行为的逐字节对齐基准见
[`../docs/binding-spec.md`](../docs/binding-spec.md)。

任何需要改动前端的诉求都视为契约破坏：必须先更新 binding-spec 并走差分回归，
再在上游变更后整树重新同步。

三套前端全部随仓（同步快照 + 品牌覆写）。

## 随仓快照（整树同步 + 品牌覆写 + 双清单门）

`admin/react/`、`admin/vue-element/` 与 `admin/vue-vben/` 是上游前端的**同步快照**，
经 `sync-frontend.sh <frontend>` 整树同步后自动施加 RushWind 品牌覆写
（`admin/brand/`：logo / favicon / 登录页品牌插画 + 品牌文案——这是快照
唯一被允许的对上游偏离），并以双清单校验：

- `<frontend>.MANIFEST.sha256` —— 快照终态字节清单（防手改门，CI 同款）
- `<frontend>.UPSTREAM.sha256` —— 上游原始树字节清单（漂移检测基线）

```shell
bash frontend/admin/sync-frontend.sh react          # 同步 + 品牌覆写 + 重建双清单
bash frontend/admin/sync-frontend.sh react --check  # 校验门（防手改 exit 1 / 上游漂移 exit 2）

bash frontend/admin/sync-frontend.sh vue-element          # 同上，vue-element 版
bash frontend/admin/sync-frontend.sh vue-element --check

bash frontend/admin/sync-frontend.sh vue-vben          # 同上，vue-vben 版（monorepo）
bash frontend/admin/sync-frontend.sh vue-vben --check
```

同步源默认取 `/d/GoProject/go-wind-admin/frontend/admin/<frontend>`（作者
Windows 机），本机无源仓时可用环境变量覆盖（`REACT_FRONTEND_SRC` /
`VUE_ELEMENT_FRONTEND_SRC` / `VUE_VBEN_FRONTEND_SRC`）。CI 无源仓，漂移对比自跳过。

**不要手改快照内任何文件**（`node_modules/` 与 `dist/` 不入库）：品牌改动一律进
`admin/brand/` 覆写层（详见其 README）；其余改动视为契约破坏，必须先更新
binding-spec 并走差分回归。多前端共用同一套同步与门禁机制，脚本用法见其头部说明。

**上游同期性**：三份快照的生成客户端必须字节相同（契约面）。上游演进后重同步时，
三个前端与后端 vendored protos 应在同一上游提交上一次性接受，避免混搭不同契约
世代的快照。vue-vben 是 vben5 monorepo（packages/ + apps/admin + internal/），
品牌覆写路径横跨 packages 与 apps 两级，见 brand/README。
