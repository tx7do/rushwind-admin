import { useQuery, type UseQueryOptions } from '@tanstack/react-query';
import {
  type notification_channelservicev1_ChannelSchema,
  type notification_channelservicev1_ListChannelSchemasResponse,
} from '@/api/generated/admin/service/v1';
import { apiClient } from '@/api/client';

// ==============================
// 扩展渠道 schema（配置表单动态渲染的数据源）
// ==============================

export function useListChannelSchemas(
  options?: UseQueryOptions<notification_channelservicev1_ListChannelSchemasResponse, Error>,
) {
  return useQuery({
    queryKey: ['listChannelSchemas'],
    queryFn: () => apiClient.notificationChannelService.ListChannelSchemas({}),
    staleTime: 5 * 60 * 1000,
    ...options,
  });
}

export type { notification_channelservicev1_ChannelSchema as ChannelSchema };
