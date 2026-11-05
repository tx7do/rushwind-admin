import { apiClient } from "@/api/client";

// ==============================
// 扩展渠道 schema（配置表单动态渲染的数据源）
// ==============================

export interface NotificationChannelField {
  name?: string;
  label?: string;
  type?: string; // text | textarea | number | password | select
  required?: boolean;
  sensitive?: boolean;
  placeholder?: string;
  hint?: string;
  options?: string[];
}

export interface NotificationChannelSchema {
  type?: string;
  group?: string; // IM | PUSH | MOBILE | SMS | MAIL | AUTO | OPS
  label?: string;
  targetLabel?: string;
  needsTarget?: boolean;
  fields?: NotificationChannelField[];
}

/** 全量扩展渠道 schema；SDK 无专用类型时按响应结构声明。 */
export async function fetchListChannelSchemas() {
  const resp = (await apiClient.notificationChannelService.ListChannelSchemas(
    {} as any,
  )) as { items?: NotificationChannelSchema[] };
  return resp.items ?? [];
}
