<template>
  <ProModal
    v-model:visible="visible"
    :title="title"
    :loading="pageLoading"
    :config="{
      component: 'drawer',
      drawer: { size: DRAWER_WIDTH, closeOnClickModal: false },
    }"
  >
    <ElForm
      ref="formRef"
      v-loading="pageLoading"
      :model="formData"
      :rules="formRules"
      label-width="120px"
      class="drawer-form"
    >
      <!-- 基本信息 -->
      <ElDivider content-position="left">{{ $t("common.section.basic") }}</ElDivider>

      <ElFormItem :label="$t('pages.role.name')" prop="name">
        <ElInput v-model="formData.name" :placeholder="$t('common.placeholder.input')" clearable />
      </ElFormItem>

      <ElFormItem :label="$t('pages.role.code')" prop="code">
        <ElInput v-model="formData.code" :placeholder="$t('common.placeholder.input')" clearable />
      </ElFormItem>

      <ElFormItem :label="$t('common.table.sortOrder')" prop="sortOrder">
        <ElInputNumber
          v-model="formData.sortOrder"
          :min="1"
          :placeholder="$t('common.placeholder.input')"
          style="width: 100%"
        />
      </ElFormItem>

      <ElFormItem :label="$t('common.table.status')" prop="status">
        <ElRadioGroup v-model="formData.status">
          <ElRadioButton v-for="item in statusList" :key="item.value" :value="item.value">
            {{ item.label }}
          </ElRadioButton>
        </ElRadioGroup>
      </ElFormItem>

      <ElFormItem :label="$t('pages.role.dataScope')" prop="dataScope">
        <ElSelect v-model="formData.dataScope" :placeholder="$t('common.placeholder.select')" style="width: 100%">
          <ElOption
            v-for="item in roleDataScopeList"
            :key="item.value"
            :label="item.label"
            :value="item.value"
          />
        </ElSelect>
      </ElFormItem>

      <!-- SELECTED_UNITS 档位的自定义授权单元集 -->
      <ElFormItem v-if="formData.dataScope === 'SELECTED_UNITS'" :label="$t('pages.role.orgUnits')">
        <template v-if="orgUnitTreeData.length > 0">
          <ElTree
            ref="unitTreeRef"
            :data="orgUnitTreeData"
            node-key="id"
            show-checkbox
            default-expand-all
            :props="{ label: 'name', children: 'children' }"
            style="max-height: 400px; overflow-y: auto"
          />
        </template>
        <div v-else class="org-unit-empty">{{ $t("pages.role.noOrgUnitData") }}</div>
      </ElFormItem>

      <ElFormItem :label="$t('common.table.description')" prop="description">
        <ElInput
          v-model="formData.description"
          type="textarea"
          :rows="3"
          :placeholder="$t('common.placeholder.input')"
        />
      </ElFormItem>

      <!-- 权限配置 -->
      <ElDivider content-position="left">{{ $t("pages.role.permissions") }}</ElDivider>

      <ElFormItem prop="permissions">
        <ElTree
          ref="permissionTreeRef"
          :data="permissionTreeData"
          node-key="key"
          show-checkbox
          default-expand-all
          :props="{ label: 'title', children: 'children' }"
          :filter-node-method="filterPermissionNode"
          style="max-height: 400px; overflow-y: auto"
        >
          <template #default="{ node, data }">
            <span class="custom-tree-node">
              <span>{{ node.label }}</span>
              <span v-if="data.key" class="text-xs text-gray-400 ml-2">{{ data.key }}</span>
            </span>
          </template>
        </ElTree>
      </ElFormItem>

      <!-- 字段权限：勾选 = 对该角色用户隐藏（黑名单语义，User 资源试点） -->
      <ElFormItem :label="$t('pages.role.fieldPerm.title')">
        <div class="field-perm-box">
          <div class="field-perm-hint">{{ $t("pages.role.fieldPerm.hint") }}</div>
          <ElCheckboxGroup v-model="userHiddenFields">
            <ElCheckbox v-for="item in fieldPermOptions" :key="item.value" :value="item.value">
              {{ item.label }}
            </ElCheckbox>
          </ElCheckboxGroup>
        </div>
      </ElFormItem>
    </ElForm>

    <template #footer>
      <div class="drawer-footer">
        <ElButton @click="handleClose">{{ $t("common.button.cancel") }}</ElButton>
        <ElButton type="primary" :loading="submitLoading" @click="handleSubmit">
          {{ $t("common.button.confirm") }}
        </ElButton>
      </div>
    </template>
  </ProModal>
</template>

<script lang="ts" setup>
import { computed, reactive, ref, nextTick } from "vue";
import { ElMessage } from "element-plus";
import type { TreeInstance } from "element-plus";

import ProModal from "@/components/Pro/ProModal/index.vue";

import {
  useCreateRole,
  useUpdateRole,
  fetchListPermissionGroups,
  fetchListPermissions,
  fetchListOrgUnits,
  statusList,
  roleDataScopeList,
  buildPermissionTree,
} from "@/api/composables";
import { PaginationQuery } from "@/core/transport/rest";
import { $t } from "@/core/i18n";
import { DRAWER_WIDTH } from "@/constants";

const emit = defineEmits<{
  success: [];
}>();

const { mutateAsync: createRole } = useCreateRole();
const { mutateAsync: updateRole } = useUpdateRole();

const visible = ref(false);
const submitLoading = ref(false);
const pageLoading = ref(false);
const isCreate = ref(true);
const currentId = ref<number>();
const formRef = ref();
const permissionTreeRef = ref<TreeInstance>();
const unitTreeRef = ref<TreeInstance>();

// 表单数据
const formData = reactive({
  name: "",
  code: "",
  sortOrder: 1,
  status: "ON",
  dataScope: "ALL",
  description: "",
  permissions: [] as number[],
});

// 组织单元树数据（SELECTED_UNITS 档位的自定义授权集）
const orgUnitTreeData = ref<any[]>([]);

// 字段权限：User 资源上勾选隐藏的字段（proto 字段 json_name）
const userHiddenFields = ref<string[]>([]);

// 可勾选字段：value 与后端 identity User proto 字段 json_name 逐字一致，
// 提交后经登录聚合写入令牌，命中字段在响应侧被裁剪。
const fieldPermOptions = [
  { value: "email", label: $t("pages.role.fieldPerm.field.email") },
  { value: "mobile", label: $t("pages.role.fieldPerm.field.mobile") },
  { value: "telephone", label: $t("pages.role.fieldPerm.field.telephone") },
  { value: "address", label: $t("pages.role.fieldPerm.field.address") },
  { value: "region", label: $t("pages.role.fieldPerm.field.region") },
  { value: "lastLoginAt", label: $t("pages.role.fieldPerm.field.lastLoginAt") },
  { value: "lastLoginIp", label: $t("pages.role.fieldPerm.field.lastLoginIp") },
];

// 表单验证规则
const formRules = {
  name: [{ required: true, message: $t("common.validation.required"), trigger: "blur" }],
  code: [{ required: true, message: $t("common.validation.required"), trigger: "blur" }],
  sortOrder: [{ required: true, message: $t("common.validation.required"), trigger: "blur" }],
  status: [{ required: true, message: $t("common.validation.selectRequired"), trigger: "change" }],
  dataScope: [{ required: true, message: $t("common.validation.selectRequired"), trigger: "change" }],
};

// 标题
const title = computed(() =>
  isCreate.value
    ? $t("common.modal.create", { moduleName: $t("pages.role.moduleName") })
    : $t("common.modal.update", { moduleName: $t("pages.role.moduleName") })
);

// 权限树数据
const permissionTreeData = ref<any[]>([]);

// 加载权限树
async function loadPermissionTree() {
  try {
    const groupData = await fetchListPermissionGroups(
      new PaginationQuery({ formValues: { status: "ON" } })
    );
    const groups = groupData.items ?? [];

    const permissionData = await fetchListPermissions(
      new PaginationQuery({ formValues: { status: "ON" } })
    );

    permissionTreeData.value = buildPermissionTree(groups, permissionData.items || []);
  } catch (error) {
    console.error("Failed to load permission tree:", error);
  }
}

// 过滤权限节点
function filterPermissionNode(value: string, data: any) {
  if (!value) return true;
  return data.title?.toLowerCase().includes(value.toLowerCase());
}

// 加载组织单元树（SELECTED_UNITS 档位的自定义授权集配置用）
async function loadOrgUnitTree() {
  try {
    const result = await fetchListOrgUnits(new PaginationQuery({ formValues: { status: "ON" } }));
    orgUnitTreeData.value = result.items || [];
  } catch (error) {
    console.error("Failed to load org unit tree:", error);
  }
}

// 打开抽屉
async function open(data?: { create: boolean; row?: any }) {
  visible.value = true;
  isCreate.value = data?.create ?? true;
  currentId.value = data?.row?.id;

  // 重置表单
  resetForm();

  // 加载树数据（显示加载状态）
  pageLoading.value = true;
  try {
    await loadPermissionTree();
    await loadOrgUnitTree();

    // 如果是编辑模式，填充数据
    if (!isCreate.value && data?.row) {
      // 仅回填表单声明的字段，避免把 id/createdAt/tenantName/tenantId 等
      // 不可变字段灌入 formData，进而被 ...formData 带入提交载荷。
      // （permissions / orgUnits 由树勾选控制，由 setCheckedKeys 回填，不在此赋值）
      formData.name = data.row.name ?? "";
      formData.code = data.row.code ?? "";
      formData.sortOrder = data.row.sortOrder ?? 1;
      formData.status = data.row.status ?? "ON";
      formData.dataScope = data.row.dataScope ?? "ALL";
      formData.description = data.row.description ?? "";

      // 设置树的选中状态
      await nextTick();
      if (data.row.permissions && Array.isArray(data.row.permissions)) {
        permissionTreeRef.value?.setCheckedKeys(data.row.permissions);
      }
      if (formData.dataScope === "SELECTED_UNITS" && Array.isArray(data.row.orgUnits)) {
        unitTreeRef.value?.setCheckedKeys(data.row.orgUnits);
      }

      // 回填字段权限（User 资源的隐藏字段集）
      const fpEntry = Array.isArray(data.row.fieldPermissions)
        ? data.row.fieldPermissions.find((e: any) => e?.resource === "User")
        : undefined;
      userHiddenFields.value = Array.isArray(fpEntry?.hiddenFields)
        ? fpEntry.hiddenFields.filter((v: any) => typeof v === "string")
        : [];
    }
  } finally {
    pageLoading.value = false;
  }
}

// 关闭抽屉
function handleClose() {
  visible.value = false;
  resetForm();
}

// 重置表单
function resetForm() {
  // 先删除所有 formData 上可能被历史 Object.assign 残留的多余 key
  // （id/createdAt/tenantName/tenantId 等），再恢复声明字段的默认值，
  // 确保跨抽屉开启无脏数据残留。
  Object.keys(formData).forEach((k) => {
    delete (formData as any)[k];
  });
  formData.name = "";
  formData.code = "";
  formData.sortOrder = 1;
  formData.status = "ON";
  formData.dataScope = "ALL";
  formData.description = "";
  formData.permissions = [];

  formRef.value?.clearValidate();
  permissionTreeRef.value?.setCheckedKeys([]);
  unitTreeRef.value?.setCheckedKeys([]);
  orgUnitTreeData.value = [];
  userHiddenFields.value = [];
}

// 提交表单
async function handleSubmit() {
  if (!formRef.value) return;

  try {
    await formRef.value.validate();
    submitLoading.value = true;

    // 获取选中的权限ID（只保留数字类型的叶子节点）
    const checkedKeys = permissionTreeRef.value?.getCheckedKeys(false) || [];
    const permissions = checkedKeys.filter((key: any) => typeof key === "number");

    const values: Record<string, any> = {
      ...formData,
      permissions,
    };

    // 仅 SELECTED_UNITS 档提交授权单元集（含清空场景）；
    // 其余档位不携带该字段，后端维持既有集不替换。
    if (formData.dataScope === "SELECTED_UNITS") {
      const unitCheckedKeys = unitTreeRef.value?.getCheckedKeys(false) || [];
      values.orgUnits = unitCheckedKeys.filter((key: any) => typeof key === "number");
    }

    // 字段权限始终随表单提交（含清空场景）：抽屉所见即保存后的最终态。
    values.fieldPermissions =
      userHiddenFields.value.length > 0
        ? [{ resource: "User", hiddenFields: [...userHiddenFields.value] }]
        : [];

    if (isCreate.value) {
      await createRole(values);
      ElMessage.success($t("common.notification.createSuccess"));
    } else {
      await updateRole({ id: currentId.value!, values });
      ElMessage.success($t("common.notification.updateSuccess"));
    }

    emit("success");
    handleClose();
  } catch (error) {
    if (error !== false) {
      // 不是验证错误
      ElMessage.error(
        isCreate.value
          ? $t("common.notification.createFailed")
          : $t("common.notification.updateFailed")
      );
    }
  } finally {
    submitLoading.value = false;
  }
}

// 暴露方法给父组件
defineExpose({
  open,
});
</script>

<style lang="scss" scoped>
.drawer-form {
  padding-right: 10px;
}

.drawer-footer {
  display: flex;
  justify-content: flex-end;
  gap: 10px;
}

.custom-tree-node {
  display: flex;
  align-items: center;
  flex: 1;
}

.org-unit-empty {
  width: 100%;
  box-sizing: border-box;
  padding: 16px 0;
  text-align: center;
  font-size: 14px;
  border: 1px dashed var(--el-border-color);
  border-radius: var(--el-border-radius-base);
  color: var(--el-text-color-secondary);
}

.field-perm-box {
  width: 100%;
  padding: 10px 12px;
  border: 1px solid var(--el-border-color);
  border-radius: var(--el-border-radius-base);
}

.field-perm-hint {
  margin-bottom: 8px;
  font-size: 12px;
  line-height: 1.5;
  color: var(--el-text-color-secondary);
}
</style>
