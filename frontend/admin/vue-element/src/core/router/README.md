# core/router 路由核心模块

> 面向 admin 业务逻辑开发者的使用指南

## 模块定位

本模块提供 **可跨项目复用** 的路由核心能力，不包含任何业务绑定代码。

项目业务层（`src/router`）通过组合本模块的能力来完成路由实例创建、守卫配置和权限接入。

```
src/core/router          ← 可复用的路由能力（本模块）
src/router                ← 项目业务绑定（实例、守卫、路由定义）
src/router/routes/modules ← 业务路由模块文件
```

---

## 目录结构

```
core/router/
├── index.ts                         # 统一导出
├── accessible.ts                    # 入口：生成可访问的路由和菜单
├── generators/
│   ├── index.ts
│   ├── generate-routes-frontend.ts  # 前端模式：静态路由 + 权限过滤
│   ├── generate-routes-backend.ts   # 后端模式：API 路由 + 组件映射
│   └── generate-menus.ts            # 路由 → 菜单转换
└── utils/
    ├── index.ts
    ├── merge-route-modules.ts       # 合并动态路由模块
    ├── merge-hidden-routes.ts       # 合并隐藏子路由
    ├── reset-routes.ts              # 重置动态路由
    └── sort-routes.ts               # 路由排序
```

---

## 两种权限模式

| 模式 | 路由来源 | 权限控制时机 | 配置项 |
|---|---|---|---|
| **前端模式** `frontend` | 本地静态路由文件 | 前端根据 `meta.authority` 过滤 | `preferences.app.accessMode: "frontend"` |
| **后端模式** `backend` | 后端 `GetNavigation` API | 后端只返回有权限的路由 | `preferences.app.accessMode: "backend"` |

默认使用 **前端模式**。

---

## 核心 API

### `generateAccessible(mode, options)`

整个模块的主入口。根据权限模式生成路由和菜单，并自动注册到 Vue Router。

```ts
import { generateAccessible } from "@/core/router";

const { accessibleMenus, accessibleRoutes } = await generateAccessible("frontend", {
  router,           // Vue Router 实例
  routes,           // 静态路由列表
  roles: [],        // 用户角色码
  accessCodes: [],  // 用户权限码
  forbiddenComponent: () => import("@/pages/core/error/403.vue"),
});
```

**内部流程**：

1. 根据模式调用 `generateRoutesByFrontend` 或 `generateRoutesByBackend`
2. 后端模式下合并隐藏子路由（`mergeHiddenRoutes`）
3. 自动为无 `redirect` 的父路由设置首个子路由为重定向目标
4. 调用 `router.addRoute()` 注册每条路由
5. 排序路由（`sortRoutes`）
6. 生成菜单（`generateMenus`）
7. 返回 `{ accessibleMenus, accessibleRoutes }`

---

### `generateRoutesByFrontend(routes, roles, forbiddenComponent, accessCodes)`

前端模式路由生成。根据 `meta.authority` 过滤本地静态路由。

**权限判断规则**（`hasAuthority` 函数）：

| 条件 | 结果 |
|---|---|
| `meta.ignoreAccess = true` | 直接放行 |
| `meta.authority` 未设置 | 直接放行 |
| 用户角色码命中 `authority` 中的任一项 | 放行 |
| 用户权限码精确命中 `authority` 中的任一项 | 放行 |
| 用户权限码是 `authority` 中任一项的前缀 | 放行（如用户有 `sys:manage`，则 `sys:manage:add` 也通过） |
| 以上都不满足，但 `meta.menuVisibleWithForbidden = true` | 保留但替换为 403 组件 |

---

### `generateRoutesByBackend(options)`

后端模式路由生成。从 API 获取路由数据，将 `component` 字符串映射为实际组件。

**组件映射规则**：

```
后端 component 值          → 映射目标
──────────────────────────────────────────────
"Layout" / "BasicLayout"  → layoutMap 中的 Layout 组件
其他字符串                  → pageMap 中对应的页面组件
```

**路径标准化** (`normalizeViewPath`)：

```
"/pages/app/system/dict/index.vue"  →  "/app/system/dict/index.vue"
"./pages/app/system/dict/index.vue" →  "/app/system/dict/index.vue"
"pages/app/system/dict/index.vue"   →  "/app/system/dict/index.vue"
```

> 即：去除 `./` 前缀，去除 `/pages` 前缀，确保以 `/` 开头。

---

### `generateMenus(routes, router)`

将路由列表转换为菜单树（`MenuRecordRaw[]`），供侧边栏渲染。

**转换逻辑**：

1. 通过 `router.getRoutes()` 获取最终路径（处理相对路径拼接）
2. 提取 `meta` 中的 `title`、`icon`、`order` 等字段
3. `meta.hideChildrenInMenu = true` 时清空子菜单
4. 递归记录 `parent` / `parents` 路径
5. 按 `meta.order` 升序排序
6. 过滤掉 `meta.hideInMenu = true` 的项

---

## 工具函数

### `mergeRouteModules(routeModules)`

合并 `import.meta.glob` 动态导入的路由模块。

```ts
// src/router/routes/index.ts 中的实际用法
const dynamicRouteFiles = import.meta.glob("./modules/**/*.ts", { eager: true });
const dynamicRoutes = mergeRouteModules(dynamicRouteFiles);
```

每个模块文件应 `export default` 一个 `RouteRecordRaw[]` 数组。

---

### `mergeHiddenRoutes(backendRoutes, staticRoutes)`

后端模式下，将静态路由中 `meta.hideInMenu = true` 的子路由合并到后端路由树。

后端 API 只返回菜单可见的路由，但隐藏页面（如详情页 `users/detail/:id`）需要被保留以支持直接 URL 访问。

```ts
// 按 name 匹配父路由，将隐藏子路由追加到 children
resultRoutes = mergeHiddenRoutes(resultRoutes, staticRoutes);
```

---

### `sortRoutes(routes)`

递归按 `meta.order` 升序排序路由树。未设置 `order` 的路由排到最后。

```ts
const sorted = sortRoutes(accessibleRoutes);
```

---

### `resetStaticRoutes(router, staticRoutes)`

重置 Vue Router，移除所有动态添加的路由，只保留静态路由。

用于登出后清理路由状态。

```ts
// src/router/index.ts 中的用法
const resetRoutes = () => resetStaticRoutes(router, routes);
```

---

## 如何添加新的业务路由

### 1. 创建路由模块文件

在 `src/router/routes/modules/app/` 下创建新文件：

```ts
// src/router/routes/modules/app/my-module.ts
import type { RouteRecordRaw } from "vue-router";
import { Layout } from "@/layouts";

const routes: RouteRecordRaw[] = [
  {
    path: "/my-module",
    name: "MyModule",
    component: Layout,
    redirect: "/my-module/list",
    meta: {
      order: 3000,
      icon: "lucide:box",
      title: "routes.myModule.name",
      authority: ["sys:platform_admin"],
    },
    children: [
      {
        path: "list",
        name: "MyModuleList",
        component: () => import("@/pages/app/my-module/list/index.vue"),
        meta: {
          title: "routes.myModule.list",
          icon: "lucide:list",
          authority: ["sys:platform_admin"],
        },
      },
      {
        path: "detail/:id",
        name: "MyModuleDetail",
        component: () => import("@/pages/app/my-module/detail/index.vue"),
        meta: {
          title: "routes.myModule.detail",
          hideInMenu: true,   // 不在菜单中显示，但仍可访问
          authority: ["sys:platform_admin"],
        },
      },
    ],
  },
];

export default routes;
```

> `import.meta.glob("./modules/**/*.ts", { eager: true })` 会自动扫描并加载。

### 2. 添加 i18n 翻译

在语言包中添加对应的 key：

```json
{
  "routes": {
    "myModule": {
      "name": "我的模块",
      "list": "列表管理",
      "detail": "详情页"
    }
  }
}
```

### 3. 配置权限

在后台权限管理中，为角色分配对应的权限码（如 `sys:platform_admin`）。

---

## RouteMeta 常用字段速查

| 字段 | 类型 | 说明 |
|---|---|---|
| `title` | `string` | **必填**。菜单/标签页/面包屑标题，支持 i18n key |
| `icon` | `string \| Component` | 菜单图标，推荐 Iconify 格式 `lucide:xxx` |
| `order` | `number` | 排序权重，值越小越靠前 |
| `authority` | `string[]` | 权限标识（角色码或权限码），匹配任一项即可访问 |
| `hideInMenu` | `boolean` | 不在菜单中显示 |
| `hideInTab` | `boolean` | 不在标签页中显示 |
| `hideInBreadcrumb` | `boolean` | 不在面包屑中显示 |
| `hideChildrenInMenu` | `boolean` | 子级不在菜单中显示（自身显示） |
| `keepAlive` | `boolean` | 开启 KeepAlive 缓存 |
| `affixTab` | `boolean` | 固定标签页（不可关闭） |
| `ignoreAccess` | `boolean` | 忽略权限检查，直接可访问 |
| `activePath` | `string` | 菜单高亮路径（用于详情页等隐藏路由指向父级菜单） |
| `link` | `string` | 外链跳转路径 |
| `iframeSrc` | `string` | 内嵌 iframe 地址 |
| `menuVisibleWithForbidden` | `boolean` | 菜单可见但访问跳 403 |

---

## 导入路径

```ts
// 统一从 @/core/router 导入
import { generateAccessible } from "@/core/router";
import { generateMenus, generateRoutesByFrontend, generateRoutesByBackend } from "@/core/router";
import { mergeRouteModules, mergeHiddenRoutes, sortRoutes, resetStaticRoutes } from "@/core/router";

// 类型从全局类型声明中获取（无需手动导入）
// RouteMeta, RouteRecordStringComponent, GenerateMenuAndRoutesOptions, ComponentRecordType
```
