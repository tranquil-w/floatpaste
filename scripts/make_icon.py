"""FloatPaste 应用图标生成脚本。

最终设计「深夜底 · 光标贴入浮层」（V13 深色底 + V14 标志融合）：
- 深夜蓝三段对角渐变圆角底板 + 左上光斑 + 边缘提亮描边
- 白色实心渐变面板（柔和投影），面板内亮蓝渐变光标竖线 + 文本条，
  面板下方两道半透明悬浮波——「内容正贴进光标处、面板浮于工作之上」

渲染分两条路径：

- 32px 及以上：向量几何 + 超采样，质感层（渐变/光斑/投影/玻璃）齐全。
- 16/20/24/28px：按目标像素逐点手绘（整数坐标、无重采样）。玻璃质感
  在 1x 下用预混色值表达（底色上叠白的中间色），轮廓保持完整。

用法：python scripts/make_icon.py
输出：
- src-tauri/icons/icon.ico      （16/20/24/28/32/40/48/64 BMP + 256 PNG）
- src-tauri/icons/icon-512.png  （512 高清源图，用于展示与再生成）
- src-tauri/icons/_preview_pngs/preview_sheet.png（多尺寸 + 像素放大预览）

依赖：Pillow、numpy
"""

from __future__ import annotations

import io
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

# 面板与内容（viewBox 100 → ×10.24）
PANEL_BOX = (256.0, 204.8, 768.0, 563.2)
PANEL_RADIUS = 112.6
PANEL_TOP = "#FFFFFF"
PANEL_BOTTOM = "#DCE9FB"
SHADOW_DY = 46.0
SHADOW_BLUR = 46.0
SHADOW_ALPHA = 0.55
SHADOW_COLOR = "#041233"

CARET_BOX = (348.2, 297.0, 394.2, 450.6)  # 光标竖线
TEXT_BOX = (440.3, 327.7, 675.8, 388.9)  # 文本条
LINE_TOP = "#2E8BFF"
LINE_BOTTOM = "#0A5ED7"
TEXT_ALPHA = 0.50

WAVE1 = ((307.2, 696.3), (512.0, 778.2), (716.8, 696.3))  # 二次贝塞尔三控制点（BASE 尺度）
WAVE2 = ((389.1, 839.7), (512.0, 890.9), (634.9, 839.7))
WAVE_COLOR = "#BFD9FF"
WAVE1_W, WAVE2_W = 51.2, 46.1

# ---- 小尺寸专用稿：按比例自动布局（全浮点、严格同心），矢量质感渲染 ----
SMALL_TEXT = "#5D9BE0"  # 文本条：白底上保持可读对比，过浅（如 #A9CBF8）在小尺寸会发虚
SMALL_WAVE = "#8FB7E8"
SMALL_WAVE_DIM = "#5F86C4"


def _small_spec(px: int) -> dict:
    """按面板/底板几何中心比例生成小尺寸布局，保证所有元素严格同心。"""

    def _snap_v(y0: float, y1: float) -> tuple[float, float]:
        # 垂直方向对齐整数像素栅格：位置取整、尺寸取整，半像素框会在
        # 降采样后把一条实线摊成 2~3 行半透明（发虚）
        y = float(round(y0))
        return (y, y + round(y1 - y0))

    tile = float(px)
    radius = tile * 0.22
    panel_w = round(tile * 0.5)
    panel_h = round(tile * 0.34)
    panel_x0 = (tile - panel_w) / 2
    panel_y0 = round(tile * 0.2)
    panel_r = max(1.0, panel_w * 0.094)
    cy = panel_y0 + panel_h / 2

    inset = round(panel_w * 0.18)
    # 线条宽度的下限按"任务栏 0.75x 缩放后仍 ≥2px"反推：源图 3px 缩后 2.25px。
    # 光标竖线 0.17：16px 档 2px、32px 档 3px
    caret_w = max(2.0, round(panel_w * 0.17))
    gap = round(panel_w * 0.09)
    caret_h = round(panel_h * 0.44)
    # 文本条高度 0.28：16px 档 1px，24px 档 2px、32px 档 3px
    text_h = max(1.0, round(panel_h * 0.28))
    text_w = panel_w - 2 * inset - caret_w - gap
    caret_x0 = panel_x0 + inset
    text_x0 = caret_x0 + caret_w + gap
    caret_y0, caret_y1 = _snap_v(cy - caret_h / 2, cy + caret_h / 2)
    text_y0, text_y1 = _snap_v(cy - text_h / 2, cy + text_h / 2)

    wave1_span = round(tile * 0.4)
    wave1_x0 = (tile - wave1_span) / 2
    wave1_y = panel_y0 + panel_h + round(tile * 0.115)
    sag1 = wave1_span * 0.16
    wave2_span = round(wave1_span * 0.72)
    wave2_x0 = (tile - wave2_span) / 2
    wave2_y = wave1_y + round(tile * 0.135)
    sag2 = wave2_span * 0.18

    return dict(
        radius=radius,
        panel=(panel_x0, panel_y0, panel_x0 + panel_w, panel_y0 + panel_h),
        panel_r=panel_r,
        caret=(caret_x0, caret_y0, caret_x0 + caret_w, caret_y1),
        text=(text_x0, text_y0, text_x0 + text_w, text_y1),
        wave1=((wave1_x0, wave1_y), (tile / 2, wave1_y + 2 * sag1), (wave1_x0 + wave1_span, wave1_y)),
        wave2=(
            ((wave2_x0, wave2_y), (tile / 2, wave2_y + 2 * sag2), (wave2_x0 + wave2_span, wave2_y))
            if px >= 20
            else None
        ),
    )


SMALL_SPECS = {px: _small_spec(px) for px in (16, 20, 24, 28, 32, 40, 48)}


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

    # 面板投影：形状下移 + 高斯模糊
    panel_mask = _rounded_mask(size, PANEL_BOX, PANEL_RADIUS)
    shadow_a = (
        Image.fromarray(np.asarray(panel_mask), "L")
        .transform((size, size), Image.AFFINE, (1, 0, 0, 0, 1, -round(SHADOW_DY * k)))
        .filter(ImageFilter.GaussianBlur(radius=SHADOW_BLUR * k))
    )
    shadow = Image.new("RGBA", (size, size), _rgba(SHADOW_COLOR, 1.0))
    shadow.putalpha(shadow_a.point(lambda v: round(v / 255 * SHADOW_ALPHA * 255)))
    img.alpha_composite(shadow)

    # 白色实心渐变面板
    rows = np.linspace(0.0, 1.0, size, dtype=np.float32)
    pt = np.array(_rgba(PANEL_TOP)[:3], dtype=np.float32)
    pb = np.array(_rgba(PANEL_BOTTOM)[:3], dtype=np.float32)
    ramp = pt[None, :] * (1 - rows[:, None]) + pb[None, :] * rows[:, None]
    panel_grad = Image.fromarray(
        np.repeat(ramp[:, None, :], size, axis=1).astype(np.uint8), "RGB"
    ).convert("RGBA")
    panel_grad.putalpha(panel_mask)
    img.alpha_composite(panel_grad)

    # 两道悬浮波：中心线法线偏移成填充多边形（无接缝），两端圆帽
    for ctrl, wv in ((WAVE1, WAVE1_W), (WAVE2, WAVE2_W)):
        alpha = 0.55 if ctrl is WAVE2 else 1.0
        ts = np.linspace(0.0, 1.0, 200)
        cx = (1 - ts) ** 2 * ctrl[0][0] + 2 * (1 - ts) * ts * ctrl[1][0] + ts**2 * ctrl[2][0]
        cy = (1 - ts) ** 2 * ctrl[0][1] + 2 * (1 - ts) * ts * ctrl[1][1] + ts**2 * ctrl[2][1]
        center = np.stack([cx * k, cy * k], axis=1)
        d = np.gradient(center, axis=0)
        norm = np.linalg.norm(d, axis=1, keepdims=True)
        norm[norm == 0] = 1.0
        normal = np.stack([-d[:, 1] / norm[:, 0], d[:, 0] / norm[:, 0]], axis=1)
        hw = wv * k / 2
        poly = np.concatenate([center + normal * hw, (center - normal * hw)[::-1]])
        wave = Image.new("RGBA", (size, size), (0, 0, 0, 0))
        wd = ImageDraw.Draw(wave)
        wd.polygon([tuple(p) for p in poly], fill=_rgba(WAVE_COLOR, alpha))
        for ex, ey in (center[0], center[-1]):
            wd.ellipse([ex - hw, ey - hw, ex + hw, ey + hw], fill=_rgba(WAVE_COLOR, alpha))
        img.alpha_composite(wave)

    # 光标竖线 + 文本条：水平渐变蓝
    lg = np.linspace(0.0, 1.0, size, dtype=np.float32)
    lt = np.array(_rgba(LINE_TOP)[:3], dtype=np.float32)
    lb = np.array(_rgba(LINE_BOTTOM)[:3], dtype=np.float32)
    lramp = lt[None, :] * (1 - lg[:, None]) + lb[None, :] * lg[:, None]
    line_grad = Image.fromarray(np.repeat(lramp[None, :, :], size, axis=0).astype(np.uint8), "RGB")

    for box, alpha in ((CARET_BOX, 1.0), (TEXT_BOX, TEXT_ALPHA)):
        bar = line_grad.convert("RGBA")
        bar.putalpha(
            _rounded_mask(size, box, (box[3] - box[1]) / 2).point(lambda v: round(v * alpha))
        )
        img.alpha_composite(bar)

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


def _h_gradient(size: int, left_hex: str, right_hex: str) -> Image.Image:
    cols = np.linspace(0.0, 1.0, size, dtype=np.float32)
    left = np.array(_rgba(left_hex)[:3], dtype=np.float32)
    right = np.array(_rgba(right_hex)[:3], dtype=np.float32)
    ramp = left[None, :] * (1 - cols[:, None]) + right[None, :] * cols[:, None]
    return Image.fromarray(np.repeat(ramp[None, :, :], size, axis=0).astype(np.uint8), "RGB")


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


def render_small_canvas(px: int, supersample: int = 8) -> Image.Image:
    """小尺寸规格的超采样画布（未降采样）。16px 档免光斑/投影的规则在此生效，
    托盘等常驻小图标的源图应取本画布，避免把母图的强光斑带进小尺寸造成明暗失衡。"""
    spec = SMALL_SPECS[px]
    size = px * supersample
    img = Image.new("RGBA", (size, size), (0, 0, 0, 0))

    # 底板：深夜渐变 + 亮边。小尺寸一律免光斑——左上亮斑会把白色面板左缘
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

    # 面板：硬投影（仅 24px+，16/20px 会与波粘连）+ 白渐变。
    # spec 面板框为几何右开区间，直接使用（此前误 +1 按含端点消费，
    # 导致面板多画一列、左右边距 4:3，内容观感右偏）
    panel_box = tuple(spec["panel"])
    panel_r = spec["panel_r"]
    if px >= 24:
        off = max(1, round(0.05 * px)) * size / px
        shadow_mask = _rounded_mask_px(
            size,
            (panel_box[0], panel_box[1] + off / (size / px), panel_box[2], panel_box[3] + off / (size / px)),
            panel_r,
            px,
        )
        shadow = Image.new("RGBA", (size, size), _rgba(SHADOW_COLOR, 0.5))
        shadow.putalpha(shadow_mask)
        img.alpha_composite(shadow)

    rows = np.linspace(0.0, 1.0, size, dtype=np.float32)
    pt = np.array(_rgba(PANEL_TOP)[:3], dtype=np.float32)
    pb = np.array(_rgba(PANEL_BOTTOM)[:3], dtype=np.float32)
    ramp = pt[None, :] * (1 - rows[:, None]) + pb[None, :] * rows[:, None]
    panel_grad = Image.fromarray(
        np.repeat(ramp[:, None, :], size, axis=1).astype(np.uint8), "RGB"
    ).convert("RGBA")
    panel_grad.putalpha(_rounded_mask_px(size, panel_box, panel_r, px))
    img.alpha_composite(panel_grad)

    # 内容条：光标竖线 + 文本条（渐变蓝 / 浅蓝）
    caret_box, text_box = spec["caret"], spec["text"]
    caret = _h_gradient(size, LINE_TOP, LINE_BOTTOM).convert("RGBA")
    caret.putalpha(_rounded_mask_px(size, caret_box, (caret_box[3] - caret_box[1]) / 2, px))
    img.alpha_composite(caret)
    text = Image.new("RGBA", (size, size), _rgba(SMALL_TEXT))
    text.putalpha(_rounded_mask_px(size, text_box, (text_box[3] - text_box[1]) / 2, px))
    img.alpha_composite(text)

    # 悬浮波：贝塞尔弧线（法线偏移填充，无接缝），两端圆帽，矢高随尺寸放大保证可见
    for key, color in (("wave1", SMALL_WAVE), ("wave2", SMALL_WAVE_DIM)):
        if not spec[key]:
            continue
        (x0, y0), (cx, cy), (x1, y1) = spec[key]
        # 线宽下限 2px：更细的波浪线在降采样后只剩半透明残影
        width = max(2.0, round(0.09 * px))
        _draw_quad_wave(img, (x0, y0), (cx, cy), (x1, y1), width, _rgba(color), px)

    return img


def draw_small(px: int, supersample: int = 8) -> Image.Image:
    """小尺寸专用稿：沿用小尺寸布局（元素占比大、锐利），渲染走向量质感
    （渐变底板 + 光斑 + 硬投影 + 渐变内容条），保证与大图同一气质。"""
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

    sizes = [16, 20, 24, 28, 32, 40, 48, 64, 256]
    images = {px: render_size(px) for px in sizes}
    (ICONS / "icon.ico").write_bytes(build_ico(images))

    _preview(images, master)
    print("icon.ico / icon-512.png / preview 已生成")


if __name__ == "__main__":
    main()
