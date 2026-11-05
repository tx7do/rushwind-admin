import type { RouteRecordRaw } from "vue-router";

import { Layout } from "@/layouts";

/**
 * AI 域路由：对话助手（登录可用）+ 模型提供商管理（平台管理员）。
 * 与 react 基准同构：providers 页挂 sys:platform_admin，chat 页不加 authority。
 */
const ai: RouteRecordRaw[] = [
  {
    path: "/ai",
    name: "AiAssistant",
    redirect: "/ai/chat",
    component: Layout,
    meta: {
      order: 2007,
      icon: "lucide:sparkles",
      title: "routes.ai.moduleName",
      keepAlive: true,
    },
    children: [
      {
        path: "chat",
        name: "AiChat",
        meta: {
          order: 1,
          icon: "lucide:message-circle",
          title: "routes.ai.chat",
          keepAlive: true,
        },
        component: () => import("@/pages/app/ai/chat/index.vue"),
      },
      {
        path: "knowledge",
        name: "AiKnowledgeManagement",
        meta: {
          order: 3,
          icon: "lucide:book-open",
          title: "routes.ai.knowledge",
          authority: ["sys:platform_admin"],
        },
        component: () => import("@/pages/app/ai/knowledge/index.vue"),
      },
      {
        path: "query",
        name: "AiQuery",
        meta: {
          order: 2,
          icon: "lucide:search-code",
          title: "routes.ai.query",
          authority: ["sys:platform_admin"],
        },
        component: () => import("@/pages/app/ai/query/index.vue"),
      },
      {
        path: "agents",
        name: "AiAgentApps",
        meta: {
          order: 5,
          icon: "lucide:workflow",
          title: "routes.ai.agents",
          keepAlive: true,
        },
        component: () => import("@/pages/app/ai/agent/index.vue"),
      },
      {
        path: "usage",
        name: "AiUsage",
        meta: {
          order: 4,
          icon: "lucide:bar-chart-3",
          title: "routes.ai.usage",
        },
        component: () => import("@/pages/app/ai/usage/index.vue"),
      },
      {
        path: "providers",
        name: "AiProviderManagement",
        meta: {
          // 与 react 基准一致：1-5 连号，配置类 providers 收尾（原误标 2 与 query 撞号）
          order: 6,
          icon: "lucide:cpu",
          title: "routes.ai.providers",
          authority: ["sys:platform_admin"],
        },
        component: () => import("@/pages/app/ai/provider/index.vue"),
      },
    ],
  },
];

export default ai;
