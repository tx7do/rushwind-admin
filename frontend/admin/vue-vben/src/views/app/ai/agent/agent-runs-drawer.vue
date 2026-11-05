<script lang="ts" setup>
import { ref } from 'vue';

import { useVbenDrawer } from '@vben/common-ui';
import { $t } from '@vben/locales';


import { fetchListAiAgentRuns, PaginationQuery } from '#/api';

// antd 组件（vben 手工注册模式）：Tag/Timeline 用 a-tag / a-timeline
function parseTrace(raw?: string): Array<Record<string, any>> {
  if (!raw) return [];
  try {
    const parsed = JSON.parse(raw);
    return Array.isArray(parsed) ? parsed : [];
  } catch {
    return [];
  }
}

const app = ref<any>({});
const runs = ref<any[]>([]);
const loading = ref(false);

const [Drawer, drawerApi] = useVbenDrawer({
  class: 'w-[640px]',
  footer: false,
  onOpenChange(isOpen) {
    if (!isOpen) return;
    const data = drawerApi.getData<{ row: any }>() ?? { row: {} };
    app.value = data.row ?? {};
    loading.value = true;
    fetchListAiAgentRuns(
      new PaginationQuery({
        paging: { page: 1, pageSize: 20 },
        formValues: { appId: String(app.value.id) },
      }),
    )
      .then((resp: any) => {
        runs.value = resp.items ?? [];
      })
      .catch((error: any) => {
        console.error('[ai-agent] list runs failed', error);
      })
      .finally(() => {
        loading.value = false;
      });
  },
});
</script>

<template>
  <Drawer :title="`${$t('page.aiAgent.runsTitle')} · ${app?.name || ''}`">
    <div v-if="runs.length === 0 && !loading" class="text-xs text-muted-foreground pb-2">
      {{ $t('page.aiAgent.runs.empty') }}
    </div>
    <div v-if="loading" class="text-xs text-muted-foreground pb-2">
      {{ $t('ui.loading') }}
    </div>

    <div
      v-for="run in runs"
      :key="run.id"
      class="mb-3 rounded-lg border border-border-light p-3"
    >
      <div class="flex flex-wrap items-center gap-2">
        <a-tag v-if="run.status === 'SUCCEEDED'" color="success">
          {{ $t('page.aiAgent.runs.succeeded') }}
        </a-tag>
        <a-tag v-else-if="run.status === 'FAILED'" color="error">
          {{ $t('page.aiAgent.runs.failed') }}
        </a-tag>
        <a-tag v-else color="processing">{{ $t('page.aiAgent.runs.running') }}</a-tag>
        <span class="text-xs text-muted-foreground">#{{ run.id }}</span>
        <a-tag v-if="(run.totalTokens ?? 0) > 0">tokens: {{ run.totalTokens }}</a-tag>
        <a-tag v-if="run.durationMs != null">{{ run.durationMs }}ms</a-tag>
      </div>
      <div
        v-if="run.output"
        class="mt-2 max-h-[72px] overflow-hidden whitespace-pre-wrap text-xs"
      >
        {{ run.output }}
      </div>
      <div
        v-if="run.error"
        class="mt-2 truncate text-xs text-destructive"
        :title="run.error"
      >
        {{ run.error }}
      </div>
      <a-timeline v-if="parseTrace(run.trace).length > 0" class="mt-3">
        <a-timeline-item
          v-for="(s, i) in parseTrace(run.trace)"
          :key="i"
          :color="s.status === 'ok' ? 'green' : 'red'"
        >
          <div class="flex items-center gap-1.5 text-xs">
            <code>{{ s.nodeId }}</code>
            <a-tag>{{ s.type }}</a-tag>
            <span>{{ s.ms }}ms</span>
          </div>
        </a-timeline-item>
      </a-timeline>
    </div>
  </Drawer>
</template>
