<template>
  <div class="shortcut-panel">
    <section class="shortcut-section">
      <h3 class="section-title">{{ t("preferences.shortcut.global") }}</h3>

      <div class="preference-item">
        <span>{{ t("preferences.shortcut.shortcutKeys") }}</span>
        <ElSwitch
          :model-value="preferences.shortcutKeys.enable"
          @change="
            (val: string | number | boolean) =>
              updatePreferences({ shortcutKeys: { enable: val as boolean } })
          "
        />
      </div>

      <div class="preference-item" :class="{ disabled: !preferences.shortcutKeys.enable }">
        <span>{{ t("preferences.shortcut.globalSearch") }}</span>
        <div class="shortcut-key-row">
          <kbd class="kbd-badge">Ctrl</kbd>
          <kbd class="kbd-badge">K</kbd>
          <ElSwitch
            :disabled="!preferences.shortcutKeys.enable"
            :model-value="preferences.shortcutKeys.globalSearch"
            @change="
              (val: string | number | boolean) =>
                updatePreferences({ shortcutKeys: { globalSearch: val as boolean } })
            "
          />
        </div>
      </div>

      <div class="preference-item" :class="{ disabled: !preferences.shortcutKeys.enable }">
        <span>{{ t("preferences.shortcut.logout") }}</span>
        <div class="shortcut-key-row">
          <kbd class="kbd-badge">Alt</kbd>
          <kbd class="kbd-badge">Q</kbd>
          <ElSwitch
            :disabled="!preferences.shortcutKeys.enable"
            :model-value="preferences.shortcutKeys.globalLogout"
            @change="
              (val: string | number | boolean) =>
                updatePreferences({ shortcutKeys: { globalLogout: val as boolean } })
            "
          />
        </div>
      </div>

      <div class="preference-item" :class="{ disabled: !preferences.shortcutKeys.enable }">
        <span>{{ t("preferences.shortcut.lockScreen") }}</span>
        <div class="shortcut-key-row">
          <kbd class="kbd-badge">Alt</kbd>
          <kbd class="kbd-badge">L</kbd>
          <ElSwitch
            :disabled="!preferences.shortcutKeys.enable"
            :model-value="preferences.shortcutKeys.globalLockScreen"
            @change="
              (val: string | number | boolean) =>
                updatePreferences({ shortcutKeys: { globalLockScreen: val as boolean } })
            "
          />
        </div>
      </div>
    </section>
  </div>
</template>

<script setup lang="ts">
import { ElSwitch } from "element-plus";
import { useI18n } from "@/core/i18n";
import { preferences, updatePreferences } from "../../index";

const { t } = useI18n();
</script>

<style scoped lang="scss">
.shortcut-panel {
  display: flex;
  flex-direction: column;
  gap: 12px;
}

.shortcut-section {
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

  &.disabled {
    opacity: 0.4;
    pointer-events: none;
  }
}

.shortcut-key-row {
  display: flex;
  align-items: center;
  gap: 6px;
}

.kbd-badge {
  display: inline-flex;
  align-items: center;
  justify-content: center;
  min-width: 24px;
  height: 22px;
  padding: 0 6px;
  font-size: 11px;
  font-family: inherit;
  line-height: 1;
  color: var(--el-text-color-secondary);
  background: var(--el-fill-color-light);
  border: 1px solid var(--el-border-color);
  border-radius: 4px;
  box-shadow: 0 1px 1px rgba(0, 0, 0, 0.06);
}
</style>
