<template>
  <ElDrawer
    :model-value="open"
    direction="rtl"
    :size="DRAWER_WIDTH"
    :show-close="false"
    class="preferences-drawer"
    @close="onClose"
  >
    <template #header>
      <div class="drawer-header">
        <div>
          <h2 class="drawer-title">{{ t("preferences.title") }}</h2>
          <p class="drawer-subtitle">{{ t("preferences.subtitle") }}</p>
        </div>
        <div class="drawer-actions">
          <ElButton
            :icon="RefreshRight"
            link
            :title="t('preferences.actions.reset')"
            @click="handleReset"
          />
          <ElButton :icon="Close" link :title="t('preferences.actions.close')" @click="onClose" />
        </div>
      </div>
    </template>

    <template #default>
      <!-- Tab 切换 -->
      <div class="drawer-tabs">
        <div class="tab-group">
          <div
            v-for="tab in TAB_OPTIONS"
            :key="tab.value"
            class="tab-item"
            :class="{ active: activeTab === tab.value }"
            @click="activeTab = tab.value"
          >
            {{ t(`preferences.tabs.${tab.label}`) }}
          </div>
        </div>
      </div>

      <!-- 内容区域 -->
      <div class="drawer-content">
        <component :is="TAB_COMPONENTS[activeTab]" />
      </div>
    </template>

    <template #footer>
      <div class="drawer-footer">
        <ElButton type="primary" :icon="CopyDocument" @click="handleCopy">
          {{ t("preferences.actions.copySettings") }}
        </ElButton>
        <ElButton link type="danger" :icon="SwitchButton" @click="handleLogout">
          {{ t("preferences.actions.clearCacheAndLogout") }}
        </ElButton>
      </div>
    </template>
  </ElDrawer>
</template>

<script setup lang="ts">
import { ref, type Component } from "vue";
import { ElDrawer, ElButton } from "element-plus";
import { RefreshRight, Close, CopyDocument, SwitchButton } from "@element-plus/icons-vue";
import { useI18n } from "@/core/i18n";
import { resetPreferences } from "../../index";
import { useAuth } from "@/composables/use-auth";
import { DRAWER_WIDTH } from "@/constants";

import AppearancePanel from "./AppearancePanel.vue";
import LayoutPanel from "./LayoutPanel.vue";
import ShortcutKeyPanel from "./ShortcutKeyPanel.vue";
import GeneralPanel from "./GeneralPanel.vue";

const { t } = useI18n();
const { logout } = useAuth();

defineProps<{
  open: boolean;
}>();

const emit = defineEmits<{
  close: [];
}>();

type TabType = "appearance" | "layout" | "shortcut" | "general";

const TAB_OPTIONS = [
  { label: "appearance", value: "appearance" as TabType },
  { label: "layout", value: "layout" as TabType },
  { label: "shortcut", value: "shortcut" as TabType },
  { label: "general", value: "general" as TabType },
];

const TAB_COMPONENTS: Record<TabType, Component> = {
  appearance: AppearancePanel,
  layout: LayoutPanel,
  shortcut: ShortcutKeyPanel,
  general: GeneralPanel,
};

const activeTab = ref<TabType>("appearance");

function onClose() {
  emit("close");
}

function handleReset() {
  resetPreferences();
}

function handleCopy() {
  // TODO: 实现复制偏好设置功能
  console.log("复制偏好设置");
}

async function handleLogout() {
  try {
    await logout(true);
  } catch (error) {
    console.error("Logout failed:", error);
  }
}
</script>

<style scoped lang="scss">
.preferences-drawer {
  :deep(.el-drawer__header) {
    padding: 16px 24px;
    margin-bottom: 0;
    border-bottom: 1px solid var(--el-border-color-lighter);
  }

  :deep(.el-drawer__footer) {
    border-top: 1px solid var(--el-border-color-lighter);
    padding: 16px 24px;
  }
}

.drawer-header {
  display: flex;
  justify-content: space-between;
  align-items: flex-start;
  width: 100%;
}

.drawer-title {
  margin: 0;
  font-size: 16px;
  font-weight: 600;
  line-height: 1.4;
  color: var(--el-text-color-primary);
}

.drawer-subtitle {
  margin: 4px 0 0;
  font-size: 12px;
  color: var(--el-text-color-secondary);
}

.drawer-actions {
  display: flex;
  gap: 4px;
}

.drawer-tabs {
  margin-bottom: 24px;
}

.tab-group {
  display: flex;
  gap: 4px;
  background: var(--el-fill-color-light);
  padding: 2px;
  border-radius: 6px;
}

.tab-item {
  flex: 1;
  height: 32px;
  border-radius: 4px;
  display: flex;
  align-items: center;
  justify-content: center;
  font-size: 12px;
  font-weight: 500;
  cursor: pointer;
  transition: all 0.2s;
  color: var(--el-text-color-regular);

  &:hover {
    color: var(--el-color-primary);
  }

  &.active {
    background: var(--el-bg-color);
    box-shadow: 0 2px 4px rgba(0, 0, 0, 0.1);
    color: var(--el-color-primary);
  }
}

.drawer-content {
  padding-bottom: 70px;
}

.drawer-footer {
  display: flex;
  justify-content: space-between;
  align-items: center;
  gap: 12px;
}
</style>
