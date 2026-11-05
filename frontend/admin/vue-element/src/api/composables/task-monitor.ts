import type {
  taskservicev1_InspectSystemTasksRequest,
  taskservicev1_InspectSystemTasksResponse,
} from "@/api/generated/admin/service/v1";
import { apiClient } from "@/api/client";
import { queryClient } from "@/plugins/vue-query";
import { useMutation, type UseMutationOptions } from "@tanstack/vue-query";

// ==============================
// 系统级常驻任务监控（只读 asynq Inspector，方案 C' 零新表）
// ==============================

const INSPECT_KEY = "inspectSystemTasks";

/** 一站式巡检：调度条目（cron/上次/下次入队）+ 各任务类型的队列状态与失败明细 */
export function useInspectSystemTasks(
  options?: UseMutationOptions<
    taskservicev1_InspectSystemTasksResponse,
    Error,
    taskservicev1_InspectSystemTasksRequest
  >
) {
  return useMutation({
    mutationFn: (req) => apiClient.taskMonitorService.InspectSystemTasks(req),
    onSuccess: () =>
      queryClient.invalidateQueries({ queryKey: [INSPECT_KEY] }),
    ...options,
  });
}

/** 手动巡检（非 Hook）：弹窗打开时直接调用 */
export async function fetchInspectSystemTasks(): Promise<taskservicev1_InspectSystemTasksResponse> {
  return queryClient.fetchQuery({
    queryKey: [INSPECT_KEY, "manual"],
    queryFn: () => apiClient.taskMonitorService.InspectSystemTasks({}),
    staleTime: 0,
    retry: 0,
  });
}
