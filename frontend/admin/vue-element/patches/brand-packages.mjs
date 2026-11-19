// brand-packages.mjs — pnpm postinstall 品牌补丁（vue-element 端）。
//
// 机制与替换表同 brand/overlay/react/patches/brand-packages.mjs（见其头注）：
// 上游把壳层机制抽成 @go-wind/admin-core + @go-wind/admin-element 常规构建包后，
// preferences 默认值与 core 命名空间文案（登录页 headerTitle/brandTitle/版权）
// 进了 admin-element 的 dist，snapshot 级 sed 够不着，postinstall 驱动本脚本
// 安装后重写。@go-wind/admin-core dist 实测零品牌串（2026-11-15 核对 0.1.0），
// 无需补丁；admin-element 另有样式注入标记 __gowind-styles__ 一并替换。
import { existsSync, readFileSync, writeFileSync } from 'node:fs';
import { dirname, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

const here = dirname(fileURLToPath(import.meta.url));
const root = resolve(here, '..');

// 目标包 dist（包版本由 package.json dependencies 钉定，路径形状稳定）。
const targets = [
  'node_modules/@go-wind/admin-element/dist/admin-element.js',
];

// [查找串, 替换串]，长串在前防前缀截断命中。
const replacements = [
  ['GoWind Admin', 'RushWind Admin'],
  ['GoWind', 'RushWind'],
  ['风行中后台管理系统', '锐风中后台管理系统'],
  ['https://www.gowind.cloud', 'https://github.com/tx7do/rushwind'],
  ['__gowind-styles__', '__rushwind-styles__'],
];

// 终态断言：这些上游品牌串在补丁后必须零残留（否则升级带来了表外形状）。
const forbidden = ['GoWind', '风行中后台', 'gowind.cloud', '__gowind-styles__'];

let failed = false;
for (const rel of targets) {
  const file = resolve(root, rel);
  if (!existsSync(file)) {
    console.warn(`[brand-packages] ${rel} 不存在（未安装？），跳过品牌补丁`);
    continue;
  }
  let content = readFileSync(file, 'utf8');
  for (const [from, to] of replacements) {
    const count = content.split(from).length - 1;
    if (count > 0) {
      content = content.split(from).join(to);
      console.log(`[brand-packages] ${rel}: "${from}" -> "${to}"（${count} 处）`);
    }
  }
  const leftover = forbidden.filter((p) => content.includes(p));
  if (leftover.length > 0) {
    console.error(`[brand-packages] ${rel}: 补丁后仍有上游品牌串残留 ${JSON.stringify(leftover)}——包已升级且替换表没接住？请核对 brand/overlay/vue-element/patches/brand-packages.mjs`);
    failed = true;
  } else {
    writeFileSync(file, content);
    console.log(`[brand-packages] ${rel}: 品牌补丁完成（零残留校验通过）`);
  }
}
process.exitCode = failed ? 1 : 0;
