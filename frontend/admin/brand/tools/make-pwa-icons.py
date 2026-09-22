#!/usr/bin/env python
"""make-pwa-icons.py — 程序化生成 vue-vben PWA 图标（overlay/vue-vben/public/pwa-icon-{192,512}.png）。

几何与 make-favicon.py 同源（rushwind-icon.svg，一比一 512 视箱）。vben 的 PWA
manifest 引用这两个尺寸（internal/vite-config/src/options.ts），构图与侧边栏
logo 相同：裁到内容包围盒 + 4% 边距、全幅真透明。几何改动只改 make-favicon.py。
"""
import importlib.util
from pathlib import Path

from PIL import Image

_spec = importlib.util.spec_from_file_location(
    "make_favicon", Path(__file__).with_name("make-favicon.py")
)
_mod = importlib.util.module_from_spec(_spec)
_spec.loader.exec_module(_mod)

SIZES = (192, 512)


def main() -> None:
    here = Path(__file__).resolve().parent
    out_dir = here.parent / "overlay" / "vue-vben" / "public"
    out_dir.mkdir(parents=True, exist_ok=True)

    mark = _mod.gradient_rgba(_mod.draw_mark(bold=1.22, full_detail=False))
    x0, y0, x1, y1 = mark.getbbox()
    w, h = x1 - x0, y1 - y0
    pad = int(max(w, h) * 0.04)
    side = max(w, h) + 2 * pad
    square = Image.new("RGBA", (side, side), (0, 0, 0, 0))
    square.paste(mark.crop((x0, y0, x1, y1)), ((side - w) // 2, (side - h) // 2), mark.crop((x0, y0, x1, y1)))

    for size in SIZES:
        icon = _mod.downsize(square, size)
        out = out_dir / f"pwa-icon-{size}.png"
        icon.save(out, format="PNG")
        print(f"wrote {out}")


if __name__ == "__main__":
    main()
