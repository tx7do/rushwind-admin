import { apiClient } from '#/api/client';

/**
 * 扩展渠道 schema（配置表单动态渲染的数据源）。
 * 字段结构与 go-utils/notify 渠道声明一一对应（见 docs/notify_channels_expansion.md）。
 */
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

/** 全量扩展渠道 schema；失败返回空数组（EXT 下拉缺项不阻塞渠道 CRUD）。 */
export async function fetchListChannelSchemas(): Promise<NotificationChannelSchema[]> {
  try {
    const resp = (await apiClient.notificationChannelService.ListChannelSchemas(
      {},
    )) as { items?: NotificationChannelSchema[] };
    return resp.items ?? [];
  } catch {
    return [];
  }
}

/** settings 读回显 JSON 容错解析：坏数据返回空对象，动态表单按全空处理。 */
export function parseSettingsSafe(raw?: string): Record<string, any> {
  if (!raw) return {};
  try {
    const parsed = JSON.parse(raw);
    return typeof parsed === 'object' && parsed !== null ? parsed : {};
  } catch {
    return {};
  }
}

/** schema 分组中文名（类型下拉的分组标题）。 */
export const EXT_GROUP_LABELS: Record<string, string> = {
  IM: 'IM / 群聊',
  PUSH: '个人推送',
  MOBILE: '移动推送',
  SMS: '短信 / 消息',
  MAIL: '邮件 API',
  AUTO: '效率自动化',
  OPS: '运维告警',
};
