<script lang="ts" setup>
import { Page } from '@vben/common-ui';
import { $t } from '@vben/locales';

import { useMyTenantUsage } from '#/api';

/**
 * 租户套餐用量面板（自取数）：用户数/存储占用/API 调用/AI Token vs 配额上限的进度条。
 * 64-bit 计数经 protojson 是字符串，一律 Number() 转换（文档已知坑）。
 */
const { data: usage, isLoading } = useMyTenantUsage();

const num = (v: number | string | undefined): number => Number(v ?? 0);

const currentOf = (quotaType?: string): number => {
  const u = usage.value;
  if (!u) return 0;
  switch (quotaType) {
    case 'USER_LIMIT': {
      return num(u.userCount);
    }
    case 'STORAGE': {
      return Math.round(num(u.storageUsedBytes) / 1024 / 1024);
    }
    case 'API_CALL': {
      return num(u.apiCallCount);
    }
    default: {
      return 0;
    }
  }
};

const pctOf = (quotaType?: string, quotaValue?: number | string): number => {
  const limit = num(quotaValue);
  if (limit <= 0) return 0;
  return Math.min(100, Math.round((currentOf(quotaType) / limit) * 100));
};

const unitOf = (quotaType?: string): string =>
  quotaType === 'STORAGE' ? 'MB' : '';

const quotaLabel = (quotaType?: string): string => {
  switch (quotaType) {
    case 'USER_LIMIT': {
      return $t('page.user.profile.usage.USER_LIMIT');
    }
    case 'STORAGE': {
      return $t('page.user.profile.usage.STORAGE');
    }
    case 'API_CALL': {
      return $t('page.user.profile.usage.API_CALL');
    }
    case 'AI_TOKENS': {
      return $t('page.user.profile.usage.AI_TOKENS');
    }
    default: {
      return quotaType ?? '-';
    }
  }
};
</script>

<template>
  <Page>
    <div class="tenant-usage-panel">
      <div v-if="isLoading" class="panel-loading">
        {{ $t('page.user.profile.usage.loading') }}
      </div>
      <div v-else-if="!usage?.quotas || usage.quotas.length === 0" class="panel-empty">
        {{ $t('page.user.profile.usage.empty') }}
      </div>
      <template v-else>
        <p class="plan-line">
          <span class="plan-label">{{ $t('page.user.profile.usage.plan') }}</span>
          <strong>{{ usage.planName || $t('page.user.profile.usage.noPlan') }}</strong>
        </p>
        <div v-for="(q, idx) in usage.quotas" :key="idx" class="quota-row">
          <div class="quota-head">
            <span>{{ quotaLabel(q.quotaType) }}</span>
            <span class="quota-numbers">
              {{ currentOf(q.quotaType) }}{{ unitOf(q.quotaType) }} /
              {{ num(q.quotaValue) }}{{ unitOf(q.quotaType) }}
              <a-tag v-if="pctOf(q.quotaType, q.quotaValue) >= 100" color="error">
                {{ $t('page.user.profile.usage.reached') }}
              </a-tag>
            </span>
          </div>
          <a-progress
            size="small"
            :percent="pctOf(q.quotaType, q.quotaValue)"
            :status="
              pctOf(q.quotaType, q.quotaValue) >= 100
                ? 'exception'
                : pctOf(q.quotaType, q.quotaValue) >= 80
                  ? 'active'
                  : 'normal'
            "
          />
        </div>
      </template>
    </div>
  </Page>
</template>

<style lang="scss" scoped>
.tenant-usage-panel {
  max-width: 560px;
}

.panel-loading,
.panel-empty {
  padding: 32px 0;
  text-align: center;
  opacity: 0.7;
}

.plan-line {
  margin: 0 0 20px;
  font-size: 14px;

  .plan-label {
    margin-right: 8px;
    opacity: 0.7;
  }
}

.quota-row {
  margin-bottom: 20px;

  .quota-head {
    display: flex;
    justify-content: space-between;
    margin-bottom: 6px;
    font-size: 13px;

    .quota-numbers {
      opacity: 0.7;
    }
  }
}
</style>
