import type { Component } from "vue";
import type { RouteRecordRaw, Router } from "vue-router";

// ==================== 路由相关 ====================

/** 定义递归类型以将 RouteRecordRaw 的 component 属性更改为 string */
export type RouteRecordStringComponent<T = string> = {
  children?: RouteRecordStringComponent<T>[];
  component: T;
} & Omit<RouteRecordRaw, "children" | "component">;

export type ComponentRecordType = Record<string, () => Promise<Component>>;

export interface GenerateMenuAndRoutesOptions {
  fetchMenuListAsync?: () => Promise<RouteRecordStringComponent[]>;
  forbiddenComponent?: RouteRecordRaw["component"];
  layoutMap?: ComponentRecordType;
  pageMap?: ComponentRecordType;
  /** 用户角色码（来自 getMe().roles） */
  roles?: string[];
  /** 用户权限码（来自 GetMyPermissionCode） */
  accessCodes?: string[];
  router: Router;
  routes: RouteRecordRaw[];
}

// ==================== 菜单相关 ====================

/** 扩展路由原始对象 */
export type ExRouteRecordRaw = {
  parent?: string;
  parents?: string[];
  path?: any;
} & RouteRecordRaw;

export interface MenuRecordBadgeRaw {
  /** 徽标 */
  badge?: string;
  /** 徽标类型 */
  badgeType?: "dot" | "normal";
  /** 徽标颜色 */
  badgeVariants?: "destructive" | "primary" | string;
}

/** 菜单原始对象 */
export interface MenuRecordRaw extends MenuRecordBadgeRaw {
  /** 激活时的图标名 */
  activeIcon?: string;
  /** 子菜单 */
  children?: MenuRecordRaw[];
  /** 是否禁用菜单 @default false */
  disabled?: boolean;
  /** 图标名 */
  icon?: Component | string;
  /** 菜单名 */
  name: string;
  /** 排序号 */
  order?: number;
  /** 父级路径 */
  parent?: string;
  /** 所有父级路径 */
  parents?: string[];
  /** 菜单路径，唯一，可当作key */
  path: string;
  /** 是否显示菜单 @default true */
  show?: boolean;
}
