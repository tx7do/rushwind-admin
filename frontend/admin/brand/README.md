# RushWind 品牌覆写层

`sync-frontend.sh <frontend>` 每次从上游整树同步后，会执行本目录的
`apply-brand.sh <frontend> <dst>` 为快照打 RushWind 品牌覆写。**品牌覆写是快照
唯一被允许的对上游偏离**，按前端分包：`overlay/<frontend>/` 整树覆盖 +
`apply-brand.sh` 内按前端声明的文案替换表。新增前端 = 新建 `overlay/<前端名>/`
（路径结构镜像该前端的快照内布局）+ 替换表加一个 case 分支，并在本 README 登记。

## React 版（overlay/react/）

| 内容 | 覆写方式 |
|------|---------|
| `public/logo.png`（200×200，RushWind R 字徽标） | `overlay/react/` 整文件覆盖；**程序化生成**：`tools/make-logo.py`（复用 `make-favicon.py` 同一几何源，PIL+numpy 按 rushwind-icon.svg 复绘）。设计要点：不用 icon-512 的徽章底（暗色侧边栏 dark-on-dark 糊团）、无圆角容器（四角全透明）、简化加粗形（进风口/尾迹短线在 32px 展示尺寸下是游离噪点）、裁到内容包围盒留 4% 边距（侧边栏定框展示，整视箱留白会让字形只占四成高）。几何改动只改 `make-favicon.py`，重跑 `python tools/make-logo.py` 重生成（顺带输出 `tools/logo-preview.png` 深浅底预览） |
| `public/favicon.ico`（16/32/48 三档） | `overlay/react/` 整文件覆盖；**程序化生成**：`tools/make-favicon.py`（PIL+numpy 按 rushwind-icon.svg 几何复绘）。设计要点：不用徽章底（深色标签栏 dark-on-dark 糊团）、无圆角容器（四角全透明）、48/32/16 全用简化加粗形（进风口/尾迹短线在 favicon 尺寸下是游离噪点）、亮渐变直接示人。改几何后 `python tools/make-favicon.py` 重生成（顺带输出 `tools/favicon-preview.png` 深浅底预览） |
| `src/components/bussiness/AuthLayout/icons/SloganIcon.tsx`（登录页品牌插画） | `overlay/react/` 整文件覆盖（R 字主标 + 能量流光 + 环流 + 风痕动效，青→天→靛渐变） |
| 品牌文案（系统名 / 版权 / meta / `VITE_APP_TITLE` 等） | `apply-brand.sh` react 分支内逐条显式 sed 替换 |

## Vue Element 版（overlay/vue-element/）

| 内容 | 覆写方式 |
|------|---------|
| `public/logo.png` / `public/favicon.ico` | `overlay/vue-element/` 整文件覆盖；与 React 版**共用同一套程序化生成资产**（`tools/make-*.py` 产物复制一份入本 overlay，几何改动仍只改 tools 后重生成、两处同步复制） |
| `src/pages/core/login/icons/slogan.vue`（登录页品牌插画） | `overlay/vue-element/` 整文件覆盖；React 版 SloganIcon 的 Vue SFC 移植（同一套 R 字几何与动效，样式走 SFC scoped，`rw-` 前缀防碰撞），浮动动效仍由 login 页父级 `.slogan-icon` 提供 |
| 品牌文案（偏好配置系统名 / 登录页 headerTitle 与 brandTitle / 版权 / `VITE_APP_TITLE` / `VITE_APP_NAMESPACE` / `APP_PREFIX` / 样式注入 id / CSP meta name） | `apply-brand.sh` vue-element 分支内逐条显式 sed 替换 |

## Vue Vben 版（overlay/vue-vben/，vben5 monorepo）

| 内容 | 覆写方式 |
|------|---------|
| `apps/admin/public/logo.png` / `favicon.ico` / `pwa-icon-192.png` / `pwa-icon-512.png` | `overlay/vue-vben/` 整文件覆盖；logo/favicon 与另两版共用同一套生成资产，PWA 两档由 `tools/make-pwa-icons.py` 程序化生成（同一几何源；PWA manifest 引用见 internal/vite-config/src/options.ts）。注意 vben 的 public 在 **apps/admin/public/**，overlay 路径必须镜像之 |
| `packages/effects/layouts/src/authentication/icons/slogan.vue`（登录页品牌插画） | `overlay/vue-vben/` 整文件覆盖；与 vue-element 版**同一份** RushWind SFC（上游两版该文件字节相同） |
| 品牌文案（偏好默认值 companyName/companySiteLink / 版权组件默认值 / zh-CN 登录页 pageTitle / 构建产物 license 横幅 / `VITE_APP_TITLE` / `VITE_APP_NAMESPACE`） | `apply-brand.sh` vue-vben 分支内逐条显式 sed 替换；偏好默认值变更**连同其测试快照** `packages/@core/preferences/__tests__/__snapshots__/config.test.ts.snap` 一起替换，保持上游测试语义 |

## 刻意**不替换**的

- `GOWIND_CRYPTO_KEY` 环境变量名（各前端生成客户端）——功能性标识符，
  与后端配置对齐，非品牌语义；生成客户端本身是字节契约面。
- `.env.production` / `.env.development` 的演示端点（`*.demo.admin.gowind.cloud`）——
  部署端点，按实际部署填写；vue-element 的 `.env.development` 里它们只是注释示例。
- `package.json` 的 `name` 字段（react 为上游模板名 `ant-design-pro`，vue-element
  为 `go-wind-vue3-element-admin`）——包管理内部标识，不出现在任何用户可见面。

## 门禁语义（sync-frontend.sh 双清单）

- `<frontend>.MANIFEST.sha256` —— 快照**最终态**（含品牌覆写）的字节清单，
  防手改门，CI 跑（react 与 vue-element 各一道）。
- `<frontend>.UPSTREAM.sha256` —— 上游原始树的字节清单，漂移检测基线：上游有变更时
  `sync-frontend.sh <frontend> --check` 报 exit 2，重跑
  `sync-frontend.sh <frontend>`（同步 + 覆写 + 重建双清单）显式接受。
