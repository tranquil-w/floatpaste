"""图标重设计候选变体生成：图形 + 色块，无细笔画。

在深夜蓝渐变底板上探索 6 个实心色块构图，渲染 256/48/32/24/16 全档位，
输出对比图 src-tauri/icons/_preview_pngs/variants_sheet.png 供选择。

用法：python scripts/icon_variants.py
依赖：Pillow、numpy（并复用 make_icon.py 的底板与降采样函数）
"""

from __future__ import annotations

import math
from pathlib import Path

import numpy as np
from PIL import Image, ImageChops, ImageDraw, ImageFilter, ImageFont

import make_icon as mi

REPO = Path(__file__).resolve().parent.parent
PREVIEW = REPO / "src-tauri" / "icons" / "_preview_pngs"

WHITE_TOP, WHITE_BOTTOM = "#FFFFFF", "#DCE9FB"
BLUE_TOP, BLUE_BOTTOM = "#2E8BFF", "#0A5ED7"
MID_BLUE, LIGHT_BLUE, PALE_BLUE = "#5B87D6", "#8FB7E8", "#BFD9FF"

# 经典鼠标光标轮廓（局部坐标，比例约 0.62:1）
CURSOR_POLYGON = [
    (0.0, 0.0), (0.0, 350.0), (98.0, 266.0), (154.0, 406.0),
    (196.0, 392.0), (146.0, 252.0), (252.0, 252.0),
]


def _v_gradient(size: int, top: str, bottom: str) -> Image.Image:
    rows = np.linspace(0.0, 1.0, size, dtype=np.float32)
    t = np.array(mi._rgba(top)[:3], dtype=np.float32)
    b = np.array(mi._rgba(bottom)[:3], dtype=np.float32)
    ramp = t[None, :] * (1 - rows[:, None]) + b[None, :] * rows[:, None]
    return Image.fromarray(np.repeat(ramp[:, None, :], size, axis=1).astype(np.uint8), "RGB")


def _solid(img: Image.Image, box: tuple[float, float, float, float], radius: float, color: str, k: float, alpha: float = 1.0) -> None:
    layer = Image.new("RGBA", img.size, mi._rgba(color, alpha))
    layer.putalpha(mi._rounded_mask(img.size[0], box, radius))
    img.alpha_composite(layer)


def _grad_box(img: Image.Image, box: tuple[float, float, float, float], radius: float, top: str, bottom: str, horizontal: bool = False) -> None:
    size = img.size[0]
    cols = np.linspace(0.0, 1.0, size, dtype=np.float32)
    t = np.array(mi._rgba(top)[:3], dtype=np.float32)
    b = np.array(mi._rgba(bottom)[:3], dtype=np.float32)
    ramp = t[None, :] * (1 - cols[:, None]) + b[None, :] * cols[:, None]
    grad = Image.fromarray(np.repeat(ramp[None, :, :], size, axis=0).astype(np.uint8), "RGB").convert("RGBA")
    if not horizontal:
        grad = grad.transpose(Image.ROTATE_90)
    grad.putalpha(mi._rounded_mask(size, box, radius))
    img.alpha_composite(grad)


def _polygon(img: Image.Image, pts: list[tuple[float, float]], color, k: float) -> None:
    layer = Image.new("RGBA", img.size, (0, 0, 0, 0))
    fill = mi._rgba(color) if isinstance(color, str) else color
    ImageDraw.Draw(layer).polygon([(x * k, y * k) for x, y in pts], fill=fill)
    img.alpha_composite(layer)


def _rot_pts(pts: list[tuple[float, float]], deg: float, cx: float, cy: float) -> list[tuple[float, float]]:
    """绕 (cx, cy) 旋转多边形顶点；正角度为视觉顺时针（y 轴向下坐标系）。"""
    t = math.radians(deg)
    c, s = math.cos(t), math.sin(t)
    out = []
    for x, y in pts:
        dx, dy = x - cx, y - cy
        out.append((cx + dx * c - dy * s, cy + dx * s + dy * c))
    return out


# ---- V1 双层浮卡：前后两张圆角卡，浮层堆叠 ----
def v1(img: Image.Image, size: int, k: float) -> None:
    _solid(img, (330, 235, 850, 700), 90, MID_BLUE, k)
    _grad_box(img, (174, 324, 694, 789), 90, WHITE_TOP, WHITE_BOTTOM)


# ---- V2 光标浮标：实心光标箭头 + 右下偏移的蓝色浮层 ----
def v2(img: Image.Image, size: int, k: float) -> None:
    scale = 1.22
    x0, y0 = 336.0, 236.0
    pts = [(x0 + x * scale, y0 + y * scale) for x, y in CURSOR_POLYGON]
    back = [(x + 118.0, y + 134.0) for x, y in pts]
    _polygon(img, back, MID_BLUE, k)
    _polygon(img, pts, "#F4F8FF", k)


# ---- V3 速贴纸飞机：白 + 双蓝切面 ----
def v3(img: Image.Image, size: int, k: float) -> None:
    _polygon(img, [(480, 640), (900, 240), (660, 830)], LIGHT_BLUE, k)
    _polygon(img, [(545, 866), (480, 640), (660, 830)], MID_BLUE, k)
    _polygon(img, [(150, 460), (900, 240), (480, 640)], "#F4F8FF", k)


# ---- V4 剪贴板：实心板 + 顶部夹子 + 光标块 ----
def v4(img: Image.Image, size: int, k: float) -> None:
    _grad_box(img, (270, 200, 754, 862), 80, WHITE_TOP, WHITE_BOTTOM)
    _solid(img, (408, 136, 616, 266), 64, MID_BLUE, k)
    _grad_box(img, (427, 430, 597, 706), 56, BLUE_TOP, BLUE_BOTTOM)


# ---- V5 折角便签：切角卡片 + 折角 + 光标块 ----
def v5(img: Image.Image, size: int, k: float) -> None:
    card = Image.new("RGBA", img.size, (0, 0, 0, 0))
    mask = Image.new("L", img.size, 0)
    ImageDraw.Draw(mask).polygon(
        [(222 * k, 270 * k), (640 * k, 270 * k), (806 * k, 436 * k), (806 * k, 844 * k), (222 * k, 844 * k)],
        fill=255,
    )
    grad = _v_gradient(img.size[0], WHITE_TOP, WHITE_BOTTOM).convert("RGBA")
    card = grad.copy()
    card.putalpha(mask)
    img.alpha_composite(card)
    _polygon(img, [(640, 270), (806, 436), (640, 436)], LIGHT_BLUE, k)
    _grad_box(img, (352, 500, 564, 720), 40, BLUE_TOP, BLUE_BOTTOM)


# ---- V6 速贴气泡：圆角气泡 + 尾角 + 光标块 ----
def v6(img: Image.Image, size: int, k: float) -> None:
    _polygon(img, [(352, 656), (524, 656), (352, 856)], "#F4F8FF", k)
    _grad_box(img, (230, 246, 818, 688), 110, WHITE_TOP, WHITE_BOTTOM)
    _grad_box(img, (389, 352, 659, 582), 56, BLUE_TOP, BLUE_BOTTOM)


# ---- V7 疾飞光标：光标朝右上 45°，左下两道平行拖尾切面 ----
def v7(img: Image.Image, size: int, k: float) -> None:
    cx, cy = 590.0, 470.0
    raw = [(cx + (x - 126) * 1.16, cy + (y - 203) * 1.16) for x, y in CURSOR_POLYGON]
    back = [(x - 70, y + 80) for x, y in raw]
    _polygon(img, [(453, 753), (613, 593), (507, 487), (347, 647)], LIGHT_BLUE, k)
    _polygon(img, [(313, 893), (433, 773), (327, 667), (207, 787)], MID_BLUE, k)
    _polygon(img, _rot_pts(back, 45, cx, cy), MID_BLUE, k)
    _polygon(img, _rot_pts(raw, 45, cx, cy), "#F4F8FF", k)


# ---- V8 飞出浮卡：斜置圆角卡片 + 卡后平行蓝切面，卡片中心带内容块 ----
def v8(img: Image.Image, size: int, k: float) -> None:
    back = Image.new("RGBA", img.size, (0, 0, 0, 0))
    _solid(back, (300, 420, 700, 790), 84, MID_BLUE, k)
    img.alpha_composite(back.rotate(18, resample=Image.BICUBIC, center=(size / 2, size / 2)))

    card = Image.new("RGBA", img.size, (0, 0, 0, 0))
    _grad_box(card, (320, 330, 720, 700), 84, WHITE_TOP, WHITE_BOTTOM)
    _grad_box(card, (430, 440, 610, 590), 48, BLUE_TOP, BLUE_BOTTOM)
    img.alpha_composite(card.rotate(18, resample=Image.BICUBIC, center=(size / 2, size / 2)))


# ---- V9 粘贴箭镞：粗壮圆头省略的实心箭头指向右上 + 双拖尾 ----
def v9(img: Image.Image, size: int, k: float) -> None:
    arrow = [(512, 232), (700, 420), (598, 420), (598, 792), (426, 792), (426, 420), (324, 420)]
    back = [(x - 62, y + 62) for x, y in arrow]
    _polygon(img, [(383, 743), (263, 863), (157, 757), (277, 637)], LIGHT_BLUE, k)
    _polygon(img, [(239, 879), (149, 969), (51, 871), (141, 781)], MID_BLUE, k)
    _polygon(img, _rot_pts(back, 45, 512, 512), MID_BLUE, k)
    _polygon(img, _rot_pts(arrow, 45, 512, 512), "#F4F8FF", k)


# ---- V10 双三角疾驰：前白后蓝两只同向三角，快进式速度感 ----
def v10(img: Image.Image, size: int, k: float) -> None:
    _polygon(img, [(270, 320), (600, 560), (270, 800)], LIGHT_BLUE, k)
    _polygon(img, [(600, 320), (930, 560), (600, 800)], "#F4F8FF", k)


# ---- V11 折角飞贴：折角便签斜置飞行 + 左下拖尾切面 ----
def v11(img: Image.Image, size: int, k: float) -> None:
    _polygon(img, [(333, 813), (233, 913), (127, 807), (227, 707)], MID_BLUE, k)

    layer = Image.new("RGBA", img.size, (0, 0, 0, 0))
    mask = Image.new("L", img.size, 0)
    card_pts = [(320, 330), (700, 330), (860, 490), (860, 760), (320, 760)]
    ImageDraw.Draw(mask).polygon([(x * k, y * k) for x, y in card_pts], fill=255)
    grad = _v_gradient(img.size[0], WHITE_TOP, WHITE_BOTTOM).convert("RGBA")
    layer.alpha_composite(grad)
    layer.putalpha(mask)
    ld = ImageDraw.Draw(layer)
    ld.polygon(
        [(700 * k, 330 * k), (860 * k, 490 * k), (700 * k, 490 * k)], fill=mi._rgba(LIGHT_BLUE)
    )
    _grad_box(layer, (420, 540, 600, 680), 44, BLUE_TOP, BLUE_BOTTOM)
    img.alpha_composite(layer.rotate(15, resample=Image.BICUBIC, center=(size / 2, size / 2)))


# ---- V12 斜切浮层：三条 45° 斜切色带阶梯排布，纯构成无具象 ----
def v12(img: Image.Image, size: int, k: float) -> None:
    _polygon(img, [(280, 540), (440, 700), (240, 900), (80, 740)], MID_BLUE, k)
    _polygon(img, [(430, 340), (630, 540), (310, 860), (110, 660)], LIGHT_BLUE, k)
    _polygon(img, [(620, 150), (820, 350), (500, 670), (300, 470)], "#F4F8FF", k)


# ---- 第三轮：柔和线条。全部由圆头曲线 / 圆 / 胶囊构成，零尖角 ----

def _wave(img: Image.Image, p0, pc, p1, width: float, color: str, k: float) -> None:
    """BASE 坐标的圆头粗曲线（复用 make_icon 的贝塞尔法线偏移实现）。"""
    mi._draw_quad_wave(img, p0, pc, p1, width, mi._rgba(color), 1024)


# ---- V13 彗星飞贴：白色圆角卡片为彗星头，两道圆头弯尾 ----
def v13(img: Image.Image, size: int, k: float) -> None:
    _wave(img, (250, 830), (420, 720), (620, 560), 120, LIGHT_BLUE, k)
    _wave(img, (180, 700), (280, 640), (420, 560), 90, MID_BLUE, k)
    card = Image.new("RGBA", img.size, (0, 0, 0, 0))
    _grad_box(card, (540, 300, 880, 640), 96, WHITE_TOP, WHITE_BOTTOM)
    _grad_box(card, (630, 390, 790, 550), 56, BLUE_TOP, BLUE_BOTTOM)
    img.alpha_composite(card.rotate(-12, resample=Image.BICUBIC, center=(size / 2, size / 2)))


# ---- V14 悬浮双波：上方内容胶囊 + 两道粗圆头悬浮波托底 ----
def v14(img: Image.Image, size: int, k: float) -> None:
    _solid(img, (392, 220, 632, 352), 66, "#F4F8FF", k)
    _wave(img, (180, 520), (512, 690), (844, 520), 150, "#F4F8FF", k)
    _wave(img, (300, 760), (512, 870), (724, 760), 120, LIGHT_BLUE, k)


# ---- V15 胶囊流风：三道圆头斜胶囊渐次变短，柔和的速度线 ----
def v15(img: Image.Image, size: int, k: float) -> None:
    bars = [(520, 380, "#F4F8FF"), (400, 560, LIGHT_BLUE), (300, 740, MID_BLUE)]
    for length, cy, color in bars:
        layer = Image.new("RGBA", img.size, (0, 0, 0, 0))
        _solid(layer, (512 - length / 2, cy - 75, 512 + length / 2, cy + 75), 75, color, k)
        img.alpha_composite(layer.rotate(45, resample=Image.BICUBIC, center=(size / 2, size / 2)))


# ---- V16 弯月滑翔：白色弦月凹口朝左下，凹处一颗蓝色圆点 ----
def v16(img: Image.Image, size: int, k: float) -> None:
    mask = Image.new("L", img.size, 0)
    d = ImageDraw.Draw(mask)
    d.ellipse([(512 - 250) * k, (450 - 250) * k, (512 + 250) * k, (450 + 250) * k], fill=255)
    d.ellipse([(420 - 250) * k, (545 - 250) * k, (420 + 250) * k, (545 + 250) * k], fill=0)
    layer = _v_gradient(img.size[0], WHITE_TOP, WHITE_BOTTOM).convert("RGBA")
    layer.putalpha(mask)
    img.alpha_composite(layer)
    dot = Image.new("RGBA", img.size, (0, 0, 0, 0))
    ImageDraw.Draw(dot).ellipse(
        [(430 - 72) * k, (575 - 72) * k, (430 + 72) * k, (575 + 72) * k], fill=mi._rgba(MID_BLUE)
    )
    img.alpha_composite(dot)


# ---- V17 弧托圆：白色大圆被两道上拱圆头弧托起，「浮起」的最简表达 ----
def v17(img: Image.Image, size: int, k: float) -> None:
    _wave(img, (200, 760), (512, 560), (824, 760), 140, LIGHT_BLUE, k)
    _wave(img, (300, 900), (512, 780), (724, 900), 110, MID_BLUE, k)
    dot = Image.new("RGBA", img.size, (0, 0, 0, 0))
    ImageDraw.Draw(dot).ellipse(
        [(512 - 230) * k, (420 - 230) * k, (512 + 230) * k, (420 + 230) * k],
        fill=mi._rgba("#F4F8FF"),
    )
    img.alpha_composite(dot)


# ---- V18 绸带 S 流：一道白色 S 形粗绸带 + 浅蓝伴带，飘动的传输感 ----
def v18(img: Image.Image, size: int, k: float) -> None:
    segs = [
        ((240, 320), (640, 320), (512, 512)),
        ((512, 512), (384, 704), (784, 704)),
    ]
    for dx, dy, color in ((64, 84, LIGHT_BLUE), (0, 0, "#F4F8FF")):
        for p0, pc, p1 in segs:
            _wave(
                img,
                (p0[0] + dx, p0[1] + dy),
                (pc[0] + dx, pc[1] + dy),
                (p1[0] + dx, p1[1] + dy),
                130,
                color,
                k,
            )


# ---- 第四轮：沿 V15/V18 的「圆头线条流动」语言继续发散 ----

def _arc_band(img: Image.Image, cx: float, cy: float, r_out: float, r_in: float, deg0: float, deg1: float, color: str, k: float) -> None:
    """BASE 坐标的圆弧带（两端圆头）。角度为 y 轴向下的屏幕角度。"""
    ts = np.linspace(math.radians(deg0), math.radians(deg1), 160)
    mid_r, cap = (r_out + r_in) / 2, (r_out - r_in) / 2
    outer = np.stack([cx + r_out * np.cos(ts), cy + r_out * np.sin(ts)], axis=1)
    inner = np.stack([cx + r_in * np.cos(ts), cy + r_in * np.sin(ts)], axis=1)[::-1]
    layer = Image.new("RGBA", img.size, (0, 0, 0, 0))
    d = ImageDraw.Draw(layer)
    d.polygon([tuple(p) for p in np.concatenate([outer, inner]) * k], fill=mi._rgba(color))
    for ang in (ts[0], ts[-1]):
        ex, ey = cx + mid_r * math.cos(ang), cy + mid_r * math.sin(ang)
        d.ellipse(
            [(ex - cap) * k, (ey - cap) * k, (ex + cap) * k, (ey + cap) * k], fill=mi._rgba(color)
        )
    img.alpha_composite(layer)


# ---- V19 流风三叠：三道平行的微波浪线斜向排列（V15 的直杠揉出起伏） ----
def v19(img: Image.Image, size: int, k: float) -> None:
    layer = Image.new("RGBA", img.size, (0, 0, 0, 0))
    for ln, cy, sag, color in [(560, 340, 150, "#F4F8FF"), (430, 550, 130, LIGHT_BLUE), (310, 750, 110, MID_BLUE)]:
        _wave(layer, (512 - ln / 2, cy - 60), (512, cy + sag), (512 + ln / 2, cy - 60), 105, color, k)
    img.alpha_composite(layer.rotate(45, resample=Image.BICUBIC, center=(size / 2, size / 2)))


# ---- V20 回环一笔：290° 开口圆环（口朝右上）+ 环心蓝点，循环往复 ----
def v20(img: Image.Image, size: int, k: float) -> None:
    _arc_band(img, 512, 512, 270, 128, -10, 280, "#F4F8FF", k)
    dot = Image.new("RGBA", img.size, (0, 0, 0, 0))
    ImageDraw.Draw(dot).ellipse(
        [(512 - 86) * k, (512 - 86) * k, (512 + 86) * k, (512 + 86) * k], fill=mi._rgba(MID_BLUE)
    )
    img.alpha_composite(dot)


# ---- V21 柔光闪弧：闪电折线圆头化 + 浅蓝错影，快的语义、柔的笔触 ----
def v21(img: Image.Image, size: int, k: float) -> None:
    segs = [((620, 240), (500, 390), (430, 530)), ((430, 530), (520, 560), (350, 800))]
    for dx, dy, color in ((60, 70, LIGHT_BLUE), (0, 0, "#F4F8FF")):
        for p0, pc, p1 in segs:
            _wave(
                img,
                (p0[0] + dx, p0[1] + dy),
                (pc[0] + dx, pc[1] + dy),
                (p1[0] + dx, p1[1] + dy),
                125,
                color,
                k,
            )


# ---- V22 括弧聚点：两道左凸圆头弧 + 一颗蓝点，聚焦/收拢 ----
def v22(img: Image.Image, size: int, k: float) -> None:
    _wave(img, (470, 280), (230, 512), (470, 744), 135, "#F4F8FF", k)
    _wave(img, (640, 330), (450, 512), (640, 694), 100, LIGHT_BLUE, k)
    dot = Image.new("RGBA", img.size, (0, 0, 0, 0))
    ImageDraw.Draw(dot).ellipse(
        [(690 - 92) * k, (512 - 92) * k, (690 + 92) * k, (512 + 92) * k], fill=mi._rgba(MID_BLUE)
    )
    img.alpha_composite(dot)


# ---- V23 光点拖影：彗星的纯线条版——大圆点领头，三道渐短拖尾斜列 ----
def v23(img: Image.Image, size: int, k: float) -> None:
    layer = Image.new("RGBA", img.size, (0, 0, 0, 0))
    d = ImageDraw.Draw(layer)
    d.ellipse([(512 - 100) * k, (220 - 100) * k, (512 + 100) * k, (220 + 100) * k], fill=mi._rgba("#F4F8FF"))
    for ln, cy, wd, color in [(330, 420, 150, "#F4F8FF"), (230, 600, 140, LIGHT_BLUE), (150, 760, 120, MID_BLUE)]:
        _solid(layer, (512 - ln / 2, cy - wd / 2, 512 + ln / 2, cy + wd / 2), wd / 2, color, k)
    img.alpha_composite(layer.rotate(45, resample=Image.BICUBIC, center=(size / 2, size / 2)))


# ---- V24 卷曲飘带：一道 S 绸带末端向内卷起（V18 更进一步） ----
def v24(img: Image.Image, size: int, k: float) -> None:
    segs = [
        ((280, 280), (660, 280), (660, 470)),
        ((660, 470), (660, 660), (430, 660)),
        ((430, 660), (300, 660), (300, 520)),
    ]
    for dx, dy, color in ((64, 84, LIGHT_BLUE), (0, 0, "#F4F8FF")):
        for p0, pc, p1 in segs:
            _wave(
                img,
                (p0[0] + dx, p0[1] + dy),
                (pc[0] + dx, pc[1] + dy),
                (p1[0] + dx, p1[1] + dy),
                130,
                color,
                k,
            )


# ---- 第五轮：柔和线条 × 功能语义（速贴浮窗 / 历史堆叠 / 剪贴板 / 粘贴落点）----

# ---- V25 速贴浮窗：白色圆角面板 + 三条圆头条目行（速贴面板的缩影）----
def v25(img: Image.Image, size: int, k: float) -> None:
    _grad_box(img, (280, 250, 744, 782), 100, WHITE_TOP, WHITE_BOTTOM)
    _grad_box(img, (360, 340, 664, 432), 46, BLUE_TOP, BLUE_BOTTOM)
    _grad_box(img, (360, 486, 616, 566), 40, "#6FA3E8", "#4E82D0")
    _grad_box(img, (360, 632, 556, 700), 34, LIGHT_BLUE, MID_BLUE)


# ---- V26 历史叠层：三张圆角卡沿对角错位（剪贴板历史的层叠）+ 顶卡内容条 ----
def v26(img: Image.Image, size: int, k: float) -> None:
    _solid(img, (352, 264, 852, 688), 92, MID_BLUE, k)
    _solid(img, (312, 322, 812, 746), 92, LIGHT_BLUE, k)
    _grad_box(img, (272, 380, 772, 804), 92, WHITE_TOP, WHITE_BOTTOM)
    _grad_box(img, (352, 480, 590, 600), 60, BLUE_TOP, BLUE_BOTTOM)


# ---- V27 圆头剪贴板：圆角板身 + 顶部夹子 + 两条圆头内容 ----
def v27(img: Image.Image, size: int, k: float) -> None:
    _grad_box(img, (320, 250, 704, 806), 88, WHITE_TOP, WHITE_BOTTOM)
    _solid(img, (424, 186, 600, 310), 62, MID_BLUE, k)
    _grad_box(img, (396, 420, 628, 512), 46, BLUE_TOP, BLUE_BOTTOM)
    _grad_box(img, (396, 568, 560, 636), 34, LIGHT_BLUE, MID_BLUE)


# ---- V28 落点浮窗：条目面板 + 右下蓝色圆点（粘贴落点）----
def v28(img: Image.Image, size: int, k: float) -> None:
    _grad_box(img, (260, 240, 700, 740), 96, WHITE_TOP, WHITE_BOTTOM)
    _grad_box(img, (336, 356, 624, 448), 46, BLUE_TOP, BLUE_BOTTOM)
    _grad_box(img, (336, 504, 560, 580), 38, LIGHT_BLUE, MID_BLUE)
    dot = Image.new("RGBA", img.size, (0, 0, 0, 0))
    ImageDraw.Draw(dot).ellipse(
        [(672 - 92) * k, (708 - 92) * k, (672 + 92) * k, (708 + 92) * k], fill=mi._rgba(BLUE_TOP)
    )
    img.alpha_composite(dot)


# ---- V29 浮卡双波：简洁圆角卡 + 内容块，下方两道圆头悬浮波（原构图的柔和简化）----
def v29(img: Image.Image, size: int, k: float) -> None:
    _grad_box(img, (300, 240, 724, 600), 84, WHITE_TOP, WHITE_BOTTOM)
    _grad_box(img, (376, 352, 556, 468), 50, BLUE_TOP, BLUE_BOTTOM)
    _wave(img, (250, 720), (512, 850), (774, 720), 105, "#F4F8FF", k)
    _wave(img, (330, 900), (512, 990), (694, 900), 85, LIGHT_BLUE, k)


# ---- V30 时光序列：圆头竖干线 + 三颗渐大蓝点（历史由旧到新）----
def v30(img: Image.Image, size: int, k: float) -> None:
    _solid(img, (472, 280, 552, 784), 40, "#F4F8FF", k)
    for cy, r, color in [(360, 56, LIGHT_BLUE), (532, 72, "#6FA3E8"), (716, 92, MID_BLUE)]:
        dot = Image.new("RGBA", img.size, (0, 0, 0, 0))
        ImageDraw.Draw(dot).ellipse(
            [(512 - r) * k, (cy - r) * k, (512 + r) * k, (cy + r) * k], fill=mi._rgba(color)
        )
        img.alpha_composite(dot)


# ---- 第六轮：「贴」的意象——翘角贴纸 / 出血浮窗 / 斜置叠层，主体带动感 ----

def _tile_mask(size: int) -> Image.Image:
    return mi._rounded_mask(size, (0, 0, mi.BASE, mi.BASE), mi.TILE_RADIUS)


def _sticker(target: Image.Image, k: float, box=(300, 240, 840, 800), cut=(620, 240, 840, 460),
             fold_c=(660, 440), bars=((380, 520, 760, 624, 52), (380, 676, 600, 752, 38))) -> None:
    """带右上翘角的贴纸：白渐变卡（切角）+ 浅色折面 + 内容圆头条。"""
    x0, y0, x1, y1 = box
    m = Image.new("L", target.size, 0)
    d = ImageDraw.Draw(m)
    d.rounded_rectangle([x0 * k, y0 * k, x1 * k, y1 * k], radius=96 * k, fill=255)
    d.polygon([(cut[0] * k, (cut[1] - 2) * k), ((cut[2] + 2) * k, cut[3] * k), ((cut[2] + 2) * k, (cut[1] - 2) * k)], fill=0)
    layer = _v_gradient(target.size[0], WHITE_TOP, WHITE_BOTTOM).convert("RGBA")
    layer.putalpha(m)
    target.alpha_composite(layer)
    fold = Image.new("RGBA", target.size, (0, 0, 0, 0))
    ImageDraw.Draw(fold).polygon(
        [(cut[0] * k, cut[1] * k), (cut[2] * k, cut[3] * k), (fold_c[0] * k, fold_c[1] * k)],
        fill=mi._rgba(PALE_BLUE),
    )
    target.alpha_composite(fold)
    for bx0, by0, bx1, by1, br in bars:
        _grad_box(target, (bx0, by0, bx1, by1), br, BLUE_TOP, BLUE_BOTTOM)


# ---- V31 翘角贴纸：一张贴在深夜底板上的贴纸，右上角微微翘起 ----
def v31(img: Image.Image, size: int, k: float) -> None:
    _sticker(img, k)


# ---- V32 双贴纸：底张蓝色斜置，顶张白贴纸带翘角与内容条 ----
def v32(img: Image.Image, size: int, k: float) -> None:
    back = Image.new("RGBA", img.size, (0, 0, 0, 0))
    _solid(back, (310, 280, 790, 760), 92, MID_BLUE, k)
    img.alpha_composite(back.rotate(-10, resample=Image.BICUBIC, center=(size / 2, size / 2)))
    front = Image.new("RGBA", img.size, (0, 0, 0, 0))
    _sticker(front, k, box=(280, 250, 740, 710), cut=(560, 250, 740, 430), fold_c=(586, 404),
             bars=((360, 450, 660, 546, 48), (360, 598, 560, 672, 37)))
    img.alpha_composite(front.rotate(6, resample=Image.BICUBIC, center=(size / 2, size / 2)))


# ---- V33 出血浮窗：斜置面板一角浮出底板（被底板轮廓裁回）----
def v33(img: Image.Image, size: int, k: float) -> None:
    layer = Image.new("RGBA", img.size, (0, 0, 0, 0))
    _grad_box(layer, (200, 260, 860, 820), 100, WHITE_TOP, WHITE_BOTTOM)
    _grad_box(layer, (300, 400, 760, 510), 55, BLUE_TOP, BLUE_BOTTOM)
    _grad_box(layer, (300, 570, 640, 660), 45, "#6FA3E8", "#4E82D0")
    layer = layer.rotate(12, resample=Image.BICUBIC, center=(size / 2, size / 2))
    layer.putalpha(ImageChops.multiply(layer.getchannel("A"), _tile_mask(size)))
    img.alpha_composite(layer)


# ---- V34 旋叠历史：历史叠层整体倾斜 12°，堆叠被赋予浮起的方向感 ----
def v34(img: Image.Image, size: int, k: float) -> None:
    layer = Image.new("RGBA", img.size, (0, 0, 0, 0))
    _solid(layer, (352, 264, 852, 688), 92, MID_BLUE, k)
    _solid(layer, (312, 322, 812, 746), 92, LIGHT_BLUE, k)
    _grad_box(layer, (272, 380, 772, 804), 92, WHITE_TOP, WHITE_BOTTOM)
    _grad_box(layer, (352, 480, 590, 600), 60, BLUE_TOP, BLUE_BOTTOM)
    layer = layer.rotate(12, resample=Image.BICUBIC, center=(size / 2, size / 2))
    layer.putalpha(ImageChops.multiply(layer.getchannel("A"), _tile_mask(size)))
    img.alpha_composite(layer)


# ---- V35 大揭角：贴纸被大幅揭起，折面内加一道圆头卷曲线 ----
def v35(img: Image.Image, size: int, k: float) -> None:
    m = Image.new("L", img.size, 0)
    d = ImageDraw.Draw(m)
    d.rounded_rectangle([300 * k, 260 * k, 840 * k, 800 * k], radius=96 * k, fill=255)
    d.polygon([(498 * k, 258 * k), (842 * k, 602 * k), (842 * k, 258 * k)], fill=0)
    layer = _v_gradient(img.size[0], WHITE_TOP, WHITE_BOTTOM).convert("RGBA")
    layer.putalpha(m)
    img.alpha_composite(layer)
    fold = Image.new("RGBA", img.size, (0, 0, 0, 0))
    ImageDraw.Draw(fold).polygon(
        [(500 * k, 260 * k), (840 * k, 600 * k), (536 * k, 584 * k)], fill=mi._rgba(PALE_BLUE)
    )
    img.alpha_composite(fold)
    _wave(img, (560, 300), (640, 430), (780, 560), 56, MID_BLUE, k)
    _grad_box(img, (380, 660, 720, 744), 42, BLUE_TOP, BLUE_BOTTOM)


# ---- V36 斜贴纸落点：贴纸斜置 + 右下蓝色圆点（贴到光标处）----
def v36(img: Image.Image, size: int, k: float) -> None:
    layer = Image.new("RGBA", img.size, (0, 0, 0, 0))
    _sticker(layer, k, box=(290, 230, 780, 720), cut=(580, 230, 780, 430), fold_c=(612, 396),
             bars=((370, 470, 700, 566, 48), (370, 618, 590, 692, 37)))
    img.alpha_composite(layer.rotate(-9, resample=Image.BICUBIC, center=(size / 2, size / 2)))
    dot = Image.new("RGBA", img.size, (0, 0, 0, 0))
    ImageDraw.Draw(dot).ellipse(
        [(700 - 88) * k, (726 - 88) * k, (700 + 88) * k, (726 + 88) * k], fill=mi._rgba(BLUE_TOP)
    )
    img.alpha_composite(dot)


# ---- 第七轮：功能动作视角——快捷键唤起 / 应用间搬运 / 捕获吸附 / 循环复用 ----

# ---- V37 键帽速贴：立体键帽 + 键面圆头闪电（全局快捷键，一按即贴）----
def v37(img: Image.Image, size: int, k: float) -> None:
    _solid(img, (280, 330, 744, 794), 100, MID_BLUE, k)
    _grad_box(img, (280, 250, 744, 714), 100, "#F4F8FF", "#DCE9FB")
    for p0, pc, p1 in [((560, 330), (470, 450), (420, 540)), ((420, 540), (480, 560), (380, 690))]:
        _wave(img, p0, pc, p1, 92, BLUE_TOP, k)


# ---- V38 传递弧线：源点抛物弧线飞向目标大圆（从这复制，到那粘贴）----
def v38(img: Image.Image, size: int, k: float) -> None:
    _wave(img, (350, 670), (450, 440), (590, 470), 85, LIGHT_BLUE, k)
    fly = Image.new("RGBA", img.size, (0, 0, 0, 0))
    _grad_box(fly, (440, 440, 540, 540), 40, BLUE_TOP, BLUE_BOTTOM)
    img.alpha_composite(fly)
    dot = Image.new("RGBA", img.size, (0, 0, 0, 0))
    ImageDraw.Draw(dot).ellipse(
        [(300 - 90) * k, (730 - 90) * k, (300 + 90) * k, (730 + 90) * k], fill=mi._rgba(MID_BLUE)
    )
    img.alpha_composite(dot)
    m = Image.new("L", img.size, 0)
    ImageDraw.Draw(m).ellipse(
        [(660 - 170) * k, (380 - 170) * k, (660 + 170) * k, (380 + 170) * k], fill=255
    )
    layer = _v_gradient(img.size[0], WHITE_TOP, WHITE_BOTTOM).convert("RGBA")
    layer.putalpha(m)
    img.alpha_composite(layer)


# ---- V39 双窗桥接：两个应用窗口之间一道圆弧桥（跨应用搬运内容）----
def v39(img: Image.Image, size: int, k: float) -> None:
    _wave(img, (480, 360), (660, 400), (700, 540), 80, LIGHT_BLUE, k)
    _grad_box(img, (250, 270, 510, 530), 64, WHITE_TOP, WHITE_BOTTOM)
    _grad_box(img, (316, 360, 444, 428), 34, BLUE_TOP, BLUE_BOTTOM)
    _solid(img, (514, 490, 774, 750), 64, MID_BLUE, k)
    _solid(img, (580, 580, 708, 648), 34, LIGHT_BLUE, k)


# ---- V40 磁吸内容：U 形圆头磁体 + 被吸附的蓝色内容块（剪贴板捕获）----
def v40(img: Image.Image, size: int, k: float) -> None:
    _solid(img, (300, 300, 420, 620), 60, "#F4F8FF", k)
    _solid(img, (604, 300, 724, 620), 60, "#F4F8FF", k)
    _arc_band(img, 512, 620, 212, 92, 0, 180, "#F4F8FF", k)
    _grad_box(img, (432, 176, 592, 336), 48, BLUE_TOP, BLUE_BOTTOM)


# ---- V41 键帽浮波：圆点键帽悬浮于两道速度波上（按下快捷键、浮出面板）----
def v41(img: Image.Image, size: int, k: float) -> None:
    _solid(img, (306, 300, 718, 620), 88, MID_BLUE, k)
    _grad_box(img, (306, 240, 718, 560), 88, "#F4F8FF", "#DCE9FB")
    dot = Image.new("RGBA", img.size, (0, 0, 0, 0))
    ImageDraw.Draw(dot).ellipse(
        [(512 - 72) * k, (400 - 72) * k, (512 + 72) * k, (400 + 72) * k], fill=mi._rgba(MID_BLUE)
    )
    img.alpha_composite(dot)
    _wave(img, (250, 750), (512, 860), (774, 750), 100, "#F4F8FF", k)
    _wave(img, (330, 900), (512, 980), (694, 900), 84, LIGHT_BLUE, k)


# ---- V42 循环节拍：开口环环住一颗内容方块（剪贴板循环复用）----
def v42(img: Image.Image, size: int, k: float) -> None:
    _arc_band(img, 512, 512, 268, 132, -10, 280, "#F4F8FF", k)
    _grad_box(img, (432, 432, 592, 592), 44, BLUE_TOP, BLUE_BOTTOM)


# ---- 第八轮：立体悬浮——斜视浮板带厚度与投影，表达「浮」字本身 ----

def _tilt(layer: Image.Image, shear: float = 0.38) -> Image.Image:
    """顶边后仰的斜切变换（input_x = x + shear·(y − h)），平面卡变斜视立体卡。"""
    h = layer.size[1]
    return layer.transform(
        layer.size, Image.AFFINE, (1, shear, -shear * h, 0, 1, 0), resample=Image.BICUBIC
    )


def _ground_shadow(img: Image.Image, k: float, box=(250, 880, 770, 966), alpha=0.5) -> None:
    m = Image.new("L", img.size, 0)
    ImageDraw.Draw(m).ellipse([box[0] * k, box[1] * k, box[2] * k, box[3] * k], fill=255)
    m = m.filter(ImageFilter.GaussianBlur(18 * k))
    shadow = Image.new("RGBA", img.size, mi._rgba("#041233", 1.0))
    shadow.putalpha(m.point(lambda v: round(v * alpha)))
    img.alpha_composite(shadow)


def _float_panel(target: Image.Image, k: float) -> None:
    """斜视浮板：暗色厚度层 + 白渐变顶面 + 两条内容条。"""
    _solid(target, (200, 470, 640, 820), 88, MID_BLUE, k)
    _grad_box(target, (170, 430, 610, 780), 88, WHITE_TOP, WHITE_BOTTOM)
    _grad_box(target, (240, 530, 480, 600), 35, BLUE_TOP, BLUE_BOTTOM)
    _grad_box(target, (240, 650, 420, 706), 28, "#6FA3E8", "#4E82D0")


# ---- V43 斜视浮板：带厚度的面板悬浮于地面投影之上 ----
def v43(img: Image.Image, size: int, k: float) -> None:
    _ground_shadow(img, k)
    layer = Image.new("RGBA", img.size, (0, 0, 0, 0))
    _float_panel(layer, k)
    img.alpha_composite(_tilt(layer))


# ---- V44 双浮层：大小两块浮板上下错位悬浮（浮层堆叠的立体版）----
def v44(img: Image.Image, size: int, k: float) -> None:
    _ground_shadow(img, k, box=(230, 900, 810, 980))
    layer = Image.new("RGBA", img.size, (0, 0, 0, 0))
    _solid(layer, (250, 560, 770, 880), 92, MID_BLUE, k)
    _grad_box(layer, (210, 500, 730, 820), 92, WHITE_TOP, WHITE_BOTTOM)
    _grad_box(layer, (290, 620, 560, 690), 35, BLUE_TOP, BLUE_BOTTOM)
    _solid(layer, (420, 380, 760, 560), 70, MID_BLUE, k)
    _grad_box(layer, (390, 330, 730, 510), 70, "#F4F8FF", "#DCE9FB")
    _grad_box(layer, (460, 400, 620, 452), 26, BLUE_TOP, BLUE_BOTTOM)
    layer = _tilt(layer, 0.34)
    layer.putalpha(ImageChops.multiply(layer.getchannel("A"), _tile_mask(layer.size[0])))
    img.alpha_composite(layer)


# ---- V45 浮板落点：斜视浮板 + 一颗蓝色圆点光标落到板角 ----
def v45(img: Image.Image, size: int, k: float) -> None:
    _ground_shadow(img, k)
    layer = Image.new("RGBA", img.size, (0, 0, 0, 0))
    _float_panel(layer, k)
    img.alpha_composite(_tilt(layer))
    dot = Image.new("RGBA", img.size, (0, 0, 0, 0))
    ImageDraw.Draw(dot).ellipse(
        [(800 - 74) * k, (420 - 74) * k, (800 + 74) * k, (420 + 74) * k], fill=mi._rgba(BLUE_TOP)
    )
    img.alpha_composite(dot)


# ---- V46 浮板踏波：斜视浮板踩在两道圆头波上（浮于内容之上）----
def v46(img: Image.Image, size: int, k: float) -> None:
    _wave(img, (260, 830), (512, 930), (764, 830), 96, "#F4F8FF", k)
    _wave(img, (340, 960), (512, 1020), (684, 960), 80, LIGHT_BLUE, k)
    layer = Image.new("RGBA", img.size, (0, 0, 0, 0))
    _float_panel(layer, k)
    img.alpha_composite(_tilt(layer))


# ---- V47 云梯浮层：三块小板沿对角阶梯上浮（多条目逐级悬浮）----
def v47(img: Image.Image, size: int, k: float) -> None:
    _ground_shadow(img, k, box=(280, 910, 800, 986))
    layer = Image.new("RGBA", img.size, (0, 0, 0, 0))
    for i, color in [(0, MID_BLUE), (1, LIGHT_BLUE)]:
        x, y = 180 + i * 150, 640 - i * 190
        _solid(layer, (x, y, x + 360, y + 130), 60, color, k)
    x, y = 480, 260
    _grad_box(layer, (x, y, x + 360, y + 130), 60, "#F4F8FF", "#DCE9FB")
    _grad_box(layer, (x + 60, y + 36, x + 250, y + 92), 28, BLUE_TOP, BLUE_BOTTOM)
    layer = _tilt(layer, 0.32)
    layer.putalpha(ImageChops.multiply(layer.getchannel("A"), _tile_mask(layer.size[0])))
    img.alpha_composite(layer)


# ---- V48 斜视剪贴板：立体化的板夹，顶部夹子与内容条随板后仰 ----
def v48(img: Image.Image, size: int, k: float) -> None:
    _ground_shadow(img, k)
    layer = Image.new("RGBA", img.size, (0, 0, 0, 0))
    _solid(layer, (230, 500, 630, 850), 84, MID_BLUE, k)
    _grad_box(layer, (200, 460, 600, 810), 84, WHITE_TOP, WHITE_BOTTOM)
    _solid(layer, (330, 390, 470, 490), 50, MID_BLUE, k)
    _grad_box(layer, (266, 580, 480, 652), 36, BLUE_TOP, BLUE_BOTTOM)
    _grad_box(layer, (266, 700, 430, 756), 28, "#6FA3E8", "#4E82D0")
    img.alpha_composite(_tilt(layer))


# ---- 第九轮：字母 F（FloatPaste 首字母）——融合 V3 斜向切面与 V15/V18 圆头线条 ----

def _f_shape(target: Image.Image, k: float, top: str, bottom: str,
             dx: float = 0.0, dy: float = 0.0) -> None:
    """正置圆头 F 三笔（全局 y 渐变保证笔画衔接处颜色连续）。"""
    _grad_box(target, (330 + dx, 210 + dy, 484 + dx, 812 + dy), 52, top, bottom)
    _grad_box(target, (330 + dx, 210 + dy, 716 + dx, 366 + dy), 52, top, bottom)
    _grad_box(target, (330 + dx, 456 + dy, 646 + dx, 590 + dy), 46, top, bottom)


# ---- V49 斜冲 F：双色切面 F 向右上冲刺，右下叠一层蓝阶背板 ----
def v49(img: Image.Image, size: int, k: float) -> None:
    dark = Image.new("RGBA", img.size, (0, 0, 0, 0))
    _f_shape(dark, k, "#9CC4F2", "#6E9BDD", dx=64, dy=64)
    cut = Image.new("L", img.size, 0)
    ImageDraw.Draw(cut).polygon([(size, 0), (size, size), (0, size)], fill=255)
    dark.putalpha(ImageChops.multiply(dark.getchannel("A"), cut))
    light = Image.new("RGBA", img.size, (0, 0, 0, 0))
    _f_shape(light, k, WHITE_TOP, WHITE_BOTTOM)
    body = Image.new("RGBA", img.size, (0, 0, 0, 0))
    body.alpha_composite(dark)
    body.alpha_composite(light)
    img.alpha_composite(body.rotate(-14, center=(size / 2, size / 2), resample=Image.BICUBIC))


# ---- V50 弯笔绸带 F：竖笔是一条向左微弯的圆头粗曲线（V18 绸带气质）----
def v50(img: Image.Image, size: int, k: float) -> None:
    _wave(img, (470, 300), (370, 540), (470, 800), 150, "#F4F8FF", k)
    _grad_box(img, (395, 230, 745, 375), 54, WHITE_TOP, WHITE_BOTTOM)
    _grad_box(img, (390, 480, 680, 596), 46, WHITE_TOP, WHITE_BOTTOM)


# ---- V51 浮波 F：白 F 踏在两道亮蓝圆头波上（V15 悬浮双波气质）----
def v51(img: Image.Image, size: int, k: float) -> None:
    _grad_box(img, (350, 190, 504, 830), 52, WHITE_TOP, WHITE_BOTTOM)
    _grad_box(img, (350, 190, 736, 346), 52, WHITE_TOP, WHITE_BOTTOM)
    _grad_box(img, (350, 436, 666, 570), 46, WHITE_TOP, WHITE_BOTTOM)
    _wave(img, (280, 856), (512, 940), (744, 856), 88, "#5FA8FF", k)
    _wave(img, (356, 962), (512, 1016), (668, 962), 72, "#8FB7E8", k)


# ---- V52 铭板 F：白色圆角铭板浮于蓝衬之上，蓝色 F 印在板心（经典字母牌）----
def v52(img: Image.Image, size: int, k: float) -> None:
    _solid(img, (256, 306, 786, 836), 96, MID_BLUE, k)
    _grad_box(img, (216, 256, 746, 786), 96, WHITE_TOP, WHITE_BOTTOM)
    _grad_box(img, (312, 322, 448, 716), 44, BLUE_TOP, BLUE_BOTTOM)
    _grad_box(img, (312, 322, 646, 456), 44, BLUE_TOP, BLUE_BOTTOM)
    _grad_box(img, (312, 524, 586, 652), 40, BLUE_TOP, BLUE_BOTTOM)


# ---- V53 闪电贴：实心多边形闪电 + 浅蓝错位背板（V3 切面手法，速度符号）----
BOLT_POLYGON = [(560, 140), (180, 600), (480, 600), (400, 900), (870, 340), (560, 340)]


def v53(img: Image.Image, size: int, k: float) -> None:
    _polygon(img, [(x + 46, y + 46) for x, y in BOLT_POLYGON], "#5FA8FF", k)
    _polygon(img, BOLT_POLYGON, "#F4F8FF", k)


# ---- V54 F 落点：大号白 F + 右下一颗蓝色圆点（F. 品牌句点 = 内容落点）----
def v54(img: Image.Image, size: int, k: float) -> None:
    _grad_box(img, (320, 200, 476, 820), 52, WHITE_TOP, WHITE_BOTTOM)
    _grad_box(img, (320, 200, 706, 358), 52, WHITE_TOP, WHITE_BOTTOM)
    _grad_box(img, (320, 452, 640, 586), 46, WHITE_TOP, WHITE_BOTTOM)
    dot = Image.new("RGBA", img.size, (0, 0, 0, 0))
    ImageDraw.Draw(dot).ellipse(
        [(770 - 88) * k, (762 - 88) * k, (770 + 88) * k, (762 + 88) * k], fill=mi._rgba("#3D97FF")
    )
    img.alpha_composite(dot)


# ---- 第十轮：V53 同族——白色实心多边形 + 浅蓝错位背板，替换动作主体 ----

def _offset_pts(pts: list[tuple[float, float]], dx: float, dy: float) -> list[tuple[float, float]]:
    return [(x + dx, y + dy) for x, y in pts]


# ---- V55 疾冲箭头：粗箭头斜指右上，粘贴的方向感与速度感 ----
ARROW_PTS = [(232, 437), (532, 437), (532, 322), (792, 512), (532, 702), (532, 587), (232, 587)]


def v55(img: Image.Image, size: int, k: float) -> None:
    pts = _rot_pts(ARROW_PTS, -45, 512, 512)
    _polygon(img, _offset_pts(pts, 48, 48), "#5FA8FF", k)
    _polygon(img, pts, "#F4F8FF", k)


# ---- V56 双人字疾进：两道人字形色块斜向冲刺（快进 / 连续速贴）----
CHEVRON_PTS = [(250, 270), (440, 270), (660, 512), (440, 754), (250, 754), (470, 512)]


def v56(img: Image.Image, size: int, k: float) -> None:
    back = _rot_pts(_offset_pts(CHEVRON_PTS, -70, 0), -20, 512, 512)
    front = _rot_pts(_offset_pts(CHEVRON_PTS, 185, 0), -20, 512, 512)
    _polygon(img, back, "#5FA8FF", k)
    _polygon(img, front, "#F4F8FF", k)


# ---- V57 对勾贴：实心对勾 + 错位背板，「贴好了」的确认感 ----
CHECK_PTS = [(220, 560), (340, 430), (470, 570), (680, 300), (820, 410), (475, 810)]


def v57(img: Image.Image, size: int, k: float) -> None:
    _polygon(img, _offset_pts(CHECK_PTS, 46, 46), "#5FA8FF", k)
    _polygon(img, CHECK_PTS, "#F4F8FF", k)


# ---- V58 加号速贴：粗短圆角十字 + 错位背板，快速添加一条内容 ----
def v58(img: Image.Image, size: int, k: float) -> None:
    _solid(img, (348, 484, 772, 636), 56, "#5FA8FF", k)
    _solid(img, (484, 348, 636, 772), 56, "#5FA8FF", k)
    _solid(img, (300, 436, 724, 588), 56, "#F4F8FF", k)
    _solid(img, (436, 300, 588, 724), 56, "#F4F8FF", k)


# ---- V59 斜叠三棱：三条斜向色块沿对角排开（多条目依次浮出）----
def _diag_band(cx: float, cy: float, half_len: float, half_w: float) -> list[tuple[float, float]]:
    d, n = (0.7071, -0.7071), (0.7071, 0.7071)
    return [
        (cx + d[0] * a * half_len + n[0] * b * half_w, cy + d[1] * a * half_len + n[1] * b * half_w)
        for a, b in [(1, 1), (1, -1), (-1, -1), (-1, 1)]
    ]


def v59(img: Image.Image, size: int, k: float) -> None:
    for cx, cy, col in [(380, 430, "#8FB7E8"), (510, 560, "#5FA8FF"), (640, 690, "#F4F8FF")]:
        _polygon(img, _diag_band(cx, cy, 250, 54), col, k)


# ---- V60 光标疾贴：光标箭头斜冲 + 左下两道速度短线（V2 的背板强化版）----
def v60(img: Image.Image, size: int, k: float) -> None:
    scale = 1.42
    base = [(358.0 + x * scale, 206.0 + y * scale) for x, y in CURSOR_POLYGON]
    pts = _rot_pts(base, -28, 512, 512)
    _polygon(img, _offset_pts(pts, 48, 48), "#5FA8FF", k)
    _polygon(img, pts, "#F4F8FF", k)
    _wave(img, (250, 800), (300, 750), (350, 700), 62, "#5FA8FF", k)
    _wave(img, (165, 705), (195, 675), (225, 645), 48, "#5FA8FF", k)


R1_VARIANTS = [
    ("V1 双层浮卡", v1),
    ("V2 光标浮标", v2),
    ("V3 速贴纸飞机", v3),
    ("V4 剪贴板", v4),
    ("V5 折角便签", v5),
    ("V6 速贴气泡", v6),
]

# 第二轮：保留 V3 斜向主体 + 切面切分的形式，替换纸飞机主体
R2_VARIANTS = [
    ("V7 疾飞光标", v7),
    ("V8 飞出浮卡", v8),
    ("V9 粘贴箭镞", v9),
    ("V10 双三角疾驰", v10),
    ("V11 折角飞贴", v11),
    ("V12 斜切浮层", v12),
]

# 第三轮：柔和线条——圆头曲线 / 圆 / 胶囊，零尖角
R3_VARIANTS = [
    ("V13 彗星飞贴", v13),
    ("V14 悬浮双波", v14),
    ("V15 胶囊流风", v15),
    ("V16 弯月滑翔", v16),
    ("V17 弧托圆", v17),
    ("V18 绸带S流", v18),
]

# 第四轮：沿 V15/V18 的圆头线条流动语言发散
R4_VARIANTS = [
    ("V19 流风三叠", v19),
    ("V20 回环一笔", v20),
    ("V21 柔光闪弧", v21),
    ("V22 括弧聚点", v22),
    ("V23 光点拖影", v23),
    ("V24 卷曲飘带", v24),
]

# 第五轮：柔和线条 × 功能语义（速贴浮窗 / 历史堆叠 / 剪贴板 / 粘贴落点）
R5_VARIANTS = [
    ("V25 速贴浮窗", v25),
    ("V26 历史叠层", v26),
    ("V27 圆头剪贴板", v27),
    ("V28 落点浮窗", v28),
    ("V29 浮卡双波", v29),
    ("V30 时光序列", v30),
]

# 第六轮：「贴」的意象——翘角贴纸 / 出血浮窗 / 斜置叠层
R6_VARIANTS = [
    ("V31 翘角贴纸", v31),
    ("V32 双贴纸", v32),
    ("V33 出血浮窗", v33),
    ("V34 旋叠历史", v34),
    ("V35 大揭角", v35),
    ("V36 斜贴纸落点", v36),
]

# 第七轮：功能动作视角——快捷键 / 搬运 / 磁吸 / 循环
R7_VARIANTS = [
    ("V37 键帽速贴", v37),
    ("V38 传递弧线", v38),
    ("V39 双窗桥接", v39),
    ("V40 磁吸内容", v40),
    ("V41 键帽浮波", v41),
    ("V42 循环节拍", v42),
]

# 第八轮：立体悬浮——斜视浮板带厚度与投影
R8_VARIANTS = [
    ("V43 斜视浮板", v43),
    ("V44 双浮层", v44),
    ("V45 浮板落点", v45),
    ("V46 浮板踏波", v46),
    ("V47 云梯浮层", v47),
    ("V48 斜视剪贴板", v48),
]


# 第九轮：字母 F——斜切面 / 弯笔绸带 / 浮波 / 铭板 / 闪电 / 落点
R9_VARIANTS = [
    ("V49 斜冲F", v49),
    ("V50 弯笔F", v50),
    ("V51 浮波F", v51),
    ("V52 铭板F", v52),
    ("V53 圆头闪电", v53),
    ("V54 F落点", v54),
]


# 第十轮：V53 同族——实心多边形 + 错位背板（箭头 / 双人字 / 对勾 / 加号 / 叠棱 / 光标）
R10_VARIANTS = [
    ("V55 疾冲箭头", v55),
    ("V56 双人字", v56),
    ("V57 对勾贴", v57),
    ("V58 加号速贴", v58),
    ("V59 斜叠三棱", v59),
    ("V60 光标疾贴", v60),
]


def render_variant(draw_fn, px: int, supersample: int) -> Image.Image:
    size = px * supersample
    img = Image.new("RGBA", (size, size), (0, 0, 0, 0))
    tile = mi._multi_stop_gradient(size).convert("RGBA")
    tile.putalpha(mi._rounded_mask(size, (0, 0, mi.BASE, mi.BASE), mi.TILE_RADIUS))
    img.alpha_composite(tile)
    draw_fn(img, size, size / mi.BASE)
    return mi._downscale_premultiplied(img, px)


def build_sheet(variants, out_name: str) -> None:
    row_h, beauty = 150, 128
    sheet_w = 90 + beauty + 40 + (48 + 32 + 24 + 16) + 4 * 14 + 40
    sheet_h = len(variants) * row_h + 30
    sheet = Image.new("RGBA", (sheet_w, sheet_h), (30, 30, 32, 255))
    try:
        font = ImageFont.truetype("C:/Windows/Fonts/segoeui.ttf", 26)
    except OSError:
        font = ImageFont.load_default()

    draw = ImageDraw.Draw(sheet)
    for i, (name, fn) in enumerate(variants):
        y = 20 + i * row_h
        draw.text((16, y + 40), name.split()[0], fill=(220, 220, 225, 255), font=font)

        beauty_img = render_variant(fn, 256, 2).resize((beauty, beauty), Image.LANCZOS)
        sheet.paste(beauty_img, (90, y + 8), beauty_img)

        x = 90 + beauty + 40
        for px in (48, 32, 24, 16):
            small = render_variant(fn, px, 8)
            sheet.paste(small, (x, y + (row_h - 16 - px) // 2 + 4), small)
            x += px + 14

        path = PREVIEW / f"variant_{name.split()[0].lower()}_256.png"
        render_variant(fn, 256, 2).save(path)

    sheet.save(PREVIEW / out_name)
    print(f"已生成 {PREVIEW / out_name}")


def main() -> None:
    PREVIEW.mkdir(parents=True, exist_ok=True)
    build_sheet(R1_VARIANTS, "variants_sheet.png")
    build_sheet(R2_VARIANTS, "variants_sheet_r2.png")
    build_sheet(R3_VARIANTS, "variants_sheet_r3.png")
    build_sheet(R4_VARIANTS, "variants_sheet_r4.png")
    build_sheet(R5_VARIANTS, "variants_sheet_r5.png")
    build_sheet(R6_VARIANTS, "variants_sheet_r6.png")
    build_sheet(R7_VARIANTS, "variants_sheet_r7.png")
    build_sheet(R8_VARIANTS, "variants_sheet_r8.png")
    build_sheet(R9_VARIANTS, "variants_sheet_r9.png")
    build_sheet(R10_VARIANTS, "variants_sheet_r10.png")


if __name__ == "__main__":
    main()
