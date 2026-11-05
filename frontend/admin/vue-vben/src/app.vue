<script lang="ts" setup>
import { computed } from 'vue';

import { useAntdDesignTokens } from '@vben/hooks';
import { preferences, usePreferences } from '@vben/preferences';

import { App, ConfigProvider, theme } from 'ant-design-vue';

import { antdLocale } from '#/locales';
import { TanstackQueryDevtools } from '#/plugins/vue-query';

defineOptions({ name: 'App' });

const isDev = import.meta.env.DEV;

const { isDark } = usePreferences();
const { tokens } = useAntdDesignTokens();

const tokenTheme = computed(() => {
  const algorithm = isDark.value
    ? [theme.darkAlgorithm]
    : [theme.defaultAlgorithm];

  // antd 紧凑模式算法
  if (preferences.app.compact) {
    algorithm.push(theme.compactAlgorithm);
  }

  return {
    algorithm,
    token: tokens,
    components: {
      Drawer: {
        // 抽屉按 §2.3 属 L1 表面（#111827），不是浮层（#1C2128）；
        // 全局 colorBgElevated 取的是 --popover，单独把 Drawer 拨回 --card
        colorBgElevated: tokens.colorBgContainer,
      },
    },
  };
});
</script>

<template>
  <ConfigProvider :locale="antdLocale" :theme="tokenTheme">
    <App>
      <RouterView />
    </App>
    <!-- Vue Query Devtools（仅开发环境） -->
    <TanstackQueryDevtools v-if="isDev" />
  </ConfigProvider>
</template>
