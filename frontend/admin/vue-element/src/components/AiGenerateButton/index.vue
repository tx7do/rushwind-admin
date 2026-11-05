<template>
  <ElPopover
    v-model:visible="visible"
    trigger="click"
    placement="bottom-end"
    :width="300"
    :teleported="true"
  >
    <template #reference>
      <ElButton
        size="small"
        class="ml-2"
        :disabled="disabled"
        @click.stop
      >
        <Icon icon="lucide:zap" class="mr-1 text-[13px]" />
        {{ label || t("pages.ai_content.button") }}
      </ElButton>
    </template>

    <div class="flex flex-col gap-2">
      <ElInput
        v-model="topic"
        size="small"
        clearable
        :disabled="loading"
        :placeholder="t('pages.ai_content.topicPlaceholder')"
        @keydown.enter.prevent="handleGenerate"
      />
      <ElButton
        type="primary"
        size="small"
        :loading="loading"
        :disabled="!topic.trim()"
        @click="handleGenerate"
      >
        <Icon icon="lucide:zap" class="mr-1 text-[13px]" />
        {{ t("pages.ai_content.generateNow") }}
      </ElButton>
    </div>
  </ElPopover>
</template>

<script setup lang="ts">
import { ref } from "vue";
import { useI18n } from "vue-i18n";
import { ElButton, ElInput, ElPopover, ElMessage } from "element-plus";
import { Icon } from "@iconify/vue";

import { generateAiContent } from "@/api/composables/ai-content";

defineOptions({ name: "AiGenerateButton" });

const props = defineProps<{
  /** 场景：DESCRIPTION / ANNOUNCEMENT / REPLY / GENERAL */
  scene: "DESCRIPTION" | "ANNOUNCEMENT" | "REPLY" | "GENERAL";
  /** 补充上下文（可选） */
  context?: string;
  /** 按钮文字，默认"AI 生成" */
  label?: string;
  /** 是否禁用 */
  disabled?: boolean;
}>();

const emit = defineEmits<{
  /** 生成结果回调（父组件将文本填入目标字段） */
  generate: [content: string];
}>();

const { t, locale } = useI18n();
const topic = ref("");
const visible = ref(false);
const loading = ref(false);

async function handleGenerate() {
  const trimmed = topic.value.trim();
  if (!trimmed || loading.value) return;
  loading.value = true;
  try {
    const resp = await generateAiContent({
      scene: props.scene,
      topic: trimmed,
      context: props.context,
      lang: locale.value,
    });
    emit("generate", resp.content ?? "");
    visible.value = false;
    topic.value = "";
  } catch (error) {
    // 不吞错：带出原始错误对象，供链路排查
    console.error("ai content generate failed:", error);
    ElMessage.error(t("pages.ai_content.failed"));
  } finally {
    loading.value = false;
  }
}
</script>
