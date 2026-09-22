<template>
  <ProModal
    v-model:visible="drawer.visible.value"
    :title="drawer.title.value"
    :loading="drawer.pageLoading.value"
    :config="{
      component: 'drawer',
      drawer: { size: drawer.drawerWidth, closeOnClickModal: false },
    }"
  >
    <ElForm
      ref="formRef"
      :model="drawer.formData"
      :rules="formRules"
      label-width="120px"
      class="drawer-form"
    >
      <ElFormItem :label="t('name')" prop="name">
        <ElInput
          v-model="drawer.formData.name"
          :placeholder="t('namePlaceholder')"
          clearable
        />
      </ElFormItem>

      <ElFormItem v-if="!isCreate" :label="t('status')" prop="status">
        <ElRadioGroup v-model="drawer.formData.status">
          <ElRadio value="ON">{{ t("pages.access_key.statusMap.ON") }}</ElRadio>
          <ElRadio value="OFF">{{ t("pages.access_key.statusMap.OFF") }}</ElRadio>
        </ElRadioGroup>
      </ElFormItem>

      <ElFormItem :label="t('expiresAt')" prop="expiresAt">
        <ElDatePicker
          v-model="drawer.formData.expiresAt"
          type="datetime"
          :placeholder="t('common.placeholder.select')"
          style="width: 100%"
        />
        <div class="form-hint">{{ t("pages.access_key.expiresAtHint") }}</div>
      </ElFormItem>
    </ElForm>

    <template #footer>
      <div class="drawer-footer">
        <ElButton @click="drawer.close">{{ $t("common.button.cancel") }}</ElButton>
        <ElButton
          type="primary"
          :loading="drawer.submitLoading.value"
          @click="drawer.handleSubmit(formRef, () => emit('success'))"
        >
          {{ $t("common.button.confirm") }}
        </ElButton>
      </div>
    </template>
  </ProModal>

  <!-- 一次性 Secret 展示：独立于抽屉，抽屉关闭后仍需可见 -->
  <ElDialog
    v-model="secretVisible"
    :title="t('secretDialogTitle')"
    width="560px"
    align-center
    :close-on-click-modal="false"
    @closed="handleSecretClosed"
  >
    <ElAlert type="warning" :closable="false" :title="t('secretDialogHint')" />
    <ElInput :model-value="createdSecret" readonly class="secret-input">
      <template #append>
        <ElButton @click="copySecret">{{ t("pages.access_key.secretCopied") }}</ElButton>
      </template>
    </ElInput>
    <template #footer>
      <ElButton type="primary" @click="secretVisible = false">
        {{ $t("common.button.confirm") }}
      </ElButton>
    </template>
  </ElDialog>
</template>

<script lang="ts" setup>
import { ref } from "vue";
import { ElMessage } from "element-plus";
import ProModal from "@/components/Pro/ProModal/index.vue";
import { useDrawerForm } from "@/components/Pro/composables/useDrawerForm";
import { useI18n } from "@/core/i18n";
import {
  createAccessKey,
  updateAccessKey,
} from "@/api/composables/access_key";

const emit = defineEmits<{
  success: [];
}>();

const { t } = useI18n();

const formRef = ref();
const createdSecret = ref("");
const secretVisible = ref(false);
const isCreate = ref(true);

const drawer = useDrawerForm({
  moduleKey: "page.accessKey.moduleName",
  defaults: {
    name: "",
    status: "ON",
    expiresAt: undefined as string | undefined,
  },
  createFn: async (values: Record<string, any>) => {
    const resp = await createAccessKey(values);
    createdSecret.value = resp.secret ?? "";
    secretVisible.value = true;
    return resp;
  },
  updateFn: (id: number, values: Record<string, any>) => updateAccessKey(id, values),
});

const formRules = {
  name: [{ required: true, message: t("pages.access_key.requiredName"), trigger: "blur" }],
};

// 包装 open 以追踪创建/编辑模式（切换状态字段的显隐）
function open(options: { create: boolean; row?: any }) {
  isCreate.value = options.create;
  drawer.open(options);
}

defineExpose({ open });

function copySecret() {
  navigator.clipboard?.writeText(createdSecret.value).catch((err: unknown) => {
    console.error("copy secret failed:", err);
  });
  ElMessage.success(t("pages.access_key.secretCopied"));
}

function handleSecretClosed() {
  createdSecret.value = "";
  emit("success");
}
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

.form-hint {
  width: 100%;
  font-size: 12px;
  color: var(--el-text-color-secondary);
  line-height: 1.4;
}

.secret-input {
  margin-top: 12px;
}
</style>
