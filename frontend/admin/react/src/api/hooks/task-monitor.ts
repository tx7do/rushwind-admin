import { useQuery, type UseQueryOptions } from '@tanstack/react-query';
import type {
  taskservicev1_InspectSystemTasksResponse,
} from '@/api/generated/admin/service/v1';
import { apiClient } from '@/api/client';

// ==============================
// 系统级常驻任务监控（只读 asynq Inspector，方案 C'）
// ==============================

const INSPECT_KEY = 'inspectSystemTasks';

/**
 * 一站式巡检：调度条目（cron/上次/下次入队）+ 各任务类型的队列状态与失败明细。
 * 数据来自 asynq 本身（Redis），零新表；Prev 为空 = 注册后从未运行。
 */
export function useInspectSystemTasks(
  options?: Omit<
    UseQueryOptions<taskservicev1_InspectSystemTasksResponse, Error>,
    'queryKey' | 'queryFn'
  >,
) {
  return useQuery({
    queryKey: [INSPECT_KEY],
    queryFn: () => apiClient.taskMonitorService.InspectSystemTasks({}),
    staleTime: 0,
    ...options,
  });
}
