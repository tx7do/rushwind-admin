#!/usr/bin/env python
"""make-favicon.py — 从 rushwind-icon.svg 的几何数据程序化生成 favicon.ico。

为什么不用上游预built的 ico：
  1. 徽章深蓝底在深色标签栏里糊成一团（dark-on-dark）；
  2. R 的细笔画（进风口、尾迹）缩到 16px 变噪点；
  3. 圆角徽章四角透明度不干净，渲染出白色杂边。

方案：去掉徽章底，亮渐变 R 标直接示人（深/浅标签栏通吃）；
  48px = 完整形（含进风口与尾迹）；16/32px = 简化加粗形（仅主干笔画）。
全帧 RGBA 真透明。几何与 rushwind-icon.svg 一比一（512 视箱，×4 超采样）。
"""
from pathlib import Path

import numpy as np
from PIL import Image, ImageDraw

S = 4                      # 超采样倍率（512 → 2048）
CANVAS = 512 * S
TEAL, SKY, INDIGO = (45, 212, 191), (56, 189, 248), (129, 140, 248)
# 品牌渐变轴：userSpaceOnUse (96,410) → (470,110)，×S
A = np.array([96 * S, 410 * S], dtype=float)
B = np.array([470 * S, 110 * S], dtype=float)
DIR = B - A


def sample_cubic(p0, p1, p2, p3, n=48):
    """贝塞尔采样（结果按超采样系数 S 缩放到画布坐标）。"""
    ts = np.linspace(0.0, 1.0, n)
    return [
        (
            ((1 - t) ** 3 * p0[0] + 3 * (1 - t) ** 2 * t * p1[0] + 3 * (1 - t) * t**2 * p2[0] + t**3 * p3[0]) * S,
            ((1 - t) ** 3 * p0[1] + 3 * (1 - t) ** 2 * t * p1[1] + 3 * (1 - t) * t**2 * p2[1] + t**3 * p3[1]) * S,
        )
        for t in ts
    ]


def draw_mark(bold: float = 1.0, full_detail: bool = True) -> Image.Image:
    """在透明画布上绘制 R 标 mask（L 模式）。bold 为笔画加粗系数（小尺寸用）。"""
    img = Image.new("L", (CANVAS, CANVAS), 0)
    d = ImageDraw.Draw(img)

    def line(p, q, w):
        d.line([tuple(v * S for v in p), tuple(v * S for v in q)], fill=255, width=int(w * S))
        r = w * S / 2
        for c in (p, q):
            d.ellipse([c[0] * S - r, c[1] * S - r, c[0] * S + r, c[1] * S + r], fill=255)

    def dot(c, r):
        d.ellipse([c[0] * S - r * S, c[1] * S - r * S, c[0] * S + r * S, c[1] * S + r * S], fill=255)

    W = lambda w: int(w * bold)

    # 竖笔 + 字 bowl（笔画 48；四角与末端补圆头，模拟 strokeLinecap/join round）
    line((176, 172), (176, 356), W(48))
    line((176, 172), (256, 172), W(48))
    line((256, 284), (176, 284), W(48))
    d.arc([200 * S, 172 * S, 312 * S, 284 * S], start=-90, end=90, fill=255, width=W(48) * S)
    for c in ((176, 172), (256, 172), (256, 284), (176, 284), (176, 356)):
        dot(c, W(48) / 2)

    # 斜腿风痕（填充形体，两条三次贝塞尔围合）
    top = sample_cubic((190, 265), (300, 346), (368, 360), (446, 322))
    bottom = sample_cubic((446, 322), (402, 360), (296, 378), (162, 303))
    d.polygon(top + bottom, fill=255)

    if full_detail:
        # 进风口双短线（14）与尾迹（8）——16px 下是噪点，仅完整形绘制
        line((100, 212), (136, 212), W(14))
        line((92, 260), (128, 260), W(14))
        line((456, 330), (476, 324), W(8))
    return img


def gradient_rgba(mask: Image.Image) -> Image.Image:
    """把品牌三停渐变按 (96,410)→(470,110) 轴投影到 mask 上。"""
    ys, xs = np.nonzero(np.asarray(mask))
    pts = np.stack([xs, ys], axis=1).astype(float)
    t = (pts - A) @ DIR / (DIR @ DIR)
    t = np.clip(t, 0.0, 1.0)
    rgb = np.zeros((t.shape[0], 3), dtype=np.uint8)
    lo = t < 0.5
    tt = (t[lo] * 2)[:, None]
    rgb[lo] = (np.array(TEAL) * (1 - tt) + np.array(SKY) * tt).astype(np.uint8)
    tt = ((t[~lo] - 0.5) * 2)[:, None]
    rgb[~lo] = (np.array(SKY) * (1 - tt) + np.array(INDIGO) * tt).astype(np.uint8)

    out = np.zeros((CANVAS, CANVAS, 4), dtype=np.uint8)
    out[ys, xs, :3] = rgb
    out[ys, xs, 3] = np.asarray(mask)[ys, xs]
    return Image.fromarray(out, "RGBA")


def downsize(img: Image.Image, size: int) -> Image.Image:
    return img.resize((size, size), Image.LANCZOS)


def main() -> None:
    here = Path(__file__).resolve().parent

    full = gradient_rgba(draw_mark(bold=1.0, full_detail=True))
    bold = gradient_rgba(draw_mark(bold=1.22, full_detail=False))

    # 三档统一用简化加粗形：进风口/尾迹短线离主体远，在 48px 下也像游离脏点
    f48 = downsize(bold, 48)
    f32 = downsize(bold, 32)
    f16 = downsize(bold, 16)

    # 三个随仓前端的 overlay 各落一份（字节相同；几何改动后重跑即全量刷新）
    out = None
    for fe in ("react", "vue-element", "vue-vben"):
        out = here.parent / "overlay" / fe / "public" / "favicon.ico"
        out.parent.mkdir(parents=True, exist_ok=True)
        f48.save(out, format="ICO", append_images=[f32, f16], sizes=[(48, 48), (32, 32), (16, 16)])
        print(f"wrote {out}")

    # 预览：深/浅两种标签栏底色上并排看 48/32/16（nearest 放大 6× 便于检查）
    frames = [f48, f32, f16]
    cell = 48 * 6
    preview = Image.new("RGB", (cell * 3 * 2 + 60, cell), (13, 17, 23))
    light = Image.new("RGB", (cell * 3, cell), (251, 251, 251))
    preview.paste(light, (cell * 3 + 60, 0))
    for i, f in enumerate(frames):
        big = f.resize((cell, cell), Image.NEAREST)
        preview.paste(big, (i * cell, 0), big)
        preview.paste(big, (cell * 3 + 60 + i * cell, 0), big)
    preview_path = here / "favicon-preview.png"
    preview.save(preview_path)
    print(f"preview {preview_path}")

    # 回读校验帧
    with Image.open(out) as ico:
        print("ico frames:", sorted(ico.info.get("sizes", [])))


if __name__ == "__main__":
    main()
