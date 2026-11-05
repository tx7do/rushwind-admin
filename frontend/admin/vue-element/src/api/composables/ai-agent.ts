import { apiClient } from "@/api/client";
import { PaginationQuery } from "@/core/transport/rest";

// ==============================
// AI Agent 编排（可视化画布应用）
// ==============================

/** 分页查询 Agent 应用列表 */
export async function fetchListAiAgentApps(query: PaginationQuery) {
  return apiClient.aiAgentService.List(query.toRawParams());
}

/** 创建 Agent 应用（graph 允许空串=先建壳后画图） */
export async function createAiAgentApp(data: Record<string, any>) {
  return apiClient.aiAgentService.Create({ data: data as any });
}

/** 更新 Agent 应用；updateMask 显式列举业务字段 */
export async function updateAiAgentApp(
  id: number,
  values: Record<string, any>,
  updateMask?: string,
) {
  return apiClient.aiAgentService.Update({
    id,
    data: values as any,
    updateMask,
  });
}

/** 删除 Agent 应用 */
export async function deleteAiAgentApp(id: number) {
  return apiClient.aiAgentService.Delete({ id });
}

/** 试运行：同步执行图并返回含 trace 的运行记录 */
export async function runAiAgentApp(id: number, input: Record<string, string>) {
  return apiClient.aiAgentService.Run({ id, input });
}

/** 某应用的运行台账（appId eq 过滤） */
export async function fetchListAiAgentRuns(query: PaginationQuery) {
  return apiClient.aiAgentService.ListRuns(query.toRawParams());
}
