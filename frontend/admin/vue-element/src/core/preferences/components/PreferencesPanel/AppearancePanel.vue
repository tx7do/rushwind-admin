<template>
  <div class="appearance-panel">
    <!-- 主题模式 -->
    <section class="appearance-section">
      <h3 class="section-title">{{ t("preferences.appearance.theme") }}</h3>
      <div class="theme-mode-options">
        <div class="theme-mode-wrapper" @click="updatePreferences({ theme: { mode: 'light' } })">
          <div class="theme-mode-item" :class="{ active: preferences.theme.mode === 'light' }">
            <ElIcon :size="24"><Sunny /></ElIcon>
          </div>
          <span class="theme-mode-label">{{ t("preferences.appearance.lightMode") }}</span>
        </div>
        <div class="theme-mode-wrapper" @click="updatePreferences({ theme: { mode: 'dark' } })">
          <div class="theme-mode-item" :class="{ active: preferences.theme.mode === 'dark' }">
            <ElIcon :size="24"><Moon /></ElIcon>
          </div>
          <span class="theme-mode-label">{{ t("preferences.appearance.darkMode") }}</span>
        </div>
        <div class="theme-mode-wrapper" @click="updatePreferences({ theme: { mode: 'auto' } })">
          <div class="theme-mode-item" :class="{ active: preferences.theme.mode === 'auto' }">
            <ElIcon :size="24"><Monitor /></ElIcon>
          </div>
          <span class="theme-mode-label">{{ t("preferences.appearance.followSystem") }}</span>
        </div>
      </div>
    </section>

    <!-- 深色侧边栏和顶栏 -->
    <section class="appearance-section">
      <div class="preference-item">
        <span>{{ t("preferences.appearance.darkSidebar") }}</span>
        <ElSwitch
          :disabled="preferences.theme.mode !== 'light'"
          :model-value="preferences.theme.semiDarkSidebar"
          @change="
            (val: string | number | boolean) =>
              updatePreferences({ theme: { semiDarkSidebar: val as boolean } })
          "
        />
      </div>
      <div class="preference-item">
        <span>{{ t("preferences.appearance.darkHeader") }}</span>
        <ElSwitch
          :disabled="preferences.theme.mode !== 'light'"
          :model-value="preferences.theme.semiDarkHeader"
          @change="
            (val: string | number | boolean) =>
              updatePreferences({ theme: { semiDarkHeader: val as boolean } })
          "
        />
      </div>
    </section>

    <!-- 内置主题 -->
    <section class="appearance-section">
      <h3 class="section-title">{{ t("preferences.appearance.builtinThemes") }}</h3>
      <div class="builtin-themes-grid">
        <div
          v-for="theme in BUILTIN_THEMES"
          :key="theme.type"
          class="builtin-theme-wrapper"
          @click="handleBuiltinThemeChange(theme.type)"
        >
          <div
            class="builtin-theme-item"
            :class="{ active: preferences.theme.builtinType === theme.type }"
          >
            <div
              class="theme-color-preview"
              :style="{
                backgroundColor: theme.type === 'custom' ? 'transparent' : theme.color,
                border: theme.type === 'custom' ? '1px dashed #666' : 'none',
              }"
            >
              <ElIcon v-if="theme.type === 'custom'" :size="20"><Brush /></ElIcon>
            </div>
          </div>
          <span class="theme-name">{{ t(`preferences.${theme.nameKey}`) }}</span>
        </div>
      </div>
    </section>

    <!-- 圆角 -->
    <section class="appearance-section">
      <h3 class="section-title">{{ t("preferences.appearance.radius") }}</h3>
      <div class="radius-group">
        <div
          v-for="opt in RADIUS_OPTIONS"
          :key="opt.value"
          class="radius-item"
          :class="{ active: preferences.theme.radius === opt.value }"
          @click="updatePreferences({ theme: { radius: opt.value } })"
        >
          {{ opt.label }}
        </div>
      </div>
    </section>

    <!-- 其它 -->
    <section class="appearance-section">
      <h3 class="section-title">{{ t("preferences.appearance.other") }}</h3>
      <div class="preference-item">
        <span>{{ t("preferences.appearance.colorWeakMode") }}</span>
        <ElSwitch
          :model-value="preferences.app.colorWeakMode"
          @change="
            (val: string | number | boolean) =>
              updatePreferences({ app: { colorWeakMode: val as boolean } })
          "
        />
      </div>
      <div class="preference-item">
        <span>{{ t("preferences.appearance.colorGrayMode") }}</span>
        <ElSwitch
          :model-value="preferences.app.colorGrayMode"
          @change="
            (val: string | number | boolean) =>
              updatePreferences({ app: { colorGrayMode: val as boolean } })
          "
        />
      </div>
    </section>
  </div>
</template>

<script setup lang="ts">
import { ElSwitch, ElIcon } from "element-plus";
import { Sunny, Moon, Monitor, Brush } from "@element-plus/icons-vue";
import { useI18n } from "@/core/i18n";
import { preferences, updatePreferences } from "../../index";
import type { BuiltinThemeType } from "../../types";

const { t } = useI18n();

const BUILTIN_THEMES: { nameKey: string; color: string; type: BuiltinThemeType }[] = [
  { nameKey: "appearance.themes.default", color: "#1677ff", type: "default" },
  { nameKey: "appearance.themes.violet", color: "#722ed1", type: "violet" },
  { nameKey: "appearance.themes.pink", color: "#eb2f96", type: "pink" },
  { nameKey: "appearance.themes.yellow", color: "#fadb14", type: "yellow" },
  { nameKey: "appearance.themes.skyBlue", color: "#52c41a", type: "sky-blue" },
  { nameKey: "appearance.themes.green", color: "#13c2c2", type: "green" },
  { nameKey: "appearance.themes.zinc", color: "#595959", type: "zinc" },
  { nameKey: "appearance.themes.deepGreen", color: "#13a8a8", type: "deep-green" },
  { nameKey: "appearance.themes.deepBlue", color: "#1677ff", type: "deep-blue" },
  { nameKey: "appearance.themes.orange", color: "#fa8c16", type: "orange" },
  { nameKey: "appearance.themes.rose", color: "#f5222d", type: "rose" },
  { nameKey: "appearance.themes.neutral", color: "#595959", type: "neutral" },
  { nameKey: "appearance.themes.slate", color: "#54687a", type: "slate" },
  { nameKey: "appearance.themes.gray", color: "#595959", type: "gray" },
  { nameKey: "appearance.themes.red", color: "#e6393b", type: "red" },
  { nameKey: "appearance.themes.stone", color: "#6b6b6b", type: "stone" },
  { nameKey: "appearance.themes.custom", color: "custom", type: "custom" },
];

const RADIUS_OPTIONS = [
  { label: "0", value: "0" },
  { label: "0.25", value: "0.25" },
  { label: "0.5", value: "0.5" },
  { label: "0.75", value: "0.75" },
  { label: "1", value: "1" },
];

function handleBuiltinThemeChange(type: BuiltinThemeType) {
  if (type === "custom") {
    updatePreferences({ theme: { builtinType: "custom" } });
    return;
  }
  const theme = BUILTIN_THEMES.find((item) => item.type === type);
  if (theme) {
    updatePreferences({ theme: { builtinType: type, colorPrimary: theme.color } });
  }
}
</script>

<style scoped lang="scss">
.appearance-panel {
  display: flex;
  flex-direction: column;
  gap: 12px;
}

.appearance-section {
  display: flex;
  flex-direction: column;
  gap: 8px;

  .section-title {
    margin: 0 0 8px;
    font-size: 14px;
    font-weight: 600;
    color: var(--el-text-color-primary);
  }
}

.preference-item {
  display: flex;
  justify-content: space-between;
  align-items: center;
  padding: 4px 0;
  font-size: 13px;
}

.theme-mode-options {
  display: grid;
  grid-template-columns: repeat(3, 1fr);
  gap: 8px;
}

.theme-mode-wrapper {
  display: flex;
  flex-direction: column;
  align-items: center;
  gap: 6px;
  cursor: pointer;
}

.theme-mode-item {
  width: 100%;
  height: 64px;
  border-radius: 6px;
  border: 2px solid var(--el-border-color-lighter);
  display: flex;
  align-items: center;
  justify-content: center;
  transition: all 0.3s cubic-bezier(0.4, 0, 0.2, 1);
  color: var(--el-text-color-secondary);
  background: var(--el-fill-color-light);

  &:hover {
    border-color: var(--el-color-primary-light-7);
    background: var(--el-color-primary-light-9);
    transform: translateY(-2px);
    box-shadow: 0 4px 12px rgba(0, 0, 0, 0.08);
    color: var(--el-color-primary);
  }

  &.active {
    border: 2px solid var(--el-color-primary);
    background: var(--el-fill-color-light);
    box-shadow: 0 0 0 1px var(--el-color-primary);
    color: var(--el-color-primary);
  }
}

.theme-mode-label {
  font-size: 12px;
  font-weight: 500;
  color: var(--el-text-color-regular);
  transition: all 0.3s;
  text-align: center;
}

.builtin-themes-grid {
  display: grid;
  grid-template-columns: repeat(3, 1fr);
  gap: 8px;
}

.builtin-theme-wrapper {
  display: flex;
  flex-direction: column;
  align-items: center;
  gap: 4px;
  cursor: pointer;
}

.builtin-theme-item {
  width: 100%;
  height: 64px;
  display: flex;
  flex-direction: column;
  align-items: center;
  justify-content: center;
  border: 2px solid var(--el-border-color-lighter);
  border-radius: 6px;
  transition: all 0.3s cubic-bezier(0.4, 0, 0.2, 1);
  background: var(--el-fill-color-light);

  &:hover {
    border-color: var(--el-color-primary-light-7);
    background: var(--el-color-primary-light-9);
    transform: translateY(-2px);
    box-shadow: 0 4px 12px rgba(0, 0, 0, 0.08);
  }

  &.active {
    border: 2px solid var(--el-color-primary);
    background: var(--el-fill-color-light);
    box-shadow: 0 0 0 1px var(--el-color-primary);
  }
}

.theme-color-preview {
  width: 28px;
  height: 28px;
  border-radius: 5px;
  display: flex;
  align-items: center;
  justify-content: center;
  transition: all 0.3s;
}

.theme-name {
  font-size: 12px;
  font-weight: 500;
  color: var(--el-text-color-regular);
  text-align: center;
}

.radius-group {
  display: flex;
  gap: 4px;
  background: var(--el-fill-color-light);
  border-radius: 6px;
  padding: 2px;
}

.radius-item {
  flex: 1;
  height: 36px;
  border-radius: 5px;
  display: flex;
  align-items: center;
  justify-content: center;
  font-size: 12px;
  font-weight: 500;
  cursor: pointer;
  transition: all 0.2s;

  &:hover {
    color: var(--el-color-primary);
  }

  &.active {
    background: var(--el-bg-color);
    box-shadow: 0 2px 4px rgba(0, 0, 0, 0.1);
    color: var(--el-color-primary);
  }
}
</style>
