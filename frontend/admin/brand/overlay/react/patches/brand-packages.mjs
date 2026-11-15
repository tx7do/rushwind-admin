// brand-packages.mjs — pnpm postinstall 品牌补丁（react 端）。
//
// 背景：上游 2026-11-15 把壳层机制抽成 npm 包（@go-wind/admin-react，常规构建
// 形态），原快照内的 preferences 默认值（应用名/版权公司名/公司链接）与登录页
// 核心文案（auth 命名空间）随之进了包 dist——node_modules 不入库，snapshot 级
// sed 够不着。包没有导出声明式品牌覆写 API（只有运行时 setPreferences），因此
// 品牌覆写机制补一条安装后重写：postinstall 钩子（apply-brand.sh 注入
// package.json）驱动本脚本，把包 dist 里的上游品牌默认值改写为 RushWind。
//
// 语义：
// - 幂等：重复 install / 手动重跑安全（替换串已不在时各规则自然空转）。
// - 响亮：替换全部完成后断言零上游品牌串残留——包升级引入新的品牌串形状时
//   （替换表没接住）install 当场失败，强制同步更新本表（与快照 sed 表同一
//   纪律：替换失败必须可见，不允许静默漏打）。
// - 包缺失（未安装/仅源码检出）→ 提示后退出 0，不算错误。
// - 只重写用户可见品牌串；`--gowind-*` CSS 变量名与类名是包 CSS 与宿主样式间
//   的内部 API，改名会断样式联动，刻意不替换（与旧替换表的口径一致）。
import { existsSync, readFileSync, writeFileSync } from 'node:fs';
import { dirname, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

const here = dirname(fileURLToPath(import.meta.url));
const root = resolve(here, '..');

// 目标包 dist（包版本由 package.json dependencies 钉定，路径形状稳定）。
const targets = [
  'node_modules/@go-wind/admin-react/dist/admin-react.js',
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
    console.error(`[brand-packages] ${rel}: 补丁后仍有上游品牌串残留 ${JSON.stringify(leftover)}——包已升级且替换表没接住？请核对 brand/overlay/react/patches/brand-packages.mjs`);
    failed = true;
  } else {
    writeFileSync(file, content);
    console.log(`[brand-packages] ${rel}: 品牌补丁完成（零残留校验通过）`);
  }
}
process.exitCode = failed ? 1 : 0;
