//! 开始菜单/搜索面板定位探针：诊断「菜单开着时速贴落位不对」的常驻工具。
//!
//! 用法（在仓库根，先把开始菜单/搜索面板打开到前台）：
//!   cargo run -p floatpaste-core --example start_menu_probe -- [宽] [高]
//!
//! 走生产链路（`start_menu.rs` + `picker_position_service.rs`）输出两层结果：
//! 宿主识别与面板矩形（含全屏宿主时的 UIA 兜底取值）、速贴最终落位。
//! 面板矩形与屏幕上可见面板对照、落位与面板边缘对照，即可判断问题
//! 落在哪个桶（宿主识别 / 几何获取 / 贴边计算）。

use floatpaste_core::domain::settings::PickerPositionMode;
use floatpaste_core::platform::windows::start_menu;
use floatpaste_core::repository::sqlite_repository::SqliteRepository;
use floatpaste_core::services::picker_position_service::PickerPositionService;

fn main() {
    let width: i32 = std::env::args()
        .nth(1)
        .and_then(|value| value.parse().ok())
        .unwrap_or(358);
    let height: i32 = std::env::args()
        .nth(2)
        .and_then(|value| value.parse().ok())
        .unwrap_or(461);
    println!("等待开始菜单/搜索面板到前台（ms-search: 或按 Win）…");

    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(20);
    loop {
        if let Some(rect) = start_menu::foreground_start_menu_rect() {
            println!(
                "面板矩形: ({},{})-({},{}) {}x{}",
                rect.left,
                rect.top,
                rect.right,
                rect.bottom,
                rect.width(),
                rect.height()
            );
            let repository =
                SqliteRepository::new(&std::env::temp_dir().join("floatpaste-probe.db"))
                    .expect("打开探针数据库失败");
            let placed = PickerPositionService::resolve_window_position(
                &repository,
                &PickerPositionMode::Mouse,
                width,
                height,
                None,
            );
            match placed {
                Ok(Some(point)) => {
                    println!("速贴落位: ({},{}) 窗口 {width}x{height}", point.x, point.y)
                }
                Ok(None) => println!("定位解析返回 None"),
                Err(error) => println!("定位解析报错: {error}"),
            }
            return;
        }
        if std::time::Instant::now() > deadline {
            println!("20 秒内开始菜单/搜索面板未到前台");
            return;
        }
        std::thread::sleep(std::time::Duration::from_millis(100));
    }
}
