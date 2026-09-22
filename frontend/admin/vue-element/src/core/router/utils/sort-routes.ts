import type { RouteRecordRaw } from "vue-router";

/**
 * 递归排序路由树（按 meta.order 升序）
 */
export const sortRoutes = (routes: RouteRecordRaw[]): RouteRecordRaw[] => {
  return routes
    .map((route) => ({
      ...route,
      children: route.children?.length ? sortRoutes(route.children) : undefined,
    }))
    .sort((a, b) => {
      const orderA = (a.meta as any)?.order ?? Number.MAX_SAFE_INTEGER;
      const orderB = (b.meta as any)?.order ?? Number.MAX_SAFE_INTEGER;
      return orderA - orderB;
    }) as RouteRecordRaw[];
};
