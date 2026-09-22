#!/usr/bin/env python
"""make-logo.py — 程序化生成侧边栏品牌 logo（overlay/public/logo.png）。

几何与 make-favicon.py 同源（rushwind-icon.svg，一比一 512 视箱）：不用 icon-512
的徽章底——深色圆角徽章在暗色侧边栏里 dark-on-dark 糊成一团；改为无徽章、
亮渐变 R 标直接示人、全幅真透明，深浅主题通吃。笔画用简化加粗形（与 favicon
同款）：侧边栏展示尺寸 32px，进风口/尾迹短线在该尺寸下是游离噪点。

构图与 favicon 不同：侧边栏按定宽定高方框展示，沿用整视箱留白会让字形只占
展示框四成高；故裁到内容包围盒再留 4% 边距（favicon 16px 下需要呼吸空间，
不做此裁切）。几何改动只改 make-favicon.py（单一几何源），本脚本负责输出。
"""
import importlib.util
from pathlib import Path

from PIL import Image

_spec = importlib.util.spec_from_file_location(
    "make_favicon", Path(__file__).with_name("make-favicon.py")
)
_mod = importlib.util.module_from_spec(_spec)
_spec.loader.exec_module(_mod)

SIZE = 200  # 侧边栏展示 32px（@2x 64px），200 留足余量
DARK_BG, LIGHT_BG = (11, 15, 25), (255, 255, 255)  # 与侧边栏 dark/light 底色一致


def main() -> None:
    here = Path(__file__).resolve().parent

    mark = _mod.gradient_rgba(_mod.draw_mark(bold=1.22, full_detail=False))
    x0, y0, x1, y1 = mark.getbbox()
    w, h = x1 - x0, y1 - y0
    pad = int(max(w, h) * 0.04)
    side = max(w, h) + 2 * pad
    square = Image.new("RGBA", (side, side), (0, 0, 0, 0))
    square.paste(mark.crop((x0, y0, x1, y1)), ((side - w) // 2, (side - h) // 2), mark.crop((x0, y0, x1, y1)))
    logo = _mod.downsize(square, SIZE)

    # 三个随仓前端的 overlay 各落一份（字节相同；几何改动后重跑即全量刷新）
    for fe in ("react", "vue-element", "vue-vben"):
        out = here.parent / "overlay" / fe / "public" / "logo.png"
        out.parent.mkdir(parents=True, exist_ok=True)
        logo.save(out, format="PNG")
        print(f"wrote {out}")

    # 预览：上暗下浅两半，每半 = 32px 实际展示尺寸（nearest 放大 6× 便于检查）+ 200px 原图
    zoom = _mod.downsize(square, 32).resize((32 * 6, 32 * 6), Image.NEAREST)
    w, half = 32 * 6 + SIZE + 120, 32 * 6 + 120
    preview = Image.new("RGB", (w, half * 2), DARK_BG)
    preview.paste(Image.new("RGB", (w, half), LIGHT_BG), (0, half))
    for top in (0, half):
        preview.paste(zoom, (30, top + (half - zoom.height) // 2), zoom)
        preview.paste(logo, (32 * 6 + 60, top + (half - logo.height) // 2), logo)
    preview_path = here / "logo-preview.png"
    preview.save(preview_path)
    print(f"preview {preview_path}")


if __name__ == "__main__":
    main()
