import { PLATFORM_TENANT_ID } from "@/constants";

/**
 * 判断是否平台租户
 *
 * @description
 * 平台租户不参与套餐与菜单配置
 */
export const isPlatformTenantId = (tenantId?: string | number) =>
  Number(tenantId) === PLATFORM_TENANT_ID;
