import { preferences } from "@/core/preferences";

/**
 * 是否是暗黑模式
 */
export function isDarkMode() {
  return preferences.theme.mode === "dark";
}
