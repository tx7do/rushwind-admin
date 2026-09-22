import { markRaw, reactive, readonly, watch } from "vue";

import { breakpointsTailwind, useBreakpoints, useDebounceFn } from "@vueuse/core";

import { defaultPreferences } from "./config/default";
import type { DeepPartial, InitialOptions, Preferences } from "./types";
import type { SupportedLanguagesType } from "./types";
import { updateCSSVariables } from "./update-css-variables";
import { StorageManager } from "@/core/storage";
import { loadLocaleMessages } from "@/core/i18n";
import { merge } from "@/utils/merge";
import { isMacOs } from "@/utils/inference";

const STORAGE_KEY = "preferences";
const STORAGE_KEY_LOCALE = `${STORAGE_KEY}-locale`;
const STORAGE_KEY_THEME = `${STORAGE_KEY}-theme`;

/**
 * 旧版缓存迁移：设计语言规范（docs/design-language.md，2026-09-08）把默认
 * 主色/语义色统一到 vben 基准。缓存按精确旧值匹配改写，只有仍停留在迁移前
 * 默认色的用户被迁移，自选过主题色的用户不受影响。
 */
const LEGACY_THEME_COLORS = {
  colorPrimary: "hsl(220 100% 55%)",
  colorSuccess: "hsl(145 100% 35%)",
  colorWarning: "hsl(32 100% 50%)",
  colorDestructive: "hsl(0 91% 60%)",
};

function migrateCachedPreferences(cached: null | Preferences): null | Preferences {
  if (!cached?.theme) {
    return cached;
  }
  const { theme } = cached;
  if (theme.colorPrimary === LEGACY_THEME_COLORS.colorPrimary) {
    theme.colorPrimary = defaultPreferences.theme.colorPrimary;
  }
  if (theme.colorSuccess === LEGACY_THEME_COLORS.colorSuccess) {
    theme.colorSuccess = defaultPreferences.theme.colorSuccess;
  }
  if (theme.colorWarning === LEGACY_THEME_COLORS.colorWarning) {
    theme.colorWarning = defaultPreferences.theme.colorWarning;
  }
  if (theme.colorDestructive === LEGACY_THEME_COLORS.colorDestructive) {
    theme.colorDestructive = defaultPreferences.theme.colorDestructive;
  }
  return cached;
}

class PreferenceManager {
  private cache: null | StorageManager = null;
  // private flattenedState: Flatten<Preferences>;
  private initialPreferences: Preferences = defaultPreferences;
  private isInitialized: boolean = false;
  private readonly savePreferences: (preference: Preferences) => void;
  private state: Preferences = reactive<Preferences>({
    ...this.loadPreferences(),
  });
  constructor() {
    this.cache = new StorageManager();

    // 避免频繁的操作缓存
    this.savePreferences = useDebounceFn(
      (preference: Preferences) => this._savePreferences(preference),
      150
    );
  }

  /**
   * 保存偏好设置
   * @param {Preferences} preference - 需要保存的偏好设置
   */
  private _savePreferences(preference: Preferences) {
    this.cache?.setItem(STORAGE_KEY, preference);
    this.cache?.setItem(STORAGE_KEY_LOCALE, preference.app.locale);
    this.cache?.setItem(STORAGE_KEY_THEME, preference.theme.mode);
  }

  /**
   * 处理更新的键值
   * 根据更新的键值执行相应的操作。
   * @param {DeepPartial<Preferences>} updates - 部分更新的偏好设置
   */
  private handleUpdates(updates: DeepPartial<Preferences>) {
    const themeUpdates = updates.theme || {};
    const appUpdates = updates.app || {};
    if (themeUpdates && Object.keys(themeUpdates).length > 0) {
      updateCSSVariables(this.state);
    }

    if (Reflect.has(appUpdates, "colorGrayMode") || Reflect.has(appUpdates, "colorWeakMode")) {
      this.updateColorMode(this.state);
    }

    // 语言切换：加载语言包并同步 vue-i18n locale
    if (Reflect.has(appUpdates, "locale") && appUpdates.locale) {
      loadLocaleMessages(appUpdates.locale as SupportedLanguagesType);
    }
  }

  private initPlatform() {
    const dom = document.documentElement;
    dom.dataset.platform = isMacOs() ? "macOs" : "window";
  }

  /**
   *  从缓存中加载偏好设置。如果缓存中没有找到对应的偏好设置，则返回默认偏好设置。
   */
  private loadCachedPreferences() {
    return migrateCachedPreferences(this.cache?.getItem<Preferences>(STORAGE_KEY) ?? null);
  }

  /**
   * 加载偏好设置
   * @returns {Preferences} 加载的偏好设置
   */
  private loadPreferences(): Preferences {
    return this.loadCachedPreferences() || { ...defaultPreferences };
  }

  /**
   * 监听状态和系统偏好设置的变化。
   */
  private setupWatcher() {
    if (this.isInitialized) {
      return;
    }

    // 监听断点，判断是否移动端
    const breakpoints = useBreakpoints(breakpointsTailwind);
    const isMobile = breakpoints.smaller("md");
    watch(
      () => isMobile.value,
      (val) => {
        this.updatePreferences({
          app: { isMobile: val },
        });
      },
      { immediate: true }
    );

    // 监听系统主题偏好设置变化
    window
      .matchMedia("(prefers-color-scheme: dark)")
      .addEventListener("change", ({ matches: isDark }) => {
        this.updatePreferences({
          theme: { mode: isDark ? "dark" : "light" },
        });
      });
  }

  /**
   * 更新页面颜色模式（灰色、色弱）
   * @param preference
   */
  private updateColorMode(preference: Preferences) {
    if (preference.app) {
      const { colorGrayMode, colorWeakMode } = preference.app;
      const dom = document.documentElement;
      const COLOR_WEAK = "invert-mode";
      const COLOR_GRAY = "grayscale-mode";
      colorWeakMode ? dom.classList.add(COLOR_WEAK) : dom.classList.remove(COLOR_WEAK);
      colorGrayMode ? dom.classList.add(COLOR_GRAY) : dom.classList.remove(COLOR_GRAY);
    }
  }

  clearCache() {
    [STORAGE_KEY, STORAGE_KEY_LOCALE, STORAGE_KEY_THEME].forEach((key) => {
      this.cache?.removeItem(key);
    });
  }

  public getInitialPreferences() {
    return this.initialPreferences;
  }

  public getPreferences() {
    return readonly(this.state);
  }

  /**
   * 覆盖偏好设置
   * overrides  要覆盖的偏好设置
   * namespace  命名空间
   */
  public async initPreferences({ namespace, overrides }: InitialOptions) {
    // 是否初始化过
    if (this.isInitialized) {
      return;
    }
    // 初始化存储管理器
    this.cache = new StorageManager({ prefix: namespace });
    // 合并初始偏好设置
    this.initialPreferences = merge({}, overrides, defaultPreferences);

    // 加载并合并当前存储的偏好设置
    const mergedPreference = merge(
      {},
      // overrides,
      this.loadCachedPreferences() || {},
      this.initialPreferences
    );

    // 更新偏好设置
    this.updatePreferences(mergedPreference);

    this.setupWatcher();

    this.initPlatform();
    // 标记为已初始化
    this.isInitialized = true;
  }

  /**
   * 重置偏好设置
   * 偏好设置将被重置为初始值，并从 localStorage 中移除。
   *
   * @example
   * 假设 initialPreferences 为 { theme: 'light', language: 'en' }
   * 当前 state 为 { theme: 'dark', language: 'fr' }
   * this.resetPreferences();
   * 调用后，state 将被重置为 { theme: 'light', language: 'en' }
   * 并且 localStorage 中的对应项将被移除
   */
  resetPreferences() {
    // 将状态重置为初始偏好设置
    Object.assign(this.state, this.initialPreferences);
    // 保存重置后的偏好设置
    this.savePreferences(this.state);
    // 从存储中移除偏好设置项
    [STORAGE_KEY, STORAGE_KEY_THEME, STORAGE_KEY_LOCALE].forEach((key) => {
      this.cache?.removeItem(key);
    });
    this.updatePreferences(this.state);
  }

  /**
   * 更新偏好设置
   * @param updates - 要更新的偏好设置
   */
  public updatePreferences(updates: DeepPartial<Preferences>) {
    const mergedState = merge({}, updates, markRaw(this.state));

    Object.assign(this.state, mergedState);

    // 根据更新的键值执行相应的操作
    this.handleUpdates(updates);
    this.savePreferences(this.state);
  }
}

const preferencesManager = new PreferenceManager();
export { PreferenceManager, preferencesManager };
