import { hexToRgb, rgbToHex } from "./color";
import { ThemeModeType } from "@/core/preferences";
import { VxeUI } from "vxe-table";

/**
 * 加深颜色值
 * @param {String} color 颜色值字符串
 * @param {Number} level 加深的程度，限0-1之间
 * @returns {String} 返回处理后的颜色值
 */
export function getDarkColor(color: string, level: number): string {
  const rgb = hexToRgb(color);
  for (let i = 0; i < 3; i++) rgb[i] = Math.round(20.5 * level + rgb[i] * (1 - level));
  return rgbToHex(rgb[0], rgb[1], rgb[2]);
}

/**
 * 变浅颜色值
 * @param {String} color 颜色值字符串
 * @param {Number} level 加深的程度，限0-1之间
 * @returns {String} 返回处理后的颜色值
 */
export const getLightColor = (color: string, level: number): string => {
  const rgb = hexToRgb(color);
  for (let i = 0; i < 3; i++) rgb[i] = Math.round(255 * level + rgb[i] * (1 - level));
  return rgbToHex(rgb[0], rgb[1], rgb[2]);
};

/**
 * 生成主题色
 * @param primary 主题色
 * @param theme 主题类型
 */
export function generateThemeColors(primary: string, theme: ThemeModeType) {
  const colors: Record<string, string> = {
    primary,
  };

  // 生成浅色变体
  for (let i = 1; i <= 9; i++) {
    colors[`primary-light-${i}`] =
      theme === "light" ? `${getLightColor(primary, i / 10)}` : `${getDarkColor(primary, i / 10)}`;
  }

  // 生成深色变体
  colors["primary-dark-2"] =
    theme === "light" ? `${getLightColor(primary, 0.2)}` : `${getDarkColor(primary, 0.3)}`;

  return colors;
}

/**
 * 生成颜色变量
 *
 * @description
 * 根据主题配置生成完整的 CSS 颜色变量，包括主题色、成功色、警告色、错误色等
 * 支持浅色和深色模式的自动适配
 *
 * @param themeConfig 主题配置对象
 * @returns CSS 变量映射对象
 *
 * @example
 * ```typescript
 * const variables = generateColorVariables({
 *   colorPrimary: '#4080FF',
 *   colorSuccess: '#52c41a',
 *   colorWarning: '#faad14',
 *   colorDestructive: '#ff4d4f',
 *   mode: 'light',
 *   radius: '6px'
 * });
 * ```
 */
export function generateColorVariables(themeConfig: {
  colorPrimary: string;
  colorSuccess: string;
  colorWarning: string;
  colorDestructive: string;
  mode: ThemeModeType;
  radius?: string;
}): Record<string, string> {
  const {
    colorPrimary,
    colorSuccess,
    colorWarning,
    colorDestructive,
    mode,
    radius = "6px",
  } = themeConfig;

  const isDark = mode === "dark";
  const variables: Record<string, string> = {};

  // ====================== 主题色（Primary）======================
  variables["--el-color-primary"] = colorPrimary;

  // 生成主题色的浅色和深色变体
  for (let i = 1; i <= 9; i++) {
    const level = i / 10;
    variables[`--el-color-primary-light-${i}`] = isDark
      ? getDarkColor(colorPrimary, level)
      : getLightColor(colorPrimary, level);
  }

  // 主题色深色变体
  variables["--el-color-primary-dark-2"] = isDark
    ? getDarkColor(colorPrimary, 0.3)
    : getLightColor(colorPrimary, 0.2);

  // ====================== 成功色（Success）======================
  variables["--el-color-success"] = colorSuccess;

  for (let i = 1; i <= 9; i++) {
    const level = i / 10;
    variables[`--el-color-success-light-${i}`] = isDark
      ? getDarkColor(colorSuccess, level)
      : getLightColor(colorSuccess, level);
  }

  variables["--el-color-success-dark-2"] = isDark
    ? getDarkColor(colorSuccess, 0.3)
    : getLightColor(colorSuccess, 0.2);

  // ====================== 警告色（Warning）======================
  variables["--el-color-warning"] = colorWarning;

  for (let i = 1; i <= 9; i++) {
    const level = i / 10;
    variables[`--el-color-warning-light-${i}`] = isDark
      ? getDarkColor(colorWarning, level)
      : getLightColor(colorWarning, level);
  }

  variables["--el-color-warning-dark-2"] = isDark
    ? getDarkColor(colorWarning, 0.3)
    : getLightColor(colorWarning, 0.2);

  // ====================== 错误色（Destructive/Error）======================
  variables["--el-color-danger"] = colorDestructive;
  variables["--el-color-error"] = colorDestructive;

  for (let i = 1; i <= 9; i++) {
    const level = i / 10;
    variables[`--el-color-danger-light-${i}`] = isDark
      ? getDarkColor(colorDestructive, level)
      : getLightColor(colorDestructive, level);
    variables[`--el-color-error-light-${i}`] = variables[`--el-color-danger-light-${i}`];
  }

  variables["--el-color-danger-dark-2"] = isDark
    ? getDarkColor(colorDestructive, 0.3)
    : getLightColor(colorDestructive, 0.2);
  variables["--el-color-error-dark-2"] = variables["--el-color-danger-dark-2"];

  // ====================== 信息色（Info）======================
  // 使用主题色作为信息色的基础
  const colorInfo = colorPrimary;
  variables["--el-color-info"] = colorInfo;

  for (let i = 1; i <= 9; i++) {
    const level = i / 10;
    variables[`--el-color-info-light-${i}`] = isDark
      ? getDarkColor(colorInfo, level)
      : getLightColor(colorInfo, level);
  }

  variables["--el-color-info-dark-2"] = isDark
    ? getDarkColor(colorInfo, 0.3)
    : getLightColor(colorInfo, 0.2);

  // ====================== 圆角 ======================
  if (radius) {
    variables["--el-border-radius-base"] = radius;
    variables["--el-border-radius-small"] = `calc(${radius} - 2px)`;
    variables["--el-border-radius-round"] = `calc(${radius} * 2)`;
  }

  return variables;
}

/**
 * 生成器颜色变量（基于颜色数组）
 *
 * @description
 * 根据颜色数组生成对应的 CSS 变量，支持自定义颜色名称和别名
 * 主要用于 preferences 系统的颜色变量更新
 *
 * @param colors 颜色配置数组，每个元素包含 color（颜色值）、name（颜色名）、alias（可选别名）
 * @returns CSS 变量映射对象
 *
 * @example
 * ```typescript
 * const variables = generatorColorVariables([
 *   { color: '#4080FF', name: 'primary' },
 *   { color: '#52c41a', name: 'green', alias: 'success' },
 *   { color: '#faad14', name: 'yellow', alias: 'warning' },
 *   { color: '#ff4d4f', name: 'red', alias: 'destructive' }
 * ]);
 * // 生成: { '--primary-500': '#4080FF', '--green-500': '#52c41a', ... }
 * ```
 */
export function generatorColorVariables(
  colors: Array<{ color: string; name: string; alias?: string }>
): Record<string, string> {
  const variables: Record<string, string> = {};

  colors.forEach(({ color, name, alias }) => {
    // 生成 50-950 的颜色梯度（Tailwind CSS 风格）
    const baseName = alias || name;

    // 主要颜色（500 级别）
    variables[`--${baseName}-500`] = color;

    // 生成其他级别的颜色（简化版，可以根据需要扩展）
    for (let i = 50; i <= 950; i += 50) {
      if (i === 500) continue; // 跳过 500，已经设置

      const level = i / 1000;
      let generatedColor: string;

      if (i < 500) {
        // 浅色：向白色靠近
        generatedColor = getLightColor(color, 1 - level * 0.8);
      } else {
        // 深色：向黑色靠近
        generatedColor = getDarkColor(color, (level - 0.5) * 2);
      }

      variables[`--${baseName}-${i}`] = generatedColor;
    }

    // 如果 name 和 alias 都存在，也为原始 name 生成变量
    if (alias && alias !== name) {
      variables[`--${name}-500`] = color;
      for (let i = 50; i <= 950; i += 50) {
        if (i === 500) continue;
        variables[`--${name}-${i}`] = variables[`--${baseName}-${i}`];
      }
    }
  });

  return variables;
}

export function applyTheme(colors: Record<string, string>) {
  const el = document.documentElement;

  Object.entries(colors).forEach(([key, value]) => {
    el.style.setProperty(`--el-color-${key}`, value);
  });

  // 确保主题色立即生效，强制重新渲染
  requestAnimationFrame(() => {
    // 触发样式重新计算
    el.style.setProperty("--theme-update-trigger", Date.now().toString());
  });
}

/**
 * 切换暗黑模式
 *
 * @param isDark 是否启用暗黑模式
 */
export function toggleDarkMode(isDark: boolean) {
  if (isDark) {
    document.documentElement.classList.add("dark");
  } else {
    document.documentElement.classList.remove("dark");
  }

  // vxe-table 4.7+ 暗色主题同步
  // 通过 VxeUI.setTheme() API 切换，vxe-table 会自动设置 data-vxe-ui-theme 属性
  // 内置的 [data-vxe-ui-theme=dark/light] CSS 变量随即生效
  VxeUI.setTheme(isDark ? "dark" : "light");
}

/**
 * 切换浅色主题下的侧边栏颜色方案
 *
 * @param isBuleSidebar 布尔值，表示是否开启深蓝色侧边栏颜色方案
 */
export function toggleSidebarColor(isBuleSidebar: boolean) {
  if (isBuleSidebar) {
    document.documentElement.classList.add("sidebar-color-blue");
  } else {
    document.documentElement.classList.remove("sidebar-color-blue");
  }
}
