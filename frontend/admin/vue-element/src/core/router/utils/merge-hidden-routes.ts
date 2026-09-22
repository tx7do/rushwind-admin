import { cloneDeep, mapTree } from "@/utils";

import type { RouteRecordRaw } from "vue-router";

/**
 * 将静态路由中 hideInMenu 的子路由合并到后端生成的路由树中。
 * 后端模式只返回菜单可见的路由，但某些隐藏页面（如详情页）
 * 定义在静态路由中需要被保留。
 */
export function mergeHiddenRoutes(
  backendRoutes: RouteRecordRaw[],
  staticRoutes: RouteRecordRaw[]
): RouteRecordRaw[] {
  // 收集静态路由中所有 hideInMenu 的子路由，按父路由 name 分组
  const hiddenChildren = new Map<string, RouteRecordRaw[]>();
  for (const staticRoute of staticRoutes) {
    if (!staticRoute.children) continue;
    const hiddens = staticRoute.children.filter((child) => child.meta?.hideInMenu);
    if (hiddens.length > 0 && staticRoute.name) {
      hiddenChildren.set(String(staticRoute.name), hiddens);
    }
  }

  if (hiddenChildren.size === 0) return backendRoutes;

  // 遍历后端路由树，将隐藏子路由合并到匹配的父路由下
  return mapTree(backendRoutes, (route) => {
    const routeName = String(route.name ?? "");
    const hiddens = hiddenChildren.get(routeName);
    if (hiddens) {
      route.children = [...(route.children || []), ...cloneDeep(hiddens)];
    }
    return route;
  });
}
