// Pro/index.ts
export { default as ProForm } from "./ProForm/index.vue";
export { default as ProTable } from "./ProTable/index.vue";
export { default as ProModal } from "./ProModal/index.vue";
export { default as ProSearch } from "./ProSearch/index.vue";
export { default as ProPagination } from "./ProPagination/index.vue";
export { default as ProToolbar } from "./ProToolbar/index.vue";
export { default as ProPage } from "./ProPage/index.vue";
export { default as ProFileSelect } from "./ProFileSelect/index.vue";

// 类型导出
export * from "./ProForm/types";
export * from "./ProTable/types";
export * from "./ProModal/types";
export * from "./ProSearch/types";
export * from "./ProPagination/types";
export * from "./ProToolbar/types";
export * from "./ProPage/types";
export * from "./ProFileSelect/types";

// Composables
export { useTableState } from "./composables/useTableState";
export type { UseTableConfig } from "./composables/useTableState";
export { useModalState } from "./composables/useModalState";

// Level 2: Api 命令式控制
export { ProPageApi } from "./composables/ProPageApi";
export type { ProPageExpose, ProPageState } from "./composables/ProPageApi";
export { useProPage } from "./composables/useProPage";
export type { UseProPage } from "./composables/useProPage";

// Level 2.5: 弹窗抽离（connectedComponent）
export { ProModalApi } from "./composables/useProModal";
export type { ProModalState, UseProModalOptions } from "./composables/useProModal";
export { useProModal, injectProModalApi } from "./composables/useProModal";
export type { UseProModal } from "./composables/useProModal";

// 常量
export { DEFAULT_CURRENT_PAGE, DEFAULT_PAGE_SIZE, DEFAULT_PAGE_SIZES } from "./constants";
