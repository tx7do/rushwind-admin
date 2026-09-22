<template>
  <div class="app-container h-full flex flex-1 flex-col">
    <ProPage ref="pageRef" :config="pageConfig" @add="handleAdd" @edit="handleEdit">
      <!-- 值类型 -->
      <template #valueType="scope: any">
        <ElTag size="small" effect="dark" round :color="configValueTypeToColor(scope.row.valueType)">
          {{ configValueTypeToName(scope.row.valueType) }}
        </ElTag>
      </template>

      <!-- 内置参数 -->
      <template #isBuiltIn="scope: any">
        <ElTag size="small" effect="plain" round :type="scope.row.isBuiltIn ? 'warning' : 'info'">
          {{ scope.row.isBuiltIn ? $t("pages.config.builtInTag") : $t("pages.config.customTag") }}
        </ElTag>
      </template>
    </ProPage>

    <!-- 新增/编辑抽屉 -->
    <ConfigDrawer ref="drawerRef" @success="handleSuccess" />
  </div>
</template>

<script lang="ts" setup>
import { computed, ref } from "vue";
import { ElTag } from "element-plus";

import ProPage from "@/components/Pro/ProPage/index.vue";
import type { ProPageConfig } from "@/components/Pro/ProPage/types";
import ConfigDrawer from "./config-drawer.vue";

import {
  configValueTypeToColor,
  configValueTypeToName,
  fetchListConfigs,
  useDeleteConfig,
  createPagedExportAction,
} from "@/api/composables";
import { PaginationQuery } from "@/core/transport/rest";
import { $t } from "@/core/i18n";

const { mutateAsync: deleteConfig } = useDeleteConfig();

const pageRef = ref();
const drawerRef = ref();

const pageConfig = computed<ProPageConfig>(() => ({
  skeleton: true,
  search: {
    grid: true,
    fields: [
      {
        type: "input",
        label: $t("pages.config.name"),
        field: "name",
        attrs: { placeholder: $t("common.placeholder.input"), clearable: true },
      },
      {
        type: "input",
        label: $t("pages.config.key"),
        field: "key",
        attrs: { placeholder: $t("common.placeholder.input"), clearable: true },
      },
    ],
  },

  table: {
    listAction: async (query: any) => {
      const { page, pageSize, ...queryParams } = query;
      const result = await fetchListConfigs(
        new PaginationQuery({
          paging: { page: page || 1, pageSize: pageSize || 10 },
          formValues: queryParams,
        })
      );
      return { items: result.items || [], total: result.total || 0 };
    },
    deleteAction: async (ids: string) => {
      await deleteConfig({ id: ids as any });
    },
    exportsAction: createPagedExportAction(fetchListConfigs),
    toolbar: [],
    toolbarRight: ["add"],
    defaultToolbar: ["refresh", "filter", "exports"],
    tableAttrs: { border: true, stripe: true },
    emptyActionText: "common.button.add",
    columns: [
      { type: "index", label: $t("common.table.seq"), width: 60 },
      { prop: "name", label: $t("pages.config.name"), minWidth: 160 },
      { prop: "key", label: $t("pages.config.key"), minWidth: 200 },
      { prop: "value", label: $t("pages.config.value"), minWidth: 140, showOverflow: "title" },
      {
        prop: "valueType",
        label: $t("pages.config.valueType"),
        minWidth: 100,
        slotName: "valueType",
      },
      {
        prop: "isBuiltIn",
        label: $t("pages.config.isBuiltIn"),
        minWidth: 100,
        slotName: "isBuiltIn",
      },
      {
        prop: "createdAt",
        label: $t("common.table.createdAt"),
        minWidth: 160,
        cellType: "date",
        dateFormat: "YYYY-MM-DD HH:mm:ss",
      },
      {
        prop: "action",
        label: $t("common.table.action"),
        fixed: "right",
        width: 150,
        cellType: "tool",
        buttons: [
          { name: "edit", label: $t("common.button.edit"), icon: "lucide:pen-line" },
          {
            name: "delete",
            label: $t("common.button.delete"),
            icon: "lucide:trash-2",
            attrs: { type: "danger" },
          },
        ],
      },
    ],
  },
}));

function handleAdd() {
  drawerRef.value?.open({ create: true });
}

function handleEdit(row: any) {
  drawerRef.value?.open({ create: false, row });
}

function handleSuccess() {
  pageRef.value?.refresh();
}
</script>

<style lang="scss" scoped>
.app-container {
  padding: 20px;
  width: 100%;
  min-width: 0;
  flex-shrink: 0;
}
</style>
