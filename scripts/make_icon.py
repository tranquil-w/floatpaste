"""FloatPaste 应用图标生成脚本。

最终设计「双人字疾进 · 雾蓝」（V57b）：
- 去饱和蓝三段对角渐变圆角底板 + 灰一白双色块，整体逆时针倾斜 20°
  向右上疾进——「连续速贴、内容快进上屏」的速度感
- 全尺寸免光斑：标题栏其余元素（横条/窗控）均为细线级，图标是
  顶栏唯一实心块，再叠光影会与窗口扁平语言打架

渲染分两条路径：

- 32px 及以上：向量几何 + 超采样。
- 16~48px：同一几何等比绘制（双人字色块占比大、无细笔画，
  超采样 + 预乘降采样即可锐利）。

用法：python scripts/make_icon.py
输出：
- crates/floatpaste-native/assets/icon.ico（16/20/24/28/30/32/36/40/42/48/64 BMP + 256 PNG）
- crates/floatpaste-native/assets/icon-512.png（512 高清源图，用于展示与再生成）
- crates/floatpaste-native/ui/app-icon.png（512 母图副本，Slint Window.icon 与关于页用）
- crates/floatpaste-native/assets/_preview_pngs/preview_sheet.png（多尺寸 + 像素放大预览）

标题栏 16px 图标不走本脚本位图：软件渲染在非整倍 DPI 下重采样
发虚，用 ui/common.slint 的 AppMark 矢量版（几何色值与本脚本
常量同构，变更须双向同步）。

依赖：Pillow、numpy
"""

from __future__ import annotations

import io
import math
import struct
from pathlib import Path

import numpy as np
from PIL import Image, ImageDraw

REPO = Path(__file__).resolve().parent.parent
ICONS = REPO / "crates" / "floatpaste-native" / "assets"
PREVIEW = ICONS / "_preview_pngs"
UI = REPO / "crates" / "floatpaste-native" / "ui"

# ---- 大尺寸向量几何（1024 基准画布） ----
BASE = 1024.0
TILE_RADIUS = 217.6  # viewBox 22 → 22.5%
TILE_STOPS = ((0.0, "#3D5178"), (0.55, "#242F52"), (1.0, "#111830"))

# 双人字主体（V57 几何，BASE 尺度）
CHEVRON_PTS = [
    (250.0, 270.0),
    (440.0, 270.0),
    (660.0, 512.0),
    (440.0, 754.0),
    (250.0, 754.0),
    (470.0, 512.0),
]
CHEVRON_ANGLE = -20.0  # y 向下坐标系，负角 = 视觉逆时针，尖端朝右上抬
CHEVRON_BACK_DX = -70.0  # 后翼（灰）相对基准左移
CHEVRON_FRONT_DX = 185.0  # 前翼（白）相对基准右移
CHEVRON_BACK = "#A9C6E8"
CHEVRON_FRONT_TOP = "#FFFFFF"
CHEVRON_FRONT_BOTTOM = "#DCE9FB"

# ---- 小尺寸专用稿：与母图同几何，仅保留底板圆角半径 ----


def _small_spec(px: int) -> dict:
    tile = float(px)
    return dict(radius=tile * 0.22)


# 30/36/42 档对应 125%/150%/175% 缩放下任务栏大图标的精确绘制尺寸
# （SM_CXICON × 3/4），供 app_icon.rs 的 LoadImageW 1:1 命中
SMALL_SPECS = {px: _small_spec(px) for px in (16, 20, 24, 28, 30, 32, 36, 40, 42, 48)}


def _rgba(hex_color: str, alpha: float = 1.0) -> tuple[int, int, int, int]:
    h = hex_color.lstrip("#")
    return (int(h[0:2], 16), int(h[2:4], 16), int(h[4:6], 16), round(alpha * 255))


def _vertical_gradient(size: int, top_hex: str, bottom_hex: str) -> Image.Image:
    rows = np.linspace(0.0, 1.0, size, dtype=np.float32)
    top = np.array(_rgba(top_hex)[:3], dtype=np.float32)
    bottom = np.array(_rgba(bottom_hex)[:3], dtype=np.float32)
    ramp = top[None, :] * (1 - rows[:, None]) + bottom[None, :] * rows[:, None]
    data = np.repeat(ramp[:, None, :], size, axis=1).astype(np.uint8)
    return Image.fromarray(data, "RGB")


def _multi_stop_gradient(size: int) -> Image.Image:
    """对角三段渐变底板（左上→右下）。"""
    axis = np.linspace(0.0, 1.0, size, dtype=np.float32)
    xx, yy = np.meshgrid(axis, axis)
    t = ((xx + yy) / 2.0).clip(0.0, 1.0)
    stops = [(p, np.array(_rgba(h)[:3], dtype=np.float32)) for p, h in TILE_STOPS]
    data = np.zeros((size, size, 3), dtype=np.float32)
    for i in range(len(stops) - 1):
        p0, c0 = stops[i]
        p1, c1 = stops[i + 1]
        seg = (t >= p0) & (t <= p1)
        k = np.zeros_like(t)
        k[seg] = (t[seg] - p0) / (p1 - p0)
        data[seg] = c0[None, :] * (1 - k[seg])[:, None] + c1[None, :] * k[seg][:, None]
    return Image.fromarray(data.astype(np.uint8), "RGB")


def _rounded_mask(size: int, box: tuple[float, float, float, float], radius: float) -> Image.Image:
    k = size / BASE
    mask = Image.new("L", (size, size), 0)
    ImageDraw.Draw(mask).rounded_rectangle([v * k for v in box], radius=radius * k, fill=255)
    return mask


def _downscale(img: Image.Image, px: int) -> Image.Image:
    """LANCZOS 降采样。Pillow 对 RGBA 缩放内部自带预乘/反预乘，透明区
    不会污染边缘；切勿再手动预乘——那会让反预乘执行两次，低 alpha 处
    除法放大量化噪声成白色（圆角白边毛刺）。极低 alpha 处 Pillow 反预乘
    仍有量化白尘，<8 置零兜底。"""
    small = img.resize((px, px), Image.LANCZOS)
    arr = np.asarray(small).copy()
    arr[..., 3][arr[..., 3] < 8] = 0
    return Image.fromarray(arr, "RGBA")


def _rot_pts(
    pts: list[tuple[float, float]], deg: float, cx: float, cy: float
) -> list[tuple[float, float]]:
    """绕 (cx, cy) 旋转多边形顶点；y 向下坐标系，正角度为视觉顺时针。"""
    t = math.radians(deg)
    c, s = math.cos(t), math.sin(t)
    return [(cx + dx * c - dy * s, cy + dx * s + dy * c) for x, y in pts for dx, dy in [(x - cx, y - cy)]]


def _draw_chevrons(img: Image.Image) -> None:
    """双人字主体：灰后翼 + 白渐变前翼，整体旋转后向右上疾进。
    在任意尺寸画布上按 BASE 坐标等比绘制，几何全局一致。"""
    size = img.width
    k = size / BASE
    back = _rot_pts([(x + CHEVRON_BACK_DX, y) for x, y in CHEVRON_PTS], CHEVRON_ANGLE, 512.0, 512.0)
    front = _rot_pts(
        [(x + CHEVRON_FRONT_DX, y) for x, y in CHEVRON_PTS], CHEVRON_ANGLE, 512.0, 512.0
    )

    layer = Image.new("RGBA", img.size, (0, 0, 0, 0))
    ImageDraw.Draw(layer).polygon([(x * k, y * k) for x, y in back], fill=_rgba(CHEVRON_BACK))
    img.alpha_composite(layer)

    grad = _vertical_gradient(size, CHEVRON_FRONT_TOP, CHEVRON_FRONT_BOTTOM).convert("RGBA")
    mask = Image.new("L", img.size, 0)
    ImageDraw.Draw(mask).polygon([(x * k, y * k) for x, y in front], fill=255)
    grad.putalpha(mask)
    img.alpha_composite(grad)


def draw_icon(px: int, supersample: int = 4) -> Image.Image:
    """大尺寸路径：向量几何在超采样画布上绘制后重采样到目标尺寸。"""
    size = px * supersample
    img = Image.new("RGBA", (size, size), (0, 0, 0, 0))

    # 底板：中性近黑对角渐变（免光斑，见模块注释）
    tile = _multi_stop_gradient(size).convert("RGBA")
    tile.putalpha(_rounded_mask(size, (0, 0, BASE, BASE), TILE_RADIUS))
    img.alpha_composite(tile)

    # 双人字主体：灰后翼 + 白渐变前翼
    _draw_chevrons(img)

    return _downscale(img, px)


def _rounded_mask_px(size: int, box: tuple[float, float, float, float], radius: float, px: int) -> Image.Image:
    """px 空间坐标的圆角 mask。box 为几何右开区间 [x0, x1)，覆盖列 x0..x1-1；
    PIL rounded_rectangle 含端点，故右/下各减 1px 再缩放。"""
    k = size / px
    mask = Image.new("L", (size, size), 0)
    ImageDraw.Draw(mask).rounded_rectangle(
        [box[0] * k, box[1] * k, box[2] * k - 1, box[3] * k - 1],
        radius=radius * k,
        fill=255,
    )
    return mask


def render_small_canvas(px: int, supersample: int = 8) -> Image.Image:
    """小尺寸规格的超采样画布（未降采样）。托盘等常驻小图标的源图
    应取本画布。全尺寸免光斑（见模块注释），大小路径质感层一致。"""
    spec = SMALL_SPECS[px]
    size = px * supersample
    img = Image.new("RGBA", (size, size), (0, 0, 0, 0))

    # 底板：中性近黑渐变
    tile = _multi_stop_gradient(size).convert("RGBA")
    tile.putalpha(_rounded_mask_px(size, (0, 0, px, px), spec["radius"], px))
    img.alpha_composite(tile)

    # 双人字主体：与母图同一几何（色块占比大、无细笔画，等比即可锐利）
    _draw_chevrons(img)

    return img


def _draw_quad_wave(
    img: Image.Image,
    p0: tuple[float, float],
    pc: tuple[float, float],
    p1: tuple[float, float],
    width: float,
    color: tuple[int, int, int, int],
    px: int,
) -> None:
    """px 空间的二次贝塞尔粗弧线：中心线法线偏移成填充多边形，两端圆帽。"""
    size = img.width
    k = size / px
    ts = np.linspace(0.0, 1.0, 200)
    mx = (1 - ts) ** 2 * p0[0] + 2 * (1 - ts) * ts * pc[0] + ts**2 * p1[0]
    my = (1 - ts) ** 2 * p0[1] + 2 * (1 - ts) * ts * pc[1] + ts**2 * p1[1]
    center = np.stack([mx * k, my * k], axis=1)
    d = np.gradient(center, axis=0)
    norm = np.linalg.norm(d, axis=1, keepdims=True)
    norm[norm == 0] = 1.0
    normal = np.stack([-d[:, 1] / norm[:, 0], d[:, 0] / norm[:, 0]], axis=1)
    hw = width * k / 2
    poly = np.concatenate([center + normal * hw, (center - normal * hw)[::-1]])
    layer = Image.new("RGBA", (size, size), (0, 0, 0, 0))
    ld = ImageDraw.Draw(layer)
    ld.polygon([tuple(p) for p in poly], fill=color)
    for ex, ey in (center[0], center[-1]):
        ld.ellipse([ex - hw, ey - hw, ex + hw, ey + hw], fill=color)
    img.alpha_composite(layer)


def draw_small(px: int, supersample: int = 8) -> Image.Image:
    """小尺寸路径：与母图同一几何，8x 超采样 + 预乘降采样保证锐利。"""
    return _downscale(render_small_canvas(px, supersample), px)


def render_size(px: int) -> Image.Image:
    if px in SMALL_SPECS:
        return draw_small(px)
    return draw_icon(px, supersample=4)


def build_ico(images: dict[int, Image.Image]) -> bytes:
    """小于等于 64 的尺寸用 BMP 条目（含 AND 掩码），256 用 PNG 条目。"""
    entries: list[tuple[int, bytes]] = []
    # 32px 必须是第一个条目：tauri-codegen 取 entries()[0] 作为 default_window_icon
    # （托盘图标源），高 DPI 托盘显示 24~32px，16px 条目被放大会糊
    order = sorted(images, key=lambda px: (0 if px == 32 else 1, px))
    for px in order:
        im = images[px]
        if px <= 64:
            buf = io.BytesIO()
            arr = np.asarray(im.convert("RGBA"), dtype=np.uint8)
            h, w = arr.shape[:2]
            bgra = arr[..., [2, 1, 0, 3]]
            xor = b"".join(row.tobytes() for row in bgra[::-1])
            and_stride = ((w + 31) // 32) * 4
            and_mask = b"\x00" * (and_stride * h)
            # BITMAPINFOHEADER: size, width, height(双倍含 AND 掩码), planes,
            # bpp, compression, image_size, x_ppm, y_ppm, colors_used, colors_important
            header = struct.pack("<IiiHHIIiiII", 40, w, h * 2, 1, 32, 0, 0, 0, 0, 0, 0)
            buf.write(header + xor + and_mask)
            entries.append((px, buf.getvalue()))
        else:
            buf = io.BytesIO()
            im.save(buf, format="PNG")
            entries.append((px, buf.getvalue()))

    count = len(entries)
    out = io.BytesIO()
    out.write(struct.pack("<HHH", 0, 1, count))
    offset = 6 + 16 * count
    for px, data in entries:
        w = 0 if px >= 256 else px
        h = 0 if px >= 256 else px
        out.write(struct.pack("<BBBBHHII", w, h, 0, 0, 1, 32, len(data), offset))
        offset += len(data)
    for _px, data in entries:
        out.write(data)
    return out.getvalue()


def _preview(images: dict[int, Image.Image], master: Image.Image) -> None:
    w_total, h_total = 880, 560
    sheet = Image.new("RGB", (w_total, h_total), "#F3F5F8")
    d = ImageDraw.Draw(sheet)
    d.rectangle([w_total // 2, 0, w_total, h_total], fill="#1B1F27")

    big = master.resize((200, 200), Image.LANCZOS)
    sheet.paste(big, (30, 30), big)
    sheet.paste(big, (w_total // 2 + 30, 30), big)
    d.text((30, 244), "512 master", fill="#555")

    for x0 in (30, w_total // 2 + 30):
        x = x0
        for px in (16, 20, 24, 28, 32, 40, 48, 64):
            im = images[px]
            sheet.paste(im, (x, 330 - px), im)
            x += px + 14
        d.text((x0, 342), "16 / 20 / 24 / 28 / 32 / 40 / 48 / 64", fill="#888")

    x = w_total // 2 + 30
    for px, factor in ((16, 8), (24, 4), (32, 4)):
        z = images[px].resize((px * factor, px * factor), Image.NEAREST)
        sheet.paste(z, (x, 376), z)
        d.text((x, 376 + px * factor + 8), f"{px}px x{factor}", fill="#888")
        x += px * factor + 16

    sheet.save(PREVIEW / "preview_sheet.png")


def main() -> None:
    PREVIEW.mkdir(parents=True, exist_ok=True)

    master = draw_icon(512, supersample=4)
    master.save(ICONS / "icon-512.png")

    # 托盘源图：16px 小尺寸规格的 8x 超采样画布（无光斑/投影），
    # 供运行时按 DPI 精确缩放（floatpaste-native/src/app_icon.rs）
    render_small_canvas(16, supersample=8).save(ICONS / "icon-tray.png")

    sizes = [16, 20, 24, 28, 30, 32, 36, 40, 42, 48, 64, 256]
    images = {px: render_size(px) for px in sizes}
    (ICONS / "icon.ico").write_bytes(build_ico(images))

    # 512 母图副本：Slint Window.icon（编辑/设置窗）与关于页 48px 展示
    master.save(UI / "app-icon.png")

    _preview(images, master)
    print("icon.ico / icon-512.png / app-icon.png / preview 已生成")


if __name__ == "__main__":
    main()
