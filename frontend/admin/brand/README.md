# RushWind 品牌覆写层

`sync-frontend.sh <frontend>` 每次从上游整树同步后，会执行本目录的
`apply-brand.sh <frontend> <dst>` 为快照打 RushWind 品牌覆写。**品牌覆写是快照
唯一被允许的对上游偏离**，按前端分包：`overlay/<frontend>/` 整树覆盖 +
`apply-brand.sh` 内按前端声明的文案替换表。新增前端 = 新建 `overlay/<前端名>/`
（路径结构镜像该前端的快照内布局）+ 替换表加一个 case 分支，并在本 README 登记。

## 机制包时代的品牌覆写面（2026-11-15 上游抽包后）

上游把壳层机制抽成 npm 包（react→`@go-wind/admin-react`、element→`@go-wind/admin-core`
+`@go-wind/admin-element`、vben→`@go-wind/admin-vben` 源码形态包）后，品牌串分布在
三个层面，覆写方式各不相同：

1. **宿主快照**——照旧 sed / overlay 整文件覆盖（index.html、.env、登录插画、
   bootstrap 兜底名、APP_PREFIX、CSP meta name、版权组件、license 横幅）。
2. **机制包 dist（react/element，常规构建包）**——preferences 默认值
   （应用名/companyName/companySiteLink）与 core/auth 文案命名空间进了包 dist，
   node_modules 不入库，snapshot sed 够不着，且包没有声明式品牌覆写 API。由
   `overlay/<frontend>/patches/brand-packages.mjs` 安装后重写：apply-brand.sh 把
   `postinstall` 钩子注入快照 package.json，每次 install 后改写包 dist 内的品牌串。
   脚本幂等且带**零残留断言**（GoWind/风行中后台/gowind.cloud/__gowind-styles__
   必须零残留）——包升级引入表外品牌串时 install 当场失败，强制先对表再放行。
3. **机制包源码（vben，源码形态包）**——包内默认值经官方覆写通道覆盖：
   `src/preferences.ts` 的 `defineOverridesPreferences`（apply-brand.sh 注入
   `copyright` 块）+ 宿主 `src/locales/langs/zh-CN/authentication.json` 键级覆写
   （`@vben/locales` 的 `mergeLocaleMessage` 深合并，宿主胜出）。

`--gowind-*` CSS 变量名与类名前缀是包样式与宿主样式间的内部 API（改名断样式联动），
三端一致**不替换**；`@go-wind/admin-core` dist 实测零品牌串，不补。

## React 版（overlay/react/）

| 内容 | 覆写方式 |
|------|---------|
| `public/logo.png`（200×200，RushWind R 字徽标） | `overlay/react/` 整文件覆盖；**程序化生成**：`tools/make-logo.py`（复用 `make-favicon.py` 同一几何源，PIL+numpy 按 rushwind-icon.svg 复绘）。设计要点：不用 icon-512 的徽章底（暗色侧边栏 dark-on-dark 糊团）、无圆角容器（四角全透明）、简化加粗形（进风口/尾迹短线在 32px 展示尺寸下是游离噪点）、裁到内容包围盒留 4% 边距（侧边栏定框展示，整视箱留白会让字形只占四成高）。几何改动只改 `make-favicon.py`，重跑 `python tools/make-logo.py` 重生成（顺带输出 `tools/logo-preview.png` 深浅底预览） |
| `public/favicon.ico`（16/32/48 三档） | `overlay/react/` 整文件覆盖；**程序化生成**：`tools/make-favicon.py`（PIL+numpy 按 rushwind-icon.svg 几何复绘）。设计要点：不用徽章底（深色标签栏 dark-on-dark 糊团）、无圆角容器（四角全透明）、48/32/16 全用简化加粗形（进风口/尾迹短线在 favicon 尺寸下是游离噪点）、亮渐变直接示人。改几何后 `python tools/make-favicon.py` 重生成（顺带输出 `tools/favicon-preview.png` 深浅底预览） |
| `src/components/bussiness/AuthLayout/icons/SloganIcon.tsx`（登录页品牌插画） | `overlay/react/` 整文件覆盖（R 字主标 + 能量流光 + 环流 + 风痕动效，青→天→靛渐变） |
| `patches/brand-packages.mjs`（机制包品牌补丁） | `overlay/react/` 拷入快照，由 apply-brand.sh 注入的 package.json `postinstall` 钩子驱动；重写 `@go-wind/admin-react` dist 内 preferences 默认值（name/companyName/companySiteLink）与 auth 命名空间登录文案（含 zh pageTitle 风行→锐风） |
| 品牌文案（meta / `VITE_APP_TITLE` / `VITE_APP_NAMESPACE`） | `apply-brand.sh` react 分支内逐条显式 sed 替换 |

## Vue Element 版（overlay/vue-element/）

| 内容 | 覆写方式 |
|------|---------|
| `public/logo.png` / `public/favicon.ico` | `overlay/vue-element/` 整文件覆盖；与 React 版**共用同一套程序化生成资产**（`tools/make-*.py` 产物复制一份入本 overlay，几何改动仍只改 tools 后重生成、两处同步复制） |
| `src/pages/core/login/icons/slogan.vue`（登录页品牌插画） | `overlay/vue-element/` 整文件覆盖；React 版 SloganIcon 的 Vue SFC 移植（同一套 R 字几何与动效，样式走 SFC scoped，`rw-` 前缀防碰撞），浮动动效仍由 login 页父级 `.slogan-icon` 提供 |
| `patches/brand-packages.mjs`（机制包品牌补丁） | `overlay/vue-element/` 拷入快照，由 apply-brand.sh 注入的 package.json `postinstall` 钩子驱动；重写 `@go-wind/admin-element` dist 内 preferences 默认值与 core 命名空间登录文案（headerTitle/brandTitle/版权，含 zh 风行→锐风）+ `__gowind-styles__` 样式注入标记 |
| 品牌文案（bootstrap 兜底名 / `APP_PREFIX` / CSP meta name / `VITE_APP_TITLE` / `VITE_APP_NAMESPACE`） | `apply-brand.sh` vue-element 分支内逐条显式 sed 替换 |

## Vue Vben 版（overlay/vue-vben/，vben5 分层形态）

上游 2026-11-15 进一步分层抽包：冻结层进 `@go-wind/admin-vben`（npm 源码形态包，
宿主经 tsconfig paths/vite alias 直引其 src/），业务热区 `framework/effects/layouts`
+ `framework/effects/plugins` 留宿主。品牌默认值与登录页文案改走官方覆写通道，
sed 面收缩到宿主自有文件。

| 内容 | 覆写方式 |
|------|---------|
| `public/logo.png` / `favicon.ico` / `pwa-icon-192.png` / `pwa-icon-512.png` | `overlay/vue-vben/` 整文件覆盖；logo/favicon 与另两版共用同一套生成资产，PWA 两档由 `tools/make-pwa-icons.py` 程序化生成（同一几何源）。单包结构下 public 在仓库根 **public/**，overlay 路径必须镜像之 |
| `framework/effects/layouts/authentication/icons/slogan.vue`（登录页品牌插画） | `overlay/vue-vben/` 整文件覆盖；与 vue-element 版**同一份** RushWind SFC（上游两版该文件字节相同） |
| `src/locales/langs/zh-CN/authentication.json`（登录页 pageTitle） | `overlay/vue-vben/` 新增宿主语言文件（上游该文案进包 `src/locales`），键级覆写 `{ pageTitle: "锐风中后台管理系统" }`——`@vben/locales` 的 `mergeLocaleMessage` 深合并宿主胜出；en-US pageTitle 为通用文案，无需覆写 |
| preferences 默认值（copyright companyName/companySiteLink） | `apply-brand.sh` 向宿主 `src/preferences.ts` 的 `defineOverridesPreferences` 注入 `copyright` 块（官方覆写通道；幂等，已含 copyright 则跳过） |
| 品牌文案（版权组件默认值 / 构建产物 license 横幅 / `VITE_APP_TITLE` / `VITE_APP_NAMESPACE`） | `apply-brand.sh` vue-vben 分支内逐条显式 sed 替换；目标文件为 `framework/effects/layouts/basic/copyright/copyright.vue`、`build-config/vite-config/src/plugins/license.ts`、根 `.env` |

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
