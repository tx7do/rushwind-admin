/**
 * 应用常量定义
 *
 * @description
 * 包含应用中所有的常量定义，包括角色、存储键名等
 */

/**
 * 应用存储前缀
 */
export const APP_PREFIX = "rushwind";

/**
 * 超级管理员角色标识
 *
 * @description
 * 拥有系统最高权限，可以访问所有资源
 */
export const ROLE_ROOT = "ROOT";

/**
 * 平台租户ID
 *
 * @description
 * 用于前端识别平台租户（不参与套餐/菜单配置）
 */
export const PLATFORM_TENANT_ID = 0;

/**
 * @zh_CN 登录页面 url 地址
 */
export const LOGIN_PATH = "/login";

/**
 * @zh_CN 默认首页地址
 */
export const DEFAULT_HOME_PATH = "/analytics";

/**
 * 抽屉通用宽度
 *
 * @description
 * 全系统标准抽屉宽度，与 Dialog 弹窗宽度统一
 * 复杂内容抽屉（菜单树、权限树等）可按需使用更宽的值
 */
export const DRAWER_WIDTH = "480px";
