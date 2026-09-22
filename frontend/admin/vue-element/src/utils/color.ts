/**
 * 生成基于字符串的固定随机色（HSL模式，保证饱和度和明度适中）
 * @param str
 */
export const getRandomColor = (str: string) => {
  let hash = 0;
  for (let i = 0; i < str.length; i++) {
    hash = str.charCodeAt(i) + ((hash << 5) - hash);
  }
  const hue = Math.abs(hash % 360);
  return `hsl(${hue}, 50%, 85%)`;
};

/**
 * 根据首字母生成固定随机色
 * @param char
 */
export const getCharColor = (char: string) => {
  let hash = 0;
  for (let i = 0; i < char.length; i++) {
    hash = char.charCodeAt(i) + ((hash << 5) - hash);
  }
  const hue = Math.abs(hash % 360);
  const saturation = 60;
  const lightness = 45;
  return `hsl(${hue}, ${saturation}%, ${lightness}%)`;
};

// 辅助函数：将十六进制颜色转换为 RGB
export function hexToRgb(hex: string): [number, number, number] {
  const bigint = parseInt(hex.slice(1), 16);
  return [(bigint >> 16) & 255, (bigint >> 8) & 255, bigint & 255];
}

// 辅助函数：将 RGB 转换为十六进制颜色
export function rgbToHex(r: number, g: number, b: number): string {
  return `#${((1 << 24) + (r << 16) + (g << 8) + b).toString(16).slice(1)}`;
}

/**
 * 将 Hex 颜色转换为 HSL 数值字符串
 * @returns 如 "210 100% 50%"，可直接用于 hsl(var(--primary))
 */
export function hexToHsl(hex: string): string {
  const [r, g, b] = hexToRgb(hex);
  const rn = r / 255;
  const gn = g / 255;
  const bn = b / 255;
  const max = Math.max(rn, gn, bn);
  const min = Math.min(rn, gn, bn);
  const l = (max + min) / 2;

  let h: number;
  if (max === min) {
    return `0 0% ${Math.round(l * 100)}%`;
  }

  const d = max - min;
  const s = l > 0.5 ? d / (2 - max - min) : d / (max + min);

  if (max === rn) {
    h = ((gn - bn) / d + (gn < bn ? 6 : 0)) / 6;
  } else if (max === gn) {
    h = ((bn - rn) / d + 2) / 6;
  } else {
    h = ((rn - gn) / d + 4) / 6;
  }

  return `${Math.round(h * 360)} ${Math.round(s * 100)}% ${Math.round(l * 100)}%`;
}

/**
 * 将 HSL 颜色转换为十六进制格式
 */
export function hslToHex(h: number, s: number, l: number): string {
  s /= 100;
  l /= 100;
  const a = s * Math.min(l, 1 - l);
  const f = (n: number) => {
    const k = (n + h / 30) % 12;
    const color = l - a * Math.max(Math.min(k - 3, 9 - k, 1), -1);
    return Math.round(255 * color);
  };
  return rgbToHex(f(0), f(8), f(4));
}

/**
 * 将任意 CSS 颜色格式转换为十六进制格式
 * 支持: #hex, hsl(), rgb()
 */
export function toHex(color: string): string {
  if (!color) return color;
  // 已经是十六进制
  if (color.startsWith("#")) return color;
  // HSL 格式: hsl(h, s%, l%) 或 hsl(h s% l%)
  const hslMatch = color.match(
    /hsl\(\s*(\d+(?:\.\d+)?)\s*,?\s*(\d+(?:\.\d+)?)%\s*,?\s*(\d+(?:\.\d+)?)%\s*\)/
  );
  if (hslMatch) {
    return hslToHex(Number(hslMatch[1]), Number(hslMatch[2]), Number(hslMatch[3]));
  }
  // RGB 格式: rgb(r, g, b) 或 rgb(r g b)
  const rgbMatch = color.match(/rgb\(\s*(\d+)\s*,?\s*(\d+)\s*,?\s*(\d+)\s*\)/);
  if (rgbMatch) {
    return rgbToHex(Number(rgbMatch[1]), Number(rgbMatch[2]), Number(rgbMatch[3]));
  }
  return color;
}

/**
 * 将十六进制颜色转换为 HSL 格式字符串
 * @param hex 十六进制颜色
 */
export function hexToHslString(hex: string): string {
  let r = 0,
    g = 0,
    b = 0;
  if (hex.length === 4) {
    r = parseInt(hex[1] + hex[1], 16);
    g = parseInt(hex[2] + hex[2], 16);
    b = parseInt(hex[3] + hex[3], 16);
  } else if (hex.length === 7) {
    r = parseInt(hex[1] + hex[2], 16);
    g = parseInt(hex[3] + hex[4], 16);
    b = parseInt(hex[5] + hex[6], 16);
  }
  r /= 255;
  g /= 255;
  b /= 255;
  const cmin = Math.min(r, g, b);
  const cmax = Math.max(r, g, b);
  const delta = cmax - cmin;
  let h: number, s: number, l: number;
  if (delta === 0) h = 0;
  else if (cmax === r) h = ((g - b) / delta) % 6;
  else if (cmax === g) h = (b - r) / delta + 2;
  else h = (r - g) / delta + 4;
  h = Math.round(h * 60);
  if (h < 0) h += 360;
  l = (cmax + cmin) / 2;
  s = delta === 0 ? 0 : delta / (1 - Math.abs(2 * l - 1));
  s = Math.round(s * 100);
  l = Math.round(l * 100);
  return `${h} ${s}% ${l}%`;
}
