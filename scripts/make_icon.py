"""FloatPaste 应用图标生成脚本。

最终设计「双人字疾进」（V56）：
- 深夜蓝三段对角渐变圆角底板 + 左上光斑 + 边缘提亮描边
- 两道人字形色块一浅蓝一白，整体逆时针倾斜 20° 向右上疾进——
  「连续速贴、内容快进上屏」的速度感

渲染分两条路径：

- 32px 及以上：向量几何 + 超采样，质感层（渐变/光斑）齐全。
- 16~48px：同一几何等比绘制（双人字色块占比大、无细笔画，
  超采样 + 预乘降采样即可锐利）；一律免光斑——左上亮斑会把
  主体左缘融进背景，破坏小尺寸的明暗平衡。

用法：python scripts/make_icon.py
输出：
- src-tauri/icons/icon.ico      （16/20/24/28/30/32/36/40/42/48/64 BMP + 256 PNG）
- src-tauri/icons/icon-512.png  （512 高清源图，用于展示与再生成）
- src-tauri/icons/_preview_pngs/preview_sheet.png（多尺寸 + 像素放大预览）

依赖：Pillow、numpy
"""

from __future__ import annotations

import io
import math
import struct
from pathlib import Path

import numpy as np
from PIL import Image, ImageChops, ImageDraw, ImageFilter

REPO = Path(__file__).resolve().parent.parent
ICONS = REPO / "src-tauri" / "icons"
PREVIEW = ICONS / "_preview_pngs"

# ---- 大尺寸向量几何（1024 基准画布） ----
BASE = 1024.0
TILE_RADIUS = 217.6  # viewBox 22 → 22.5%
TILE_STOPS = ((0.0, "#2A3F6B"), (0.55, "#16224A"), (1.0, "#0A1230"))
GLOW_CX, GLOW_CY, GLOW_R = 0.28, 0.16, 0.9
GLOW_COLOR = "#4F8FFF"
GLOW_ALPHA = 0.55
EDGE_ALPHA = 0.22  # 底板边缘提亮描边

# 双人字主体（V56 几何，BASE 尺度）
CHEVRON_PTS = [
    (250.0, 270.0),
    (440.0, 270.0),
    (660.0, 512.0),
    (440.0, 754.0),
    (250.0, 754.0),
    (470.0, 512.0),
]
CHEVRON_ANGLE = -20.0  # y 向下坐标系，负角 = 视觉逆时针，尖端朝右上抬
CHEVRON_BACK_DX = -70.0  # 后翼（浅蓝）相对基准左移
CHEVRON_FRONT_DX = 185.0  # 前翼（白）相对基准右移
CHEVRON_BACK = "#5FA8FF"
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


def _downscale_premultiplied(img: Image.Image, px: int) -> Image.Image:
    """预乘 alpha 后缩放再还原：避免 LANCZOS 把透明区(RGB=0)混入边缘，
    产生圆角暗晕与振铃毛刺。"""
    arr = np.asarray(img).astype(np.float32)
    alpha = arr[..., 3:4] / 255.0
    packed = np.dstack([arr[..., :3] * alpha, arr[..., 3:]]).astype(np.uint8)
    small = np.asarray(
        Image.fromarray(packed, "RGBA").resize((px, px), Image.LANCZOS)
    ).astype(np.float32)
    small[..., 3][small[..., 3] < 8] = 0  # 清除缩放产生的近透明白尘
    al = small[..., 3:4] / 255.0
    rgb = np.where(al > 0, small[..., :3] / np.maximum(al, 1e-6), 0.0)
    out = np.dstack([np.clip(rgb, 0, 255).astype(np.uint8), small[..., 3].astype(np.uint8)])
    return Image.fromarray(out, "RGBA")


def _rot_pts(
    pts: list[tuple[float, float]], deg: float, cx: float, cy: float
) -> list[tuple[float, float]]:
    """绕 (cx, cy) 旋转多边形顶点；y 向下坐标系，正角度为视觉顺时针。"""
    t = math.radians(deg)
    c, s = math.cos(t), math.sin(t)
    return [(cx + dx * c - dy * s, cy + dx * s + dy * c) for x, y in pts for dx, dy in [(x - cx, y - cy)]]


def _draw_chevrons(img: Image.Image) -> None:
    """双人字主体：浅蓝后翼 + 白渐变前翼，整体旋转后向右上疾进。
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
    k = size / BASE
    img = Image.new("RGBA", (size, size), (0, 0, 0, 0))

    # 底板：深夜对角渐变 + 左上光斑
    tile = _multi_stop_gradient(size).convert("RGBA")
    tile.putalpha(_rounded_mask(size, (0, 0, BASE, BASE), TILE_RADIUS))
    img.alpha_composite(tile)

    nx = np.linspace(0.0, 1.0, size, dtype=np.float32)
    gx, gy = np.meshgrid(nx, nx)
    dist = np.sqrt(((gx - GLOW_CX) ** 2 + (gy - GLOW_CY) ** 2) / GLOW_R**2)
    glow_a = (np.clip(1.0 - dist, 0.0, 1.0) * GLOW_ALPHA * 255).astype(np.uint8)
    glow = Image.fromarray(
        np.dstack([np.full((size, size, 3), _rgba(GLOW_COLOR)[:3], dtype=np.uint8), glow_a]),
        "RGBA",
    )
    glow.putalpha(
        ImageChops.multiply(
            glow.getchannel("A"), _rounded_mask(size, (0, 0, BASE, BASE), TILE_RADIUS)
        )
    )
    img.alpha_composite(glow)

    # 底板边缘提亮描边：直接画 outline，避免腐蚀滤波的边界泄漏
    sw = 15.36  # viewBox 1.5 → BASE 尺度
    edge = Image.new("RGBA", (size, size), (0, 0, 0, 0))
    ImageDraw.Draw(edge).rounded_rectangle(
        [sw / 2 * k, sw / 2 * k, (BASE - sw / 2) * k, (BASE - sw / 2) * k],
        radius=(TILE_RADIUS - sw / 2) * k,
        outline=(255, 255, 255, round(EDGE_ALPHA * 255)),
        width=max(1, round(sw * k)),
    )
    img.alpha_composite(edge)

    # 双人字主体：浅蓝后翼 + 白渐变前翼
    _draw_chevrons(img)

    return _downscale_premultiplied(img, px)


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
    """小尺寸规格的超采样画布（未降采样）。16px 档免光斑/投影的规则在此生效，
    托盘等常驻小图标的源图应取本画布，避免把母图的强光斑带进小尺寸造成明暗失衡。"""
    spec = SMALL_SPECS[px]
    size = px * supersample
    img = Image.new("RGBA", (size, size), (0, 0, 0, 0))

    # 底板：深夜渐变 + 亮边。小尺寸一律免光斑——左上亮斑会把主体左缘
    # 融进背景、右缘对比过强，几何居中的内容也会被看成右偏
    tile = _multi_stop_gradient(size).convert("RGBA")
    tile.putalpha(_rounded_mask_px(size, (0, 0, px, px), spec["radius"], px))
    img.alpha_composite(tile)

    # 边缘提亮描边：与底板圆角同心，且用底板 mask 裁剪——
    # 否则圆角弧上描边会凸出底板轮廓，在透明角区漏白
    sw = max(2, round(0.7 * supersample))
    inset = sw / 2
    edge = Image.new("RGBA", (size, size), (0, 0, 0, 0))
    ImageDraw.Draw(edge).rounded_rectangle(
        [inset, inset, size - inset, size - inset],
        radius=max(1.0, spec["radius"] * size / px - inset),
        outline=(255, 255, 255, round(0.22 * 255)),
        width=sw,
    )
    tile_mask = _rounded_mask_px(size, (0, 0, px, px), spec["radius"], px)
    edge.putalpha(ImageChops.multiply(edge.getchannel("A"), tile_mask))
    img.alpha_composite(edge)

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
    return _downscale_premultiplied(render_small_canvas(px, supersample), px)


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
    # 供运行时按 DPI 精确缩放（src-tauri/src/platform/windows/app_icon.rs）
    render_small_canvas(16, supersample=8).save(ICONS / "icon-tray.png")

    sizes = [16, 20, 24, 28, 30, 32, 36, 40, 42, 48, 64, 256]
    images = {px: render_size(px) for px in sizes}
    (ICONS / "icon.ico").write_bytes(build_ico(images))

    _preview(images, master)
    print("icon.ico / icon-512.png / preview 已生成")


if __name__ == "__main__":
    main()
