<template>
  <template v-if="col.formatter">
    {{ col.formatter(row, col) }}
  </template>
  <component
    :is="renderer"
    v-else-if="renderer"
    :col="col"
    :row="row"
    :field="field"
    :row-index="rowIndex"
    @modify="(d: any) => emit('modify', d)"
    @operate="(d: any) => emit('operate', d)"
  />
  <span v-else-if="field" :class="{ 'pro-table__empty-cell': isEmptyValue(row[field]) }">
    {{ isEmptyValue(row[field]) ? "-" : row[field] }}
  </span>
  <span v-else></span>
</template>

<script setup lang="ts">
import { computed } from "vue";
import type { Component } from "vue";
import { getCellRenderer } from "./cellRendererRegistry";
import type { ProTableColumn } from "./types";

const props = defineProps<{
  col: ProTableColumn;
  row: any;
  rowIndex: number;
}>();

const emit = defineEmits<{
  modify: [data: { row: any; field: string; value: any }];
  operate: [data: { name: string; row: any; $index: number }];
}>();

const field = computed(() => props.col.prop ?? "");
const renderer = computed<Component | undefined>(() => {
  const type = props.col.cellType;
  if (!type || type === "custom") return undefined;
  return getCellRenderer(type);
});

// 判断是否为空值（null、undefined、空字符串）
function isEmptyValue(val: any): boolean {
  return val === null || val === undefined || val === "";
}
</script>

<style scoped lang="scss">
// 空值单元格样式：使用 Element Plus 主题 placeholder 文字色（亮/暗模式自动切换）
.pro-table__empty-cell {
  color: var(--el-text-color-placeholder);
}
</style>
