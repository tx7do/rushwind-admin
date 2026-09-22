import type React from 'react';

/**
 * 品牌标识「锐风 RushWind」v2 登录页插画（RushWind 框架仓 assets/logo，R 字单标）：
 * R 字主标 + 能量流光沿笔画巡航（自左下向右上，与品牌渐变同向）+ 呼吸辉光 +
 * 双层对旋虚线环流 + 双彗星弧光反向绕行 + 四向漂移风痕 + 尾羽周期性呼出阵风。
 * 色板同源 RushWind 品牌：Wind Teal #2DD4BF → Wind Sky #38BDF8 → Wind Indigo #818CF8，
 * 渐变方向固定左下 → 右上（与「风掠过」方向一致）。
 * 全部动效走 CSS keyframes，prefers-reduced-motion 下整体静止；
 * 浮动动效由父级样式提供。
 * 注：SVG 内嵌 <style> 为文档级作用域，keyframes/类名均带 rw- 前缀防碰撞。
 */
const ANIMATION_CSS = `
.rw-ring1,.rw-ring2,.rw-orbit,.rw-orbit2,.rw-comet1,.rw-comet2{transform-box:view-box;transform-origin:280px 250px}
.rw-ring1{animation:rw-spin 48s linear infinite}
.rw-ring2{animation:rw-spin 70s linear infinite reverse}
.rw-orbit{animation:rw-spin 14s linear infinite}
.rw-orbit2{animation:rw-spin 22s linear infinite reverse}
.rw-comet1{animation:rw-spin 9s linear infinite}
.rw-comet2{animation:rw-spin 13s linear infinite reverse}
.rw-streak{animation:rw-drift 9s ease-in-out infinite alternate}
.rw-s2{animation-duration:12s;animation-delay:-4s}
.rw-s3{animation-duration:10s;animation-delay:-2s}
.rw-s4{animation-duration:13s;animation-delay:-6s}
.rw-glow{transform-box:view-box;transform-origin:280px 250px;animation:rw-breathe 6s ease-in-out infinite}
.rw-flow{animation:rw-flow 4.8s linear infinite}
.rw-flow-leg{animation:rw-flow-leg 4.8s linear infinite;animation-delay:-2.4s}
.rw-gust{animation:rw-gust 5s ease-in-out infinite}
@keyframes rw-spin{to{transform:rotate(360deg)}}
@keyframes rw-drift{from{transform:translateX(-16px)}to{transform:translateX(16px)}}
@keyframes rw-breathe{0%,100%{transform:scale(1);opacity:.85}50%{transform:scale(1.06);opacity:1}}
@keyframes rw-flow{from{stroke-dashoffset:640}to{stroke-dashoffset:0}}
@keyframes rw-flow-leg{from{stroke-dashoffset:350}to{stroke-dashoffset:0}}
@keyframes rw-gust{0%{transform:translateX(0);opacity:0}12%{opacity:.9}45%{transform:translateX(52px);opacity:0}100%{transform:translateX(52px);opacity:0}}
@media (prefers-reduced-motion:reduce){
.rw-ring1,.rw-ring2,.rw-orbit,.rw-orbit2,.rw-comet1,.rw-comet2,.rw-streak,.rw-glow,.rw-flow,.rw-flow-leg,.rw-gust{animation:none}
.rw-gust{opacity:0}
}
`;

const SloganIcon: React.FC<React.SVGProps<SVGSVGElement>> = (props) => (
  <svg
    viewBox="0 0 560 560"
    xmlns="http://www.w3.org/2000/svg"
    role="img"
    aria-label="RushWind Admin"
    width="100%"
    height="100%"
    {...props}
  >
    <style>{ANIMATION_CSS}</style>
    <defs>
      <linearGradient
        id="rw-slogan-grad"
        gradientUnits="userSpaceOnUse"
        x1="96"
        y1="410"
        x2="470"
        y2="110"
      >
        <stop offset="0" stopColor="#2DD4BF" />
        <stop offset="0.5" stopColor="#38BDF8" />
        <stop offset="1" stopColor="#818CF8" />
      </linearGradient>
      <radialGradient id="rw-slogan-glow" cx="0.5" cy="0.5" r="0.5">
        <stop offset="0" stopColor="#38BDF8" stopOpacity="0.14" />
        <stop offset="1" stopColor="#38BDF8" stopOpacity="0" />
      </radialGradient>
    </defs>

    {/* 呼吸辉光 */}
    <circle className="rw-glow" cx="280" cy="250" r="252" fill="url(#rw-slogan-glow)" />

    {/* 双层对旋虚线环流 */}
    <circle
      className="rw-ring2"
      cx="280"
      cy="250"
      r="246"
      fill="none"
      stroke="#38BDF8"
      strokeOpacity="0.07"
      strokeWidth="1.5"
      strokeDasharray="2 16"
      strokeLinecap="round"
    />
    <circle
      className="rw-ring1"
      cx="280"
      cy="250"
      r="212"
      fill="none"
      stroke="#2DD4BF"
      strokeOpacity="0.14"
      strokeWidth="1.5"
      strokeDasharray="3 13"
      strokeLinecap="round"
    />

    {/* 双彗星弧光：外圈青色正向、内圈靛色反向，绕环巡航 */}
    <circle
      className="rw-comet1"
      cx="280"
      cy="250"
      r="246"
      fill="none"
      stroke="#2DD4BF"
      strokeOpacity="0.8"
      strokeWidth="5"
      strokeLinecap="round"
      strokeDasharray="170 1376"
    />
    <circle
      className="rw-comet2"
      cx="280"
      cy="250"
      r="212"
      fill="none"
      stroke="#818CF8"
      strokeOpacity="0.65"
      strokeWidth="4"
      strokeLinecap="round"
      strokeDasharray="130 1202"
    />

    {/* 轨道光点 */}
    <g className="rw-orbit">
      <circle cx="280" cy="95" r="4.5" fill="#2DD4BF" opacity="0.55" />
      <circle cx="280" cy="405" r="3" fill="#818CF8" opacity="0.45" />
    </g>
    <g className="rw-orbit2">
      <circle cx="435" cy="250" r="3.5" fill="#38BDF8" opacity="0.4" />
    </g>

    {/* 四向漂移风痕 */}
    <g
      className="rw-streak"
      fill="none"
      stroke="#2DD4BF"
      strokeOpacity="0.12"
      strokeWidth="10"
      strokeLinecap="round"
    >
      <path d="M52 190 H140 A20 20 0 0 0 120 156" />
    </g>
    <g
      className="rw-streak rw-s2"
      fill="none"
      stroke="#38BDF8"
      strokeOpacity="0.1"
      strokeWidth="10"
      strokeLinecap="round"
    >
      <path d="M400 126 H494 A22 22 0 0 0 474 88" />
    </g>
    <g
      className="rw-streak rw-s3"
      fill="none"
      stroke="#2DD4BF"
      strokeOpacity="0.12"
      strokeWidth="10"
      strokeLinecap="round"
    >
      <path d="M368 440 H480 A24 24 0 0 0 460 402" />
    </g>
    <g
      className="rw-streak rw-s4"
      fill="none"
      stroke="#38BDF8"
      strokeOpacity="0.09"
      strokeWidth="10"
      strokeLinecap="round"
    >
      <path d="M84 465 H161 A18 18 0 0 0 143 435" />
    </g>

    {/* R 字主标（几何取自 rushwind-icon） */}
    <g transform="translate(280,250) scale(0.95) translate(-256,-257)">
      <g fill="none" stroke="url(#rw-slogan-grad)" strokeLinecap="round">
        <path d="M 100 212 H 136" strokeWidth="14" />
        <path d="M 92 260 H 128" strokeWidth="14" />
        <path d="M 176 172 V 356" strokeWidth="48" />
        <path d="M 176 172 H 256 A 56 56 0 0 1 256 284 H 176" strokeWidth="48" />
        <path
          d="M 190 265 C 300 346 368 360 446 322 C 402 360 296 378 162 303 Z"
          fill="url(#rw-slogan-grad)"
          stroke="none"
        />
        <path d="M 456 330 L 476 324" strokeWidth="8" />
      </g>

      {/* 能量流光：亮脉冲沿笔画巡航（竖笔→字 bowl 一条、斜腿风痕一条，交错出发）。
          每条双层描边（宽淡 + 窄亮）伪发光；纯 CSS stroke-dashoffset，可随 reduced-motion 静止。 */}
      <g fill="none" strokeLinecap="round">
        <path
          className="rw-flow"
          d="M 176 356 V 172 H 256 A 56 56 0 0 1 256 284 H 176"
          stroke="#7DD3FC"
          strokeOpacity="0.28"
          strokeWidth="18"
          strokeDasharray="120 520"
        />
        <path
          className="rw-flow"
          d="M 176 356 V 172 H 256 A 56 56 0 0 1 256 284 H 176"
          stroke="#BAE6FD"
          strokeOpacity="0.95"
          strokeWidth="7"
          strokeDasharray="120 520"
        />
        <path
          className="rw-flow-leg"
          d="M 190 268 C 300 348 368 360 446 322"
          stroke="#7DD3FC"
          strokeOpacity="0.28"
          strokeWidth="14"
          strokeDasharray="80 270"
        />
        <path
          className="rw-flow-leg"
          d="M 190 268 C 300 348 368 360 446 322"
          stroke="#BAE6FD"
          strokeOpacity="0.95"
          strokeWidth="6"
          strokeDasharray="80 270"
        />
      </g>
    </g>

    {/* 尾羽阵风：R 的风痕尾端周期性呼出一组速度线 */}
    <g className="rw-gust" opacity="0" transform="translate(452,296)">
      <path d="M0 0 H54" stroke="#38BDF8" strokeOpacity="0.75" strokeWidth="5" strokeLinecap="round" fill="none" />
      <path d="M-12 18 H34" stroke="#2DD4BF" strokeOpacity="0.55" strokeWidth="4" strokeLinecap="round" fill="none" />
      <path d="M8 -16 H46" stroke="#818CF8" strokeOpacity="0.45" strokeWidth="4" strokeLinecap="round" fill="none" />
    </g>
  </svg>
);

export default SloganIcon;
