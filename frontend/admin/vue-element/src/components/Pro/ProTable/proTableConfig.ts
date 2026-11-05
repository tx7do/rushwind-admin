import type { ProTableConfig } from "./types";
import { DEFAULT_PAGE_SIZE, DEFAULT_PAGE_SIZES } from "@/components/Pro";

export const proTableGlobalConfig: ProTableConfig = {
  engine: "vxe",
  rowKey: "id",
  // vxe 引擎行高由全局 _vxe-table.scss 的 --vxe-ui-table-row-height-* 变量控制
  // （34px，同时是主表与固定列克隆表行高对齐的机制，改值前读该文件块注释）；
  // 此处字段仅 el-table 引擎兼容用
  rowHeight: 30,
  headerHeight: 30,
  border: "none",
  stripe: true,
  borderRadius: 6,
  fontSize: 13,
  emptyText: "common.table.noData",

  pagination: {
    show: true,
    pageSizes: DEFAULT_PAGE_SIZES,
    pageSize: DEFAULT_PAGE_SIZE,
    layout: "total, sizes, ->, prev, pager, next",
    background: true,
  },

  column: {
    align: "center",
    resizable: true,
    showOverflowTooltip: true,
    sortable: false,
  },

  toolButton: {
    size: "small",
    link: true,
    iconSize: 16,
  },
};

export function mergeTableConfig(
  global: ProTableConfig,
  component?: Partial<ProTableConfig>
): ProTableConfig {
  return {
    ...global,
    ...component,
    pagination: { ...global.pagination, ...component?.pagination },
    column: { ...global.column, ...component?.column },
    toolButton: { ...global.toolButton, ...component?.toolButton },
  };
}
