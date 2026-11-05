<script lang="ts" setup>
import { computed, h, onMounted, onUnmounted, ref } from 'vue';

import { useVbenDrawer } from '@vben/common-ui';
import { LucideRefreshCw, LucideTrash2, Search } from '@vben/icons';
import { $t } from '@vben/locales';
import { dateUtil } from '@vben/utils';

import { notification } from 'ant-design-vue';

import { fetchListScriptLogs, usePurgeScriptLogs } from '#/api';

const { mutateAsync: purgeLogs } = usePurgeScriptLogs();

const logs = ref<any[]>([]);
const total = ref(0);
const page = ref(1);
const pageSize = ref(20);
const loading = ref(false);
const filterScriptName = ref('');
const filterTrigger = ref<string | undefined>(undefined);
const filterSuccess = ref<string | undefined>(undefined);

// 表体滚动高度：抽屉全高，扣掉标题/搜索行/清理行/表头/分页器的估高
const tableY = ref(400);

function measureTableY() {
  tableY.value = Math.max(window.innerHeight - 300, 200);
}

onMounted(() => {
  measureTableY();
  window.addEventListener('resize', measureTableY);
});

onUnmounted(() => {
  window.removeEventListener('resize', measureTableY);
});

const columns = [
  { title: $t('page.script.logScriptName'), dataIndex: 'scriptName', width: 150, ellipsis: true },
  { title: $t('page.script.language'), dataIndex: 'language', width: 90 },
  { title: $t('page.script.logTrigger'), dataIndex: 'triggerType', width: 100 },
  { title: $t('page.script.hookPoint'), dataIndex: 'hookPoint', width: 140, ellipsis: true },
  { title: $t('page.script.logSuccess'), dataIndex: 'success', width: 90 },
  { title: $t('page.script.testRunDuration'), dataIndex: 'durationMs', width: 90 },
  { title: $t('page.script.logError'), dataIndex: 'error', ellipsis: true },
  { title: $t('page.script.logCreatedAt'), dataIndex: 'createdAt', width: 170 },
];

const triggerOptions = [
  { label: 'hook', value: 'hook' },
  { label: 'task', value: 'task' },
  { label: 'test_run', value: 'test_run' },
];

const successOptions = computed(() => [
  { label: $t('page.script.testRunSuccess'), value: 'true' },
  { label: $t('page.script.testRunFailed'), value: 'false' },
]);

const paginationConfig = computed(() => ({
  current: page.value,
  pageSize: pageSize.value,
  total: total.value,
  showSizeChanger: true,
  onChange: (p: number, s: number) => {
    const sizeChanged = s !== pageSize.value;
    page.value = p;
    pageSize.value = s;
    if (sizeChanged) {
      page.value = 1;
    }
    loadLogs();
  },
}));

async function loadLogs() {
  loading.value = true;
  try {
    const formValues: Record<string, any> = {};
    if (filterScriptName.value) formValues.scriptName = filterScriptName.value;
    if (filterTrigger.value) formValues.triggerType = filterTrigger.value;
    if (filterSuccess.value) formValues.success = filterSuccess.value;

    const result = await fetchListScriptLogs({
      page: page.value,
      pageSize: pageSize.value,
      orderBy: JSON.stringify(['-created_at']),
      query: JSON.stringify(formValues),
    });
    logs.value = result.items ?? [];
    total.value = Number(result.total ?? 0);
  } catch (error) {
    console.error('加载脚本执行日志失败', error);
    notification.error({ message: $t('page.script.fetchFailed') });
  } finally {
    loading.value = false;
  }
}

function handleSearch() {
  page.value = 1;
  loadLogs();
}

function handleReset() {
  filterScriptName.value = '';
  filterTrigger.value = undefined;
  filterSuccess.value = undefined;
  handleSearch();
}

async function handlePurge() {
  try {
    const before = new Date(Date.now() - 90 * 24 * 3600 * 1000).toISOString();
    const resp = await purgeLogs({ before });
    notification.success({
      message: $t('page.script.logPurgeSuccess', { count: Number(resp?.deleted ?? 0) }),
    });
    await loadLogs();
  } catch (error) {
    console.error('清理脚本执行日志失败', error);
    notification.error({ message: $t('page.script.fetchFailed') });
  }
}

const [Drawer] = useVbenDrawer({
  title: $t('page.script.viewLogTitle'),
  class: 'w-[960px]',
  footer: false,
  onOpenChange(isOpen) {
    if (isOpen) {
      measureTableY();
      loadLogs();
    }
  },
});
</script>

<template>
  <Drawer>
    <div class="flex h-full flex-col">
      <!-- 搜索行 -->
      <div class="mb-3 flex flex-wrap items-center gap-2">
        <a-input
          v-model:value="filterScriptName"
          :placeholder="$t('page.script.logScriptName')"
          style="width: 160px"
          allow-clear
          @press-enter="handleSearch"
        />
        <a-select
          v-model:value="filterTrigger"
          :options="triggerOptions"
          :placeholder="$t('page.script.logTrigger')"
          style="width: 130px"
          allow-clear
        />
        <a-select
          v-model:value="filterSuccess"
          :options="successOptions"
          :placeholder="$t('page.script.logSuccess')"
          style="width: 130px"
          allow-clear
        />
        <a-button type="primary" :icon="h(Search)" @click="handleSearch">
          {{ $t('ui.button.search') }}
        </a-button>
        <a-button :icon="h(LucideRefreshCw)" type="text" @click="handleReset">
          {{ $t('ui.button.reset') }}
        </a-button>
      </div>

      <!-- 清理钮：右对齐（对齐 react 表格工具栏） -->
      <div class="mb-2 flex justify-end">
        <a-popconfirm
          :cancel-text="$t('ui.button.cancel')"
          :ok-text="$t('ui.button.ok')"
          :title="$t('page.script.logPurgeConfirmDesc')"
          @confirm="handlePurge"
        >
          <a-button danger :icon="h(LucideTrash2)">
            {{ $t('page.script.logPurge') }}
          </a-button>
        </a-popconfirm>
      </div>

      <a-table
        :columns="columns"
        :data-source="logs"
        :loading="loading"
        :pagination="paginationConfig"
        :scroll="{ y: tableY }"
        row-key="id"
        size="small"
      >
        <template #bodyCell="{ column, record }">
          <template v-if="column.dataIndex === 'language'">
            <a-tag v-if="record.language">{{ record.language }}</a-tag>
            <span v-else>-</span>
          </template>
          <template v-else-if="column.dataIndex === 'triggerType'">
            <a-tag v-if="record.triggerType">{{ record.triggerType }}</a-tag>
            <span v-else>-</span>
          </template>
          <template v-else-if="column.dataIndex === 'hookPoint'">
            {{ record.hookPoint || '-' }}
          </template>
          <template v-else-if="column.dataIndex === 'success'">
            <a-tag :color="record.success ? 'success' : 'error'">
              {{
                record.success
                  ? $t('page.script.testRunSuccess')
                  : $t('page.script.testRunFailed')
              }}
            </a-tag>
          </template>
          <template v-else-if="column.dataIndex === 'durationMs'">
            {{ record.durationMs ?? 0 }}ms
          </template>
          <template v-else-if="column.dataIndex === 'error'">
            <a-tooltip v-if="record.error" :title="record.error">
              <span class="text-destructive">{{ record.error }}</span>
            </a-tooltip>
            <span v-else>-</span>
          </template>
          <template v-else-if="column.dataIndex === 'createdAt'">
            {{ record.createdAt ? dateUtil(record.createdAt).format('YYYY-MM-DD HH:mm:ss') : '-' }}
          </template>
        </template>
      </a-table>
    </div>
  </Drawer>
</template>
