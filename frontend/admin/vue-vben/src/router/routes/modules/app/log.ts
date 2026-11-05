import type { RouteRecordRaw } from 'vue-router';

import { BasicLayout } from '#/layouts';
import { $t } from '#/locales';

const log: RouteRecordRaw[] = [
  {
    path: '/log',
    name: 'LogAuditManagement',
    component: BasicLayout,
    redirect: '/log/login-audit-logs',
    meta: {
      order: 2004,
      icon: 'lucide:logs',
      title: $t('menu.log.moduleName'),
      keepAlive: true,
      authority: ['sys:platform_admin'],
    },
    children: [
      {
        path: 'login-audit-logs',
        name: 'LoginAuditLog',
        meta: {
          order: 1,
          icon: 'lucide:user-lock',
          title: $t('menu.log.loginAuditLog'),
          authority: ['sys:platform_admin'],
        },
        component: () => import('#/views/app/log/login_audit_log/index.vue'),
      },

      {
        path: 'api-audit-logs',
        name: 'ApiAuditLog',
        meta: {
          order: 2,
          icon: 'lucide:file-clock',
          title: $t('menu.log.apiAuditLog'),
          authority: ['sys:platform_admin'],
        },
        component: () => import('#/views/app/log/api_audit_log/index.vue'),
      },

      {
        path: 'operation-audit-logs',
        name: 'OperationAuditLog',
        meta: {
          order: 3,
          icon: 'lucide:shield-ellipsis',
          title: $t('menu.log.operationAuditLog'),
          authority: ['sys:platform_admin'],
        },
        component: () =>
          import('#/views/app/log/operation_audit_log/index.vue'),
      },

      {
        path: 'data-access-audit-logs',
        name: 'DataAccessAuditLog',
        meta: {
          order: 4,
          icon: 'lucide:shield-check',
          title: $t('menu.log.dataAccessAuditLog'),
          authority: ['sys:platform_admin'],
        },
        component: () =>
          import('#/views/app/log/data_access_audit_log/index.vue'),
      },

      {
        path: 'permission-audit-logs',
        name: 'PermissionAuditLog',
        meta: {
          order: 5,
          icon: 'lucide:shield-alert',
          title: $t('menu.log.permissionAuditLog'),
          authority: ['sys:platform_admin'],
        },
        component: () =>
          import('#/views/app/log/permission_audit_log/index.vue'),
      },

      {
        path: 'policy-evaluation-logs',
        name: 'PolicyEvaluationLog',
        meta: {
          order: 6,
          icon: 'lucide:gavel',
          title: $t('menu.log.policyEvaluationLog'),
          authority: ['sys:platform_admin'],
        },
        component: () =>
          import('#/views/app/log/policy_evaluation_log/index.vue'),
      },

      {
        path: 'redis-cache-monitor',
        name: 'RedisCacheMonitor',
        meta: {
          order: 7,
          icon: 'lucide:database',
          title: $t('menu.log.redisCacheMonitor'),
          authority: ['sys:platform_admin'],
        },
        component: () => import('#/views/app/log/redis_cache_monitor/index.vue'),
      },
    ],
  },
];

export default log;
