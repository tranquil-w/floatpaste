//! 区域抓屏：BitBlt 从屏幕 DC 取 DWM 合成后的最终画面（含其他应用
//! 窗口与壁纸），供速贴自绘模糊底图取材（`core::backdrop`）。
//! 输出 RGBA8、top-down。

use windows::Win32::Graphics::Gdi::*;

/// 抓取屏幕区域（物理像素，虚拟屏坐标系）。失败返回 None
/// （锁屏/会话切换/DC 不可得等）
pub fn capture_region_rgba(x: i32, y: i32, w: i32, h: i32) -> Option<Vec<u8>> {
    if w <= 0 || h <= 0 || w > 16384 || h > 16384 {
        return None;
    }
    unsafe {
        let hdc_screen = GetDC(None);
        if hdc_screen.is_invalid() {
            return None;
        }
        let result = capture_with_dc(hdc_screen, x, y, w, h);
        let _ = ReleaseDC(None, hdc_screen);
        result
    }
}

unsafe fn capture_with_dc(hdc_screen: HDC, x: i32, y: i32, w: i32, h: i32) -> Option<Vec<u8>> {
    unsafe {
        let hdc_mem = CreateCompatibleDC(Some(hdc_screen));
        let bmi = BITMAPINFO {
            bmiHeader: BITMAPINFOHEADER {
                biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
                biWidth: w,
                biHeight: -h, // top-down
                biPlanes: 1,
                biBitCount: 32,
                biCompression: BI_RGB.0,
                ..Default::default()
            },
            ..Default::default()
        };
        let mut bits: *mut std::ffi::c_void = std::ptr::null_mut();
        let dib = CreateDIBSection(Some(hdc_mem), &bmi, DIB_RGB_COLORS, &mut bits, None, 0).ok()?;
        let old = SelectObject(hdc_mem, dib.into());
        // CAPTUREBLT：纳入 layered/topmost 窗口（否则这些区域取到黑）
        let blt = BitBlt(
            hdc_mem,
            0,
            0,
            w,
            h,
            Some(hdc_screen),
            x,
            y,
            SRCCOPY | CAPTUREBLT,
        );
        let mut out = None;
        if blt.is_ok() && !bits.is_null() {
            let buf =
                std::slice::from_raw_parts(bits as *const u8, (w as usize) * (h as usize) * 4);
            // 32bpp DIB 内存序为 BGRA，swizzle 成 RGBA 并置满 alpha
            let mut rgba = buf.to_vec();
            for chunk in rgba.chunks_exact_mut(4) {
                chunk.swap(0, 2);
                chunk[3] = 255;
            }
            out = Some(rgba);
        }
        let _ = SelectObject(hdc_mem, old);
        let _ = DeleteObject(dib.into());
        let _ = DeleteDC(hdc_mem);
        out
    }
}
