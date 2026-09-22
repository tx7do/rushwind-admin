import { cloneDeep, mapTree } from "@/utils";
import { mergeHiddenRoutes, sortRoutes } from "@/core/router/utils";
import {
  generateMenus,
  generateRoutesByBackend,
  generateRoutesByFrontend,
} from "@/core/router/generators";
import { AccessModeType } from "@/core/preferences";
import type { GenerateMenuAndRoutesOptions } from "./types";
import type { RouteRecordRaw } from "vue-router";

async function generateAccessible(mode: AccessModeType, options: GenerateMenuAndRoutesOptions) {
  const { router } = options;

  options.routes = cloneDeep(options.routes);
  // 生成路由
  const accessibleRoutes = await generateRoutes(mode, options);

  // 动态添加到router实例内
  accessibleRoutes.forEach((route) => {
    router.addRoute(route);
  });

  const sortedRoutes = sortRoutes(accessibleRoutes);

  // 生成菜单
  const accessibleMenus = await generateMenus(sortedRoutes, options.router);

  // 将排序后的路由存入 Store（侧边栏渲染使用的是 accessRoutes）
  return { accessibleMenus, accessibleRoutes: sortedRoutes };
}

/**
 * Generate routes
 * @param mode
 * @param options
 */
async function generateRoutes(mode: AccessModeType, options: GenerateMenuAndRoutesOptions) {
  const { forbiddenComponent, roles, routes } = options;

  let resultRoutes: RouteRecordRaw[] = routes;
  switch (mode) {
    case "backend": {
      resultRoutes = await generateRoutesByBackend(options);
      // 将静态路由中 hideInMenu 的子路由合并到后端生成的路由树中
      resultRoutes = mergeHiddenRoutes(resultRoutes, routes);
      break;
    }
    case "frontend": {
      resultRoutes = await generateRoutesByFrontend(
        routes,
        roles || [],
        forbiddenComponent,
        options.accessCodes
      );
      break;
    }
  }

  /**
   * 调整路由树，做以下处理：
   * 1. 对未添加redirect的路由添加redirect
   */
  resultRoutes = mapTree(resultRoutes, (route) => {
    // 如果有redirect或者没有子路由，则直接返回
    if (route.redirect || !route.children || route.children.length === 0) {
      return route;
    }
    const firstChild = route.children[0];

    // 如果子路由不是以/开头，则直接返回,这种情况需要计算全部父级的path才能得出正确的path，这里不做处理
    if (!firstChild?.path || !firstChild.path.startsWith("/")) {
      return route;
    }

    route.redirect = firstChild.path;
    return route;
  });

  return resultRoutes;
}

export { generateAccessible };
