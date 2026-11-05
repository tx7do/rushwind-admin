import { apiClient } from "@/api/client";
import { useQuery, type UseQueryOptions } from "@tanstack/vue-query";

/**
 * 租户自助用量：当前操作者所属租户的套餐用量与配额（服务端钉定租户，
 * 不接受传参）。配额硬限制（USER_LIMIT/STORAGE 403）发生时，租户管理员
 * 在此看到原因。平台用户（tenantId==0）返回空 Usage，调用方不展示。
 */
export function useMyTenantUsage(
  options?: UseQueryOptions<
    Awaited<ReturnType<typeof apiClient.myTenantUsageService.GetMyTenantUsage>>,
    Error
  >
) {
  return useQuery({
    queryKey: ["myTenantUsage"],
    queryFn: () => apiClient.myTenantUsageService.GetMyTenantUsage({}),
    staleTime: 60_000,
    ...options,
  });
}
