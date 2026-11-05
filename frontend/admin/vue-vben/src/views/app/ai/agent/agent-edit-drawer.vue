<script lang="ts" setup>
import { computed, ref } from 'vue';

import { useVbenDrawer } from '@vben/common-ui';
import { $t } from '@vben/locales';

import { message } from 'ant-design-vue';

import { createAiAgentApp, updateAiAgentApp } from '#/api';

const emit = defineEmits<{ success: [] }>();

const create = ref(true);
const row = ref<any>({});
// 标题按模式区分：创建 Agent 编排 / 更新 Agent 编排（对齐三端抽屉标题口径）
const title = computed(() =>
  create.value
    ? $t('ui.modal.create', { moduleName: $t('page.aiAgent.moduleName') })
    : $t('ui.modal.update', { moduleName: $t('page.aiAgent.moduleName') }),
);
const form = ref<Record<string, any>>({
  name: '',
  description: '',
  graph: '',
  enabled: true,
});

const [Drawer, drawerApi] = useVbenDrawer({
  class: 'w-[560px]',
  onOpenChange(isOpen) {
    if (!isOpen) return;
    const data = drawerApi.getData<{ create: boolean; row?: any }>() ?? { create: true, row: {} };
    create.value = data.create;
    row.value = data.row ?? {};
    form.value = {
      name: row.value.name || '',
      description: row.value.description || '',
      graph: row.value.graph || '',
      enabled: !!row.value.enabled,
    };
  },
  onConfirm: async () => {
    if (!form.value.name) {
      message.error($t('page.aiAgent.requiredName'));
      return;
    }
    if (form.value.graph) {
      try {
        JSON.parse(form.value.graph);
      } catch {
        message.error($t('page.aiAgent.graphInvalid'));
        return;
      }
    }
    drawerApi.setState({ loading: true });
    try {
      const data = create.value
        ? await createAiAgentApp({ ...form.value })
        : await updateAiAgentApp(row.value.id, { ...form.value }, 'name,description,graph,enabled');
      if (data) {
        message.success(
          create.value
            ? $t('ui.notification.create_success')
            : $t('ui.notification.update_success'),
        );
        drawerApi.close();
        emit('success');
      }
    } catch (error: any) {
      console.error('[ai-agent] save failed', error);
      message.error(error?.message || $t('page.aiAgent.saveFailed'));
    } finally {
      drawerApi.setState({ loading: false });
    }
  },
});


</script>

<template>
  <Drawer :title="title">
    <a-form layout="vertical" class="pt-2">
      <a-form-item :label="$t('page.aiAgent.name')" required>
        <a-input v-model:value="form.name" />
      </a-form-item>
      <a-form-item :label="$t('page.aiAgent.description')">
        <a-textarea v-model:value="form.description" :rows="2" />
      </a-form-item>
      <a-form-item
        :label="$t('page.aiAgent.graph')"
        :help="$t('page.aiAgent.graphHint')"
      >
        <a-textarea
          v-model:value="form.graph"
          :rows="10"
          :placeholder="$t('page.aiAgent.graphPlaceholder')"
          class="font-mono"
        />
      </a-form-item>
      <a-form-item :label="$t('page.aiAgent.enabled')">
        <a-switch v-model:checked="form.enabled" />
      </a-form-item>
    </a-form>
  </Drawer>
</template>
