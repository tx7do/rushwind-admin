<template>
  <div class="layout-wrapper">
    <component :is="currentLayoutComponent" />
    <PreferencesPanel :open="settingsVisible" @close="settingsVisible = false" />
  </div>
</template>

<script setup lang="ts">
import { useRoute } from "vue-router";

import { PreferencesPanel } from "@/core/preferences/components";
import { LayoutType } from "@/core/preferences";

import { useLayout } from "./useLayout";

import LeftLayout from "./LeftLayout.vue";
import TopLayout from "./TopLayout.vue";
import MixLayout from "./MixLayout.vue";

const route = useRoute();
const { currentLayout } = useLayout();

// 设置面板可见性（全局状态）
const settingsVisible = ref(false);

// 内容区刷新状态（true 表示正在刷新，用于清理 wrapperMap）
const contentRefreshing = ref(false);

// 刷新 key：每次刷新递增，改变组件 key 强制重建
const contentRefreshKey = ref(0);

// 提供给子组件使用
provide("settingsVisible", settingsVisible);
provide("contentRefreshing", contentRefreshing);
provide("contentRefreshKey", contentRefreshKey);

const currentLayoutComponent = computed(() => {
  const override = route.meta?.layout as LayoutType | undefined;
  const layout = override ?? currentLayout.value;

  switch (layout) {
    case "header-nav":
      return TopLayout;
    case "mixed-nav":
      return MixLayout;
    default:
      return LeftLayout;
  }
});
</script>

<style lang="scss" scoped>
.layout-wrapper {
  width: 100%;
  height: 100%;
}
</style>
