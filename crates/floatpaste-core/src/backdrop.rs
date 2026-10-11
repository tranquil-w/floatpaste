//! 速贴无焦点窗的自绘模糊底图：唤起时抓取窗口背后屏幕区域，CPU
//! 模糊 + 主题 tint + 圆角烘焙成不透明底图。抓屏区域向四周外扩一个
//! 模糊半径、模糊后裁回窗口尺寸——边缘模糊吃到窗外内容，不因滑窗
//! 在图像边界 clamp 而比中心更「实」。
//!
//! 为什么不用 DWM 管线：
//! - SystemBackdrop 的 Acrylic 模糊是激活态特权——失活窗口只剩半透明
//!   无模糊（条纹振幅实验：激活态 8.4 / 失活态 29.1）；速贴是
//!   NOACTIVATE 常态失活窗，热键唤起永远拿不到玻璃感
//! - SWCA accent（ACCENT_ENABLE_ACRYLICBLURBEHIND）在 Win11 26100 上
//!   不产生模糊（振幅 95~106，仅 tint）——软渲侧自画模糊是唯一出路
//!
//! 代价是底图为唤起瞬间的静态快照（背后内容动态变化不跟随），
//! 速贴是瞬态面板，可接受。

/// 自绘模糊半径（逻辑 px，物理半径 = 该值 × 缩放因子）。观感目标是
/// 饱和玻璃（Acrylic 面板底）：半径过小时复杂背景的高对比结构只是
/// 糊边不成雾，留下一团团可辨识的色斑
pub const BLUR_RADIUS_LOGICAL: f32 = 30.0;

/// Acrylic 档速贴烘焙的 tint alpha（明暗同值）：比可激活窗侧的
/// material-layer 0.70（ADR 锁定勿放开）更实一档。速贴底图是唤起
/// 瞬间的静态快照，更实的 tint 稀释静态背景的杂色存在感；此档同时
/// 是半透玻璃卡的分层甜点（卡底合成差浅色 ~33 / 深色 ~11 灰阶，
/// 0.85+ 浅色塌到 9 阶，快照取色实测）
pub const ACRYLIC_TINT_ALPHA: f32 = 0.80;

/// Mica 档速贴烘焙的 tint alpha（明暗同值）。Mica 材质本身不透明
/// （壁纸取色染色，不透窗口背后的内容），可激活窗侧 DWM Mica 即如此；
/// 速贴的近似 = 近不透明的烘焙底：92% tint 盖住模糊底图，只留 8% 色
/// 晕暗示「背后环境」的存在（烘焙管线的意义所在，全盖死则与「无」档
/// 实底无区分且烘焙白跑）。此实度下半透玻璃卡必然塌层（0.85+ 卡底
/// 合成差即跌到 9 灰阶，快照取色实测），行卡走不透明实卡 + 细描边
/// 分层。速贴永不激活，DWM 对从未激活窗不渲染 Mica backdrop（实测），
/// 两档均走本烘焙管线
pub const MICA_TINT_ALPHA: f32 = 0.92;

/// 三遍 box blur 近似高斯。`px` 为 RGBA8（内存序 R,G,B,A）、top-down，
/// 行宽 `w`。滑动窗口实现，O(n) 与半径无关
pub fn blur_rgba(px: &mut [u8], w: usize, h: usize, radius: usize) {
    debug_assert!(px.len() == w * h * 4);
    if w == 0 || h == 0 || radius == 0 {
        return;
    }
    let mut tmp = vec![0u8; px.len()];
    for _ in 0..3 {
        blur_pass_h(px, &mut tmp, w, h, radius);
        blur_pass_v(&tmp, px, w, h, radius);
    }
}

/// 单遍水平滑窗 box（RGBA 四通道整体均值——alpha 随后由 tint 重置，
/// 这里一并模糊无碍）
fn blur_pass_h(src: &[u8], dst: &mut [u8], w: usize, h: usize, r: usize) {
    for row in 0..h {
        let base = row * w * 4;
        let mut sum = [0u32; 4];
        let hi0 = r.min(w - 1);
        for xi in 0..=hi0 {
            for c in 0..4 {
                sum[c] += src[base + xi * 4 + c] as u32;
            }
        }
        let out0 = base;
        for c in 0..4 {
            dst[out0 + c] = (sum[c] / (hi0 + 1) as u32) as u8;
        }
        for x in 1..w {
            let new_hi = (x + r).min(w - 1);
            let prev_hi = (x - 1 + r).min(w - 1);
            if new_hi > prev_hi {
                for c in 0..4 {
                    sum[c] += src[base + new_hi * 4 + c] as u32;
                }
            }
            let prev_lo = (x - 1).saturating_sub(r);
            let new_lo = x.saturating_sub(r);
            if new_lo > prev_lo {
                for c in 0..4 {
                    sum[c] -= src[base + prev_lo * 4 + c] as u32;
                }
            }
            let n = (new_hi - new_lo + 1) as u32;
            let out = base + x * 4;
            for c in 0..4 {
                dst[out + c] = (sum[c] / n) as u8;
            }
        }
    }
}

/// 单遍垂直滑窗 box
fn blur_pass_v(src: &[u8], dst: &mut [u8], w: usize, h: usize, r: usize) {
    let stride = w * 4;
    for x in 0..w {
        let col = x * 4;
        let mut sum = [0u32; 4];
        let hi0 = r.min(h - 1);
        for yi in 0..=hi0 {
            for c in 0..4 {
                sum[c] += src[yi * stride + col + c] as u32;
            }
        }
        for c in 0..4 {
            dst[col + c] = (sum[c] / (hi0 + 1) as u32) as u8;
        }
        for y in 1..h {
            let new_hi = (y + r).min(h - 1);
            let prev_hi = (y - 1 + r).min(h - 1);
            if new_hi > prev_hi {
                for c in 0..4 {
                    sum[c] += src[new_hi * stride + col + c] as u32;
                }
            }
            let prev_lo = (y - 1).saturating_sub(r);
            let new_lo = y.saturating_sub(r);
            if new_lo > prev_lo {
                for c in 0..4 {
                    sum[c] -= src[prev_lo * stride + col + c] as u32;
                }
            }
            let n = (new_hi - new_lo + 1) as u32;
            let out = y * stride + col;
            for c in 0..4 {
                dst[out + c] = (sum[c] / n) as u8;
            }
        }
    }
}

/// 从 `w×h` 大图中裁取偏移 `(ox, oy)` 起、尺寸 `out_w×out_h` 的子图。
/// 配合扩边抓屏：整图模糊后裁回窗口尺寸，消除滑窗边界 clamp 的模糊量
/// 缩水与拉丝；抓取范围钳到虚拟桌面时裁剪偏移不对称，故带偏移
pub fn crop(
    px: &[u8],
    w: usize,
    h: usize,
    ox: usize,
    oy: usize,
    out_w: usize,
    out_h: usize,
) -> Vec<u8> {
    debug_assert!(px.len() == w * h * 4);
    debug_assert!(ox + out_w <= w && oy + out_h <= h);
    let mut out = vec![0u8; out_w * out_h * 4];
    for row in 0..out_h {
        let src = ((row + oy) * w + ox) * 4;
        let dst = row * out_w * 4;
        out[dst..dst + out_w * 4].copy_from_slice(&px[src..src + out_w * 4]);
    }
    out
}

/// 主题 tint 混合并置不透明：`out = src*(1-a) + rgb*a`，alpha 通道置
/// 255。模糊底图的最终观感层（等效原 material-layer 0.70 半透叠底）
pub fn tint_opaque(px: &mut [u8], rgb: [u8; 3], alpha: f32) {
    let a = alpha.clamp(0.0, 1.0);
    for chunk in px.chunks_exact_mut(4) {
        for c in 0..3 {
            chunk[c] = (chunk[c] as f32 * (1.0 - a) + rgb[c] as f32 * a).round() as u8;
        }
        chunk[3] = 255;
    }
}

/// 圆角烘焙：四角外 alpha=0（带半像素抗锯齿）。直线边缘不受影响。
/// `radius` 为物理像素圆角半径
pub fn bake_round_corners(px: &mut [u8], w: usize, h: usize, radius: usize) {
    if radius == 0 || w < radius * 2 || h < radius * 2 {
        return;
    }
    let r = radius as f32;
    let (left_c, right_c) = (r, (w - 1) as f32 - r);
    let (top_c, bottom_c) = (r, (h - 1) as f32 - r);
    for dy in 0..radius {
        for dx in 0..radius {
            let (lx, rx) = (dx, w - 1 - dx);
            let (ty, by) = (dy, h - 1 - dy);
            bake_corner(px, w, lx, ty, left_c, top_c, r);
            bake_corner(px, w, rx, ty, right_c, top_c, r);
            bake_corner(px, w, lx, by, left_c, bottom_c, r);
            bake_corner(px, w, rx, by, right_c, bottom_c, r);
        }
    }
}

/// 单像素圆角覆盖度：圆外 alpha 按半像素线性衰减
fn bake_corner(px: &mut [u8], w: usize, x: usize, y: usize, ccx: f32, ccy: f32, r: f32) {
    let dx = x as f32 - ccx;
    let dy = y as f32 - ccy;
    let d = (dx * dx + dy * dy).sqrt();
    let coverage = (r + 0.5 - d).clamp(0.0, 1.0);
    let idx = (y * w + x) * 4 + 3;
    px[idx] = (px[idx] as f32 * coverage) as u8;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn blur_uniform_is_identity() {
        let mut px = vec![200u8; 4 * 4 * 4];
        let before = px.clone();
        blur_rgba(&mut px, 4, 4, 2);
        assert_eq!(px, before, "均匀色模糊后应保持不变");
    }

    #[test]
    fn blur_single_hot_pixel_spreads_and_conserves() {
        // 8×8 黑底中心白点：三遍 box 后能量近似守恒（只重分布）
        let mut px = vec![0u8; 8 * 8 * 4];
        for c in 0..4 {
            px[(3 * 8 + 3) * 4 + c] = 255;
        }
        let before_sum: u64 = px.iter().map(|v| *v as u64).sum();
        blur_rgba(&mut px, 8, 8, 1);
        let after_sum: u64 = px.iter().map(|v| *v as u64).sum();
        let diff = before_sum.abs_diff(after_sum);
        assert!(
            diff < 300,
            "模糊应近似守恒：before={before_sum} after={after_sum} diff={diff}"
        );
        assert!(px[(3 * 8 + 3) * 4] > 0, "中心应仍有点亮");
        assert!(px[(3 * 8 + 4) * 4] > 0, "邻列应获得亮度");
        assert_eq!(px[0], 0, "远角不应被波及");
    }

    #[test]
    fn crop_extracts_region_at_offset() {
        // 4×4 每像素记录坐标编码：对称裁 1 应得 2×2 中心块；
        // 不对称偏移 (0,2) 应得贴边 2×2 块（钳制抓屏的偏移路径）
        let mut px = Vec::new();
        for y in 0..4 {
            for x in 0..4 {
                px.extend_from_slice(&[x as u8, y as u8, 0, 255]);
            }
        }
        let out = crop(&px, 4, 4, 1, 1, 2, 2);
        assert_eq!(out.len(), 2 * 2 * 4);
        assert_eq!(&out[0..4], &[1, 1, 0, 255], "左上应取自 (1,1)");
        assert_eq!(&out[out.len() - 4..], &[2, 2, 0, 255], "右下应取自 (2,2)");
        let edge = crop(&px, 4, 4, 0, 2, 4, 2);
        assert_eq!(&edge[0..4], &[0, 2, 0, 255], "不对称裁剪左上应取自 (0,2)");
    }

    #[test]
    fn expanded_capture_blurs_edges_with_outside_content() {
        // 贴角白块：扩边抓屏+裁剪路径的角落应混入窗外黑背景（变暗），
        // 直接按窗口大小模糊则被边界 clamp 重复自身（保持近白）；
        // 远离边缘与色块的中心点两路径须完全一致
        let (w, h, r, m, block) = (200usize, 200usize, 12usize, 12usize, 80usize);
        let mut direct = vec![0u8; w * h * 4];
        for chunk in direct.chunks_exact_mut(4) {
            chunk[3] = 255;
        }
        for y in 0..block {
            for x in 0..block {
                let i = (y * w + x) * 4;
                direct[i] = 255;
                direct[i + 1] = 255;
                direct[i + 2] = 255;
            }
        }
        let mut expanded = vec![0u8; (w + 2 * m) * (h + 2 * m) * 4];
        for chunk in expanded.chunks_exact_mut(4) {
            chunk[3] = 255;
        }
        for y in 0..block {
            for x in 0..block {
                let i = ((y + m) * (w + 2 * m) + (x + m)) * 4;
                expanded[i] = 255;
                expanded[i + 1] = 255;
                expanded[i + 2] = 255;
            }
        }
        blur_rgba(&mut direct, w, h, r);
        blur_rgba(&mut expanded, w + 2 * m, h + 2 * m, r);
        let cropped = crop(&expanded, w + 2 * m, h + 2 * m, m, m, w, h);
        let corner = |px: &[u8]| px[0];
        assert!(
            corner(&cropped) < 200,
            "扩边裁剪后角落应被窗外内容稀释: {}",
            corner(&cropped)
        );
        assert!(
            corner(&direct) > 240,
            "clamp 路径角落应近白: {}",
            corner(&direct)
        );
        let mid = (150 * w + 150) * 4;
        assert_eq!(cropped[mid], direct[mid], "远离边缘处两路径应一致");
    }

    #[test]
    fn tint_mixes_to_target() {
        let mut px = vec![255u8, 255, 255, 128, 0, 0, 0, 128];
        tint_opaque(&mut px, [0, 0, 0], 0.5);
        assert_eq!(px[0], 128, "白底 50% 黑 tint 应得 128");
        assert_eq!(px[1], 128);
        assert_eq!(px[3], 255, "tint 后应不透明");
        assert_eq!(px[7], 255);
    }

    #[test]
    fn round_corners_zeroes_corner_alpha() {
        let mut px = vec![255u8; 10 * 10 * 4];
        bake_round_corners(&mut px, 10, 10, 3);
        assert_eq!(px[3], 0, "左上角应被裁掉");
        assert_eq!(px[(9 * 10 + 9) * 4 + 3], 0, "右下角应被裁掉");
        assert_eq!(px[(5 * 10 + 5) * 4 + 3], 255, "中心不受影响");
        assert_eq!(px[(5 * 10 + 0) * 4 + 3], 255, "直线边缘不受影响");
    }
}
