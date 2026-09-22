import type { RouteRecordRaw } from "vue-router";

import { DEFAULT_HOME_PATH, LOGIN_PATH } from "@/constants";

import Login from "@/pages/core/login/index.vue";
import { Layout } from "@/layouts";

/** 全局网络异常页 */
const errorRoute: RouteRecordRaw = {
  component: () => import("@/pages/core/error/offline.vue"),
  meta: {
    hideInBreadcrumb: true,
    hideInMenu: true,
    hideInTab: true,
    title: "offline",
    ignoreAccess: true,
  },
  name: "FallbackOffline",
  path: "/offline",
};

/** 全局404页面 */
const fallbackNotFoundRoute: RouteRecordRaw = {
  component: () => import("@/pages/core/error/404.vue"),
  meta: {
    hideInBreadcrumb: true,
    hideInMenu: true,
    hideInTab: true,
    title: "404",
  },
  name: "FallbackNotFound",
  path: "/:path(.*)*",
};

/** 基本路由，这些路由是必须存在的 */
const coreRoutes: RouteRecordRaw[] = [
  {
    meta: {
      title: "Root",
    },
    name: "Root",
    path: "/",
    redirect: DEFAULT_HOME_PATH,
  },
  {
    component: Layout,
    meta: {
      hideInTab: true,
      title: "Authentication",
    },
    name: "Authentication",
    path: "/auth",
    redirect: LOGIN_PATH,
    children: [
      {
        name: "Login",
        path: "login",
        component: Login,
        meta: {
          title: "pages.auth.login",
        },
      },
      {
        name: "MfaChallenge",
        path: "mfa-challenge",
        component: () => import("@/pages/core/login/components/MfaChallenge.vue"),
        meta: {
          title: "pages.auth.mfaChallenge",
          hideInBreadcrumb: true,
          hideInMenu: true,
          hideInTab: true,
        },
      },
    ],
  },
  {
    name: "Login",
    path: LOGIN_PATH,
    component: Login,
    meta: {
      title: "pages.auth.login",
      ignoreAccess: true,
    },
  },
];

export { coreRoutes, errorRoute, fallbackNotFoundRoute };
