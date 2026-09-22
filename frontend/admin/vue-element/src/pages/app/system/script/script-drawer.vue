<template>
  <ProModal
    v-model:visible="visible"
    :title="title"
    :config="{ component: 'drawer', drawer: { size: DRAWER_WIDTH, closeOnClickModal: false } }"
  >
    <ElForm
      ref="formRef"
      :model="formData"
      :rules="formRules"
      label-width="120px"
      class="drawer-form"
    >
      <ElFormItem :label="$t('pages.script.name')" prop="name">
        <ElInput
          v-model="formData.name"
          :placeholder="$t('pages.script.namePlaceholder')"
          clearable
        />
      </ElFormItem>

      <ElFormItem :label="$t('pages.script.language')" prop="language">
        <ElRadioGroup v-model="formData.language" @change="handleLanguageChanged">
          <ElRadioButton
            v-for="item in scriptLanguageList"
            :key="item.value"
            :value="item.value"
          >
            {{ item.label }}
          </ElRadioButton>
        </ElRadioGroup>
        <div class="form-extra">
          {{
            formData.language === "LUA"
              ? $t("pages.script.languageLuaTip")
              : $t("pages.script.languageJavascriptTip")
          }}
        </div>
      </ElFormItem>

      <ElFormItem :label="$t('pages.script.hookPoint')" prop="hookPoint">
        <ElAutocomplete
          v-model="formData.hookPoint"
          :fetch-suggestions="queryHookPoints"
          :placeholder="$t('pages.script.hookPointPlaceholder')"
          clearable
          style="width: 100%"
        />
      </ElFormItem>

      <ElFormItem :label="$t('pages.script.priority')" prop="priority">
        <ElInputNumber v-model="formData.priority" style="width: 100%" />
      </ElFormItem>

      <ElFormItem :label="$t('pages.script.description')" prop="description">
        <ElInput
          v-model="formData.description"
          type="textarea"
          :rows="2"
          :placeholder="$t('pages.script.descriptionPlaceholder')"
        />
      </ElFormItem>

      <ElFormItem :label="$t('pages.script.critical')" prop="critical">
        <ElSwitch v-model="formData.critical" />
        <div class="form-extra">{{ $t("pages.script.criticalTip") }}</div>
      </ElFormItem>

      <ElFormItem :label="$t('pages.script.source')" prop="source">
        <div class="source-editor-wrapper">
          <Editor
            :key="formData.language"
            v-model="source"
            :editor-type="EditorType.CODE"
            :height="360"
            :code-options="{
              language: formData.language === 'LUA' ? 'lua' : 'javascript',
              tabSize: 4,
              autoDetectLanguage: false,
            }"
          />
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
import { computed, reactive, ref, watch } from "vue";
import { ElButton, ElForm, ElFormItem, ElInput, ElInputNumber, ElMessage, ElMessageBox, ElAutocomplete, ElRadioButton, ElRadioGroup, ElSwitch, type FormInstance, type FormRules } from "element-plus";

import {
  fetchHookPoints,
  scriptLanguageList,
  useCreateScript,
  useUpdateScript,
} from "@/api/composables";
import { $t } from "@/core/i18n";
import { DRAWER_WIDTH } from "@/constants";
import ProModal from "@/components/Pro/ProModal/index.vue";
import { Editor, EditorType } from "@/components/Editor";

const emit = defineEmits<{
  success: [];
}>();

const { mutateAsync: createScript } = useCreateScript();
const { mutateAsync: updateScript } = useUpdateScript();

const visible = ref(false);
const submitLoading = ref(false);
const isCreate = ref(true);
const currentId = ref<number>();
const formRef = ref<FormInstance>();
const source = ref("");

// 内置骨架与 pristine 判定：源码为空或仍等于骨架时，切语言才自动换骨架
const DEFAULT_LUA_SOURCE = `local log = require "kratos_logger"

function execute()
    local ctx = __get_ctx()
    -- ctx.set("key", value)
    return true
end
`;

const DEFAULT_JS_SOURCE = `function execute() {
    var ctx = __get_ctx();
    // ctx.set("key", value);
    return true;
}
`;

const skeletonFor = (lang: string) => (lang === "LUA" ? DEFAULT_LUA_SOURCE : DEFAULT_JS_SOURCE);

const isPristineSource = (src: string) =>
  !src.trim() || src === DEFAULT_LUA_SOURCE || src === DEFAULT_JS_SOURCE;

// 表单数据
const formData = reactive({
  name: "",
  language: "LUA",
  hookPoint: "",
  priority: 0,
  description: "",
  critical: false,
  isEnabled: true,
});

// 表单验证规则
const formRules: FormRules = {
  name: [
    { required: true, message: $t("pages.script.requiredName"), trigger: "blur" },
    {
      pattern: /^[A-Za-z][A-Za-z0-9_.-]*$/,
      message: $t("pages.script.namePlaceholder"),
      trigger: "blur",
    },
  ],
  source: [{ required: true, message: $t("pages.script.requiredSource"), trigger: "change" }],
};

// 标题
const title = computed(() =>
  isCreate.value
    ? $t("pages.script.create")
    : $t("pages.script.edit")
);

// 钩子点自动补全建议
async function queryHookPoints(
  queryString: string,
  callback: (results: Array<{ value: string }>) => void
) {
  try {
    const res = await fetchHookPoints();
    const items = (res.items ?? [])
      .filter((hp) => !queryString || (hp.name ?? "").includes(queryString))
      .map((hp) => ({
        value: hp.name ?? "",
        label: hp.description ? `${hp.name}（${hp.description}）` : hp.name,
      }));
    callback(items);
  } catch (error) {
    console.error("加载钩子点失败", error);
    callback([]);
  }
}

// 切语言：pristine 换骨架；已有真实代码则弹确认，取消时回退选择
function handleLanguageChanged(next: string | number | boolean | undefined) {
  const nextLang = String(next);
  if (isPristineSource(source.value)) {
    source.value = skeletonFor(nextLang);
    return;
  }

  ElMessageBox.confirm($t("pages.script.switchLanguageConfirm"), $t("pages.script.switchLanguageTitle"), {
    confirmButtonText: $t("common.button.confirm"),
    cancelButtonText: $t("common.button.cancel"),
    type: "warning",
  })
    .then(() => {
      // 保留源码，仅切换语言与高亮
    })
    .catch(() => {
      formData.language = nextLang === "LUA" ? "JAVASCRIPT" : "LUA";
    });
}

// 打开抽屉
function open(row?: any) {
  visible.value = true;

  if (row) {
    isCreate.value = false;
    currentId.value = row.id;
    formData.name = row.name ?? "";
    formData.language = row.language ?? "LUA";
    formData.hookPoint = row.hookPoint ?? "";
    formData.priority = row.priority ?? 0;
    formData.description = row.description ?? "";
    formData.critical = row.critical ?? false;
    formData.isEnabled = row.isEnabled ?? true;
    source.value = row.source ?? "";
  } else {
    isCreate.value = true;
    currentId.value = undefined;
    resetForm();
    source.value = skeletonFor("LUA");
  }
}

// 关闭抽屉
function handleClose() {
  visible.value = false;
  resetForm();
}

// 重置表单
function resetForm() {
  formData.name = "";
  formData.language = "LUA";
  formData.hookPoint = "";
  formData.priority = 0;
  formData.description = "";
  formData.critical = false;
  formData.isEnabled = true;
  source.value = "";
  formRef.value?.clearValidate();
}

// 提交表单
async function handleSubmit() {
  if (!formRef.value) return;

  try {
    await formRef.value.validate();
  } catch {
    return; // 校验失败，不提示"创建失败"
  }

  if (!source.value.trim()) {
    ElMessage.error($t("pages.script.requiredSource"));
    return;
  }

  submitLoading.value = true;
  try {
    const values = { ...formData, source: source.value };

    if (isCreate.value) {
      await createScript(values);
      ElMessage.success($t("common.notification.createSuccess"));
    } else {
      await updateScript({ id: currentId.value!, values });
      ElMessage.success($t("common.notification.updateSuccess"));
    }

    emit("success");
    handleClose();
  } catch (error) {
    console.error("保存脚本失败", error);
    ElMessage.error(
      isCreate.value
        ? $t("common.notification.createFailed")
        : $t("common.notification.updateFailed")
    );
  } finally {
    submitLoading.value = false;
  }
}

// ProModal 关闭时自动重置表单
watch(visible, (val) => {
  if (!val) resetForm();
});

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

.form-extra {
  width: 100%;
  margin-top: 4px;
  font-size: 12px;
  line-height: 1.4;
  color: var(--el-text-color-placeholder);
}

.source-editor-wrapper {
  width: 100%;
  min-height: 360px;
}
</style>
