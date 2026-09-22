import type { RouteRecordRaw } from "vue-router";
import { Layout } from "@/layouts";

const log: RouteRecordRaw[] = [
  {
    path: "/log",
    name: "LogAuditManagement",
    component: Layout,
    redirect: "/log/login-audit-logs",
    meta: {
      order: 2004,
      icon: "lucide:logs",
      title: "routes.log.moduleName",
      keepAlive: true,
      authority: ["sys:platform_admin"],
    },
    children: [
      {
        path: "login-audit-logs",
        name: "LoginAuditLog",
        meta: {
          icon: "lucide:user-lock",
          title: "routes.log.loginAuditLog",
          authority: ["sys:platform_admin"],
        },
        component: () => import("@/pages/app/log/login_audit_log/index.vue"),
      },

      {
        path: "api-audit-logs",
        name: "ApiAuditLog",
        meta: {
          icon: "lucide:file-clock",
          title: "routes.log.apiAuditLog",
          authority: ["sys:platform_admin"],
        },
        component: () => import("@/pages/app/log/api_audit_log/index.vue"),
      },

      {
        path: "operation-audit-logs",
        name: "OperationAuditLog",
        meta: {
          icon: "lucide:shield-ellipsis",
          title: "routes.log.operationAuditLog",
          authority: ["sys:platform_admin"],
        },
        component: () => import("@/pages/app/log/operation_audit_log/index.vue"),
      },

      {
        path: "data-access-audit-logs",
        name: "DataAccessAuditLog",
        meta: {
          icon: "lucide:shield-check",
          title: "routes.log.dataAccessAuditLog",
          authority: ["sys:platform_admin"],
        },
        component: () => import("@/pages/app/log/data_access_audit_log/index.vue"),
      },

      {
        path: "permission-audit-logs",
        name: "PermissionAuditLog",
        meta: {
          icon: "lucide:shield-alert",
          title: "routes.log.permissionAuditLog",
          authority: ["sys:platform_admin"],
        },
        component: () => import("@/pages/app/log/permission_audit_log/index.vue"),
      },

      {
        path: "policy-evaluation-logs",
        name: "PolicyEvaluationLog",
        meta: {
          icon: "lucide:gavel",
          title: "routes.log.policyEvaluationLog",
          authority: ["sys:platform_admin"],
        },
        component: () => import("@/pages/app/log/policy_evaluation_log/index.vue"),
      },

      {
        path: "redis-cache-monitor",
        name: "RedisCacheMonitor",
        meta: {
          icon: "lucide:database",
          title: "routes.log.redisCacheMonitor",
          authority: ["sys:platform_admin"],
        },
        component: () => import("@/pages/app/log/redis_cache_monitor/index.vue"),
      },
    ],
  },
];

export default log;
