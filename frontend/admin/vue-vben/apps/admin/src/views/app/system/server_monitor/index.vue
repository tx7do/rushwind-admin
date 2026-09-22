<script lang="ts" setup>
import { computed } from 'vue';

import { Page } from '@vben/common-ui';
import { $t } from '@vben/locales';

import { useServerMonitorInfo } from '#/api';
import type {
  server_monitorservicev1_ServerMonitorInfo as ServerMonitorInfo,
} from '#/api/generated/admin/service/v1';
import { formatDateTime } from '@vben/utils';

const { data } = useServerMonitorInfo();

const info = computed(() => (data.value ?? undefined) as ServerMonitorInfo | undefined);
const go = computed(() => info.value?.go);
const db = computed(() => info.value?.database);
const host = computed(() => info.value?.host);

function fmtBytes(bytes?: number | string): string {
  const n = Number(bytes ?? 0);
  if (!Number.isFinite(n)) return '-';
  if (n >= 1024 ** 3) return `${(n / 1024 ** 3).toFixed(2)} GB`;
  if (n >= 1024 ** 2) return `${(n / 1024 ** 2).toFixed(2)} MB`;
  if (n >= 1024) return `${(n / 1024).toFixed(1)} KB`;
  return String(n);
}

function fmtUptime(seconds?: number | string): string {
  const s = Number(seconds ?? 0);
  if (!Number.isFinite(s)) return '-';
  const d = Math.floor(s / 86400);
  const h = Math.floor((s % 86400) / 3600);
  const m = Math.floor((s % 3600) / 60);
  if (d > 0) return `${d}d ${h}h ${m}m`;
  if (h > 0) return `${h}h ${m}m`;
  return `${m}m ${Math.floor(s % 60)}s`;
}

function fmtPercent(a?: number | string, b?: number | string): string {
  const av = Number(a ?? 0);
  const bv = Number(b ?? 0);
  if (!bv) return '0%';
  return `${((av / bv) * 100).toFixed(1)}%`;
}
</script>

<template>
  <Page auto-content-height :title="$t('page.serverMonitor.moduleName')">
    <div class="grid grid-cols-1 gap-4 lg:grid-cols-3">
      <a-card :title="$t('page.serverMonitor.database')" size="small">
        <template #extra>
          <a-tag :color="db?.pingOk ? 'success' : 'error'">
            {{ db?.pingOk ? $t('page.serverMonitor.pingOk') : $t('page.serverMonitor.pingFail') }}
          </a-tag>
        </template>
        <a-descriptions :column="1" size="small" bordered>
          <a-descriptions-item :label="$t('page.serverMonitor.dbDriver')">
            {{ db?.driver || '-' }}
          </a-descriptions-item>
          <a-descriptions-item :label="$t('page.serverMonitor.openConnections')">
            {{ db?.openConnections ?? '-' }} / {{ db?.maxOpenConnections ?? '-' }}
            ({{ fmtPercent(db?.openConnections, db?.maxOpenConnections) }})
          </a-descriptions-item>
          <a-descriptions-item :label="$t('page.serverMonitor.inUseConnections')">
            {{ db?.inUseConnections ?? '-' }}
          </a-descriptions-item>
          <a-descriptions-item :label="$t('page.serverMonitor.idleConnections')">
            {{ db?.idleConnections ?? '-' }}
          </a-descriptions-item>
        </a-descriptions>
      </a-card>

      <a-card :title="$t('page.serverMonitor.goRuntime')" size="small">
        <a-descriptions :column="1" size="small" bordered>
          <a-descriptions-item :label="$t('page.serverMonitor.goVersion')">
            {{ go?.version || '-' }}
          </a-descriptions-item>
          <a-descriptions-item :label="$t('page.serverMonitor.numGoroutine')">
            {{ go?.numGoroutine ?? '-' }}
          </a-descriptions-item>
          <a-descriptions-item :label="$t('page.serverMonitor.memAlloc')">
            {{ fmtBytes(go?.memAllocBytes) }}
          </a-descriptions-item>
          <a-descriptions-item :label="$t('page.serverMonitor.memSys')">
            {{ fmtBytes(go?.memSysBytes) }}
          </a-descriptions-item>
          <a-descriptions-item :label="$t('page.serverMonitor.gcCycles')">
            {{ go?.gcCycles ?? '-' }}
          </a-descriptions-item>
          <a-descriptions-item :label="$t('page.serverMonitor.uptime')">
            {{ fmtUptime(go?.uptimeSeconds) }}
          </a-descriptions-item>
        </a-descriptions>
      </a-card>

      <a-card :title="$t('page.serverMonitor.host')" size="small">
        <a-descriptions :column="1" size="small" bordered>
          <a-descriptions-item :label="$t('page.serverMonitor.hostname')">
            {{ host?.hostname || '-' }}
          </a-descriptions-item>
          <a-descriptions-item :label="$t('page.serverMonitor.os')">
            {{ host ? `${host.os || '-'} / ${host.arch || '-'}` : '-' }}
          </a-descriptions-item>
          <a-descriptions-item :label="$t('page.serverMonitor.numCpu')">
            {{ host?.numCpu ?? '-' }}
          </a-descriptions-item>
        </a-descriptions>
      </a-card>
    </div>

    <div class="mt-4 text-xs opacity-60">
      {{ $t('page.serverMonitor.disclaimer') }}
      <span v-if="info?.collectedAt">({{ formatDateTime(info.collectedAt) }})</span>
    </div>
  </Page>
</template>
