//! L3 真进程端到端冒烟：spawn 真实二进制，靠隔离钩子把被测实例的数据与
//! 系统副作用圈在临时目录内（APPDATA 重定向隔离数据库/图片/日志；
//! FLOATPASTE_INSTANCE_ID 隔离单实例互斥量与唤醒事件；
//! FLOATPASTE_RETENTION_FIRST_DELAY_MS 把首轮保留清理缩短到秒级；
//! FLOATPASTE_SKIP_AUTOSTART_SYNC 挡住对真实系统 Run 键与任务计划的同步）。
//!
//! 外部驱动只走真实用户路径：写系统剪贴板触发录入、二次启动进程经唤醒
//! 事件开关速贴面板、命名事件驱动托盘「切换监听」的处理路径；断言落在
//! 隔离库（只读连接）与窗口几何上。
//! 上屏注入（SendInput）依赖注入环境是否放行，按验收纪律留给用户实测。
//!
//! 单测试函数串行推进：系统剪贴板、全局热键、速贴窗口都是会话级全局
//! 资源，不可并行。运行期间被测实例会短暂占用 Ctrl+Q/Alt+S 并读写
//! 剪贴板，结束时尽力恢复原文本。

use std::{
    fs,
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    thread,
    time::{Duration, Instant},
};

use floatpaste_core::{
    domain::settings::{PickerPositionMode, StoredWindowPosition, UserSetting},
    platform::windows::picker_position::{
        current_cursor_point, primary_monitor_work_area, work_area_from_point,
    },
    repository::sqlite_repository::SqliteRepository,
    services::normalize_service::NormalizeService,
    services::picker_position_service::center_in_work_area,
};
use windows::{
    core::{BOOL, PCWSTR},
    Win32::{
        Foundation::{HWND, LPARAM, RECT},
        System::DataExchange::{CloseClipboard, OpenClipboard},
        UI::{
            HiDpi::GetDpiForWindow,
            WindowsAndMessaging::{
                EnumWindows, FindWindowW, GetWindowRect, GetWindowTextW, IsWindowVisible,
            },
        },
    },
};

/// 预置未收藏条目数：越过 history_limit 触发溢出清理
const BULK_ITEMS: usize = 105;
const HISTORY_LIMIT: u32 = 100;
const PICKER_TITLE: &str = "FloatPaste · 速贴";
const SEARCH_TITLE: &str = "FloatPaste · 搜索";
/// 停屏位在 Windows 坐标下限（-32000），左上角小于该阈值视为屏外
const OFFSCREEN_THRESHOLD: i32 = -30_000;

fn wait_until(timeout: Duration, interval: Duration, mut probe: impl FnMut() -> bool) -> bool {
    let deadline = Instant::now() + timeout;
    while Instant::now() < deadline {
        if probe() {
            return true;
        }
        thread::sleep(interval);
    }
    probe()
}

/// 测试启动前的剪贴板可用性探测：连续 ~1.2 秒打不开视为环境先在的
/// 锁死/长占用态。此时测试与被测实例都还没碰过剪贴板，责任不在被测
/// 物，重试无解——打印恢复指引后跳过，不把环境故障计成测试失败。
/// 运行中途的写入失败仍由 retry_clipboard 短重试后 panic 兜底（那才
/// 可能是被测实例的问题）
fn clipboard_unusable_before_start() -> bool {
    for _ in 0..8 {
        if unsafe { OpenClipboard(None) }.is_ok() {
            unsafe {
                let _ = CloseClipboard();
            }
            return false;
        }
        thread::sleep(Duration::from_millis(150));
    }
    true
}
/// 剪贴板是互斥资源，输入法、剪贴板管理器等外部进程会间歇占用；
/// 写入类操作短重试，持续失败才让测试失败
fn retry_clipboard<T>(mut op: impl FnMut() -> Result<T, arboard::Error>) -> T {
    let mut last = None;
    for _ in 0..15 {
        match op() {
            Ok(value) => return value,
            Err(error) => last = Some(error),
        }
        thread::sleep(Duration::from_millis(200));
    }
    panic!(
        "剪贴板操作持续失败（被外部进程占用）: {last:?}\n\
         若反复出现，系统剪贴板可能已被外部进程持有不释放或进程死在临界区\
         （见 docs/testing.md 故障模式）；恢复：taskkill /f /im explorer.exe \
         && start explorer.exe"
    );
}

/// 只读连接查隔离库；被测实例持有写连接（短事务），busy_timeout 兜并发
fn count_rows(db_path: &Path, sql: &str, param: Option<&str>) -> i64 {
    let connection =
        rusqlite::Connection::open_with_flags(db_path, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)
            .unwrap();
    connection.busy_timeout(Duration::from_secs(5)).unwrap();
    match param {
        Some(value) => connection
            .query_row(sql, [value], |row| row.get(0))
            .unwrap(),
        None => connection.query_row(sql, [], |row| row.get(0)).unwrap(),
    }
}

fn count_active_unfavorited(db_path: &Path) -> i64 {
    count_rows(
        db_path,
        "SELECT COUNT(*) FROM clip_items WHERE deleted_at IS NULL AND is_favorited = 0",
        None,
    )
}

fn active_full_text_exists(db_path: &Path, text: &str) -> bool {
    count_rows(
        db_path,
        "SELECT COUNT(*) FROM clip_items WHERE deleted_at IS NULL AND full_text = ?1",
        Some(text),
    ) > 0
}

/// 只读查询隔离库 settings 表，取 user_settings JSON 的 pauseMonitoring
/// 当前落库值（UserSetting 按 serde camelCase 序列化；托盘切换监听翻转后
/// 经 update_settings 即时落库，可作切换已受理的确认信号）
fn monitoring_paused(db_path: &Path) -> bool {
    let connection =
        rusqlite::Connection::open_with_flags(db_path, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)
            .unwrap();
    connection.busy_timeout(Duration::from_secs(5)).unwrap();
    let raw: String = connection
        .query_row(
            "SELECT value FROM settings WHERE key = 'user_settings'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    serde_json::from_str::<serde_json::Value>(&raw)
        .ok()
        .and_then(|value| {
            value
                .get("pauseMonitoring")
                .and_then(|field| field.as_bool())
        })
        .unwrap_or(false)
}

/// 速贴显隐是「屏内/屏外停屏」语义而非 SW_HIDE：隐藏=平移到 (-32000,-32000)，
/// Win32 可见性恒真。以窗口左上角是否落在屏内判定面板开合
fn picker_onscreen() -> bool {
    let title: Vec<u16> = PICKER_TITLE
        .encode_utf16()
        .chain(std::iter::once(0))
        .collect();
    unsafe {
        match FindWindowW(PCWSTR::null(), PCWSTR::from_raw(title.as_ptr())) {
            Ok(hwnd) if !hwnd.is_invalid() => {
                let mut rect = RECT::default();
                GetWindowRect(hwnd, &mut rect).is_ok()
                    && rect.left > OFFSCREEN_THRESHOLD
                    && rect.top > OFFSCREEN_THRESHOLD
            }
            _ => false,
        }
    }
}

struct SearchEnumState {
    found: Option<(isize, RECT)>,
}

unsafe extern "system" fn enum_onscreen_search_proc(hwnd: HWND, lparam: LPARAM) -> BOOL {
    let state = &mut *(lparam.0 as *mut SearchEnumState);
    let mut text = [0u16; 64];
    let len = GetWindowTextW(hwnd, &mut text);
    if len > 0 && String::from_utf16_lossy(&text[..len as usize]) == SEARCH_TITLE {
        let mut rect = RECT::default();
        // 屏内过滤必须叠加可见性：winit 预建的隐藏窗口自进程启动起就停在
        // 级联默认位（CI 实测 top 130~156），矩形上过得了停屏位阈值；不看
        // IsWindowVisible 会把「尚未装配/落位」的隐藏窗口当成已落位，测试
        // 与应用赛跑（CI 实测测试先赢：应用随即将窗正确落位 top=50，断言
        // 却已拿默认位 130 失败）。装配后的搜索窗走 Slint show + 停屏平移，
        // Win32 恒可见，可见性过滤不影响正常路径
        if GetWindowRect(hwnd, &mut rect).is_ok()
            && rect.top > OFFSCREEN_THRESHOLD
            && IsWindowVisible(hwnd).as_bool()
        {
            state.found = Some((hwnd.0 as isize, rect));
            return BOOL::from(false);
        }
    }
    BOOL::from(true)
}

/// 屏内搜索窗的句柄与几何。双实例并存（主场景实例 + 搜索窗场景实例）时
/// 存在同标题的停屏搜索窗，FindWindowW 返回哪个不确定，必须枚举并按
/// 屏内过滤；找不到（尚未打开/已停屏）返回 None
fn onscreen_search_window() -> Option<(isize, RECT)> {
    let mut state = SearchEnumState { found: None };
    unsafe {
        let _ = EnumWindows(
            Some(enum_onscreen_search_proc),
            LPARAM(&mut state as *mut _ as isize),
        );
    }
    state.found
}

/// 搜索窗在自身 DPI 下的满高（物理像素）：窗口逻辑 max 620px，首开无
/// 历史基线时以该高度起步并按光标所在工作区居中
fn search_window_full_height(hwnd: isize) -> i32 {
    let dpi = unsafe { GetDpiForWindow(HWND(hwnd as *mut _)) };
    (620.0 * dpi as f32 / 96.0).round() as i32
}

/// 搜索窗开窗落位的期望上边沿：被测实例与测试进程读的是同一静止光标，
/// 用同源的居中计算复算「满高起步」的居中 top，作为顶边锚定的基准——
/// 直接对比窗口两次读数会在「首读已收缩完成」时漏检重居中回归。
/// 光标读取被拒（输入桌面不可达，CI 断连会话必现）时与应用同样退回
/// 主屏工作区：单屏下矩形相同，基准与落位仍逐位一致
fn expected_search_top_at_full_height(full_height: i32) -> i32 {
    let work = match current_cursor_point()
        .and_then(|cursor| work_area_from_point(cursor))
    {
        Ok(work) => work,
        Err(error) => {
            eprintln!("[e2e] 光标/工作区不可得（{error}），基准退回主屏工作区");
            primary_monitor_work_area().expect("主屏工作区不可得")
        }
    };
    center_in_work_area(work, 0, full_height).y
}

fn spawn_instance(envs: &[(&str, String)], silent: bool) -> Child {
    let mut command = Command::new(env!("CARGO_BIN_EXE_floatpaste"));
    command
        .envs(envs.iter().cloned())
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    if silent {
        command.arg("--silent");
    }
    command.spawn().expect("启动被测实例失败")
}

/// 二次启动进程：单实例互斥量被主实例持有 → 发唤醒信号 → 干净退出。
/// 这是「二次启动唤醒面板」的真实用户路径，不依赖键盘注入。
fn wake_via_second_launch(envs: &[(&str, String)]) {
    let mut second = spawn_instance(envs, false);
    let exited_cleanly = wait_until(
        Duration::from_secs(15),
        Duration::from_millis(100),
        || matches!(second.try_wait(), Ok(Some(status)) if status.success()),
    );
    if !exited_cleanly {
        let _ = second.kill();
        let _ = second.wait();
        panic!("二次启动进程应在发送唤醒信号后以退出码 0 退出");
    }
}

/// 收尾守卫：无论测试成败（含断言 panic 的 unwind 路径）都让被测实例退出、
/// 尽力恢复剪贴板并清理临时目录——孤儿被测实例持有外层管道句柄会把
/// 整条测试命令挂死
struct Cleanup {
    child: Option<Child>,
    instance_id: String,
    original_text: Option<String>,
    data_root: PathBuf,
}

impl Cleanup {
    fn assert_running(&mut self, stage: &str) {
        let Some(child) = self.child.as_mut() else {
            unreachable!("守卫内子进程不应被提前取走");
        };
        let status = child.try_wait().expect("查询被测实例状态失败");
        assert!(
            status.is_none(),
            "被测实例在「{stage}」阶段前已退出: {status:?}"
        );
    }
}

impl Drop for Cleanup {
    fn drop(&mut self) {
        if let Some(mut child) = self.child.take() {
            // 优雅退出优先：被测实例若正处剪贴板打开临界区，强杀会把系统
            // 剪贴板锁死（OpenClipboard 持续 ERROR_ACCESS_DENIED，无法自愈）；
            // 超时（信号未装载或收尾卡住）才兜底强杀
            let _ = floatpaste_core::platform::windows::single_instance::signal_test_exit(
                &self.instance_id,
            );
            let exited = wait_until(Duration::from_secs(10), Duration::from_millis(100), || {
                matches!(child.try_wait(), Ok(Some(_)))
            });
            if !exited {
                let _ = child.kill();
            }
            let _ = child.wait();
        }
        if let Ok(mut clipboard) = arboard::Clipboard::new() {
            for _ in 0..5 {
                let attempt = match self.original_text.clone() {
                    Some(text) => clipboard.set_text(text).map(|_| ()),
                    None => clipboard.clear(),
                };
                if attempt.is_ok() {
                    break;
                }
                thread::sleep(Duration::from_millis(200));
            }
        }
        // 句柄延迟释放时删除失败不视作测试失败，留给系统临时目录自清
        let _ = fs::remove_dir_all(&self.data_root);
    }
}

#[test]
fn app_e2e_smoke_flow() {
    // 前置：环境剪贴板已锁死/被长占用时跳过（探测时序保证责任不在
    // 被测物，见 clipboard_unusable_before_start）
    if clipboard_unusable_before_start() {
        eprintln!(
            "跳过 e2e_smoke：系统剪贴板被外部进程持续占用（~1.2 秒内无法打开，\n\
             大概率已被锁死——外部进程持有剪贴板不释放或死在临界区，重试无法\n\
             恢复，见 docs/testing.md「故障模式：系统剪贴板锁死」）。\n\
             恢复命令：taskkill /f /im explorer.exe && start explorer.exe\n\
             环境恢复后重跑即恢复完整验证。"
        );
        return;
    }

    // ── 预置：105 条未收藏 + 1 条收藏 + history_limit=100 + 记忆位置 ──
    // 定位模式取 LastPosition 并预置屏内记忆位置：Mouse/Caret 模式依赖
    // 运行环境的光标与前台窗口（E2E 会话里不稳定），本测试验证的是
    // 唤醒开关面板链路而非定位模式本身
    let stamp = std::process::id();
    let data_root: PathBuf = std::env::temp_dir().join(format!("floatpaste-e2e-{stamp}"));
    let app_dir = data_root.join("com.floatpaste");
    fs::create_dir_all(&app_dir).unwrap();
    let db_path = app_dir.join("floatpaste.db");

    {
        let repository = SqliteRepository::new(&db_path).unwrap();
        let mut settings = UserSetting::default();
        settings.history_limit = HISTORY_LIMIT;
        settings.picker_position_mode = PickerPositionMode::LastPosition;
        repository.save_settings(&settings).unwrap();
        repository
            .save_picker_window_state(&StoredWindowPosition {
                x: 80,
                y: 80,
                width: Some(420),
                height: Some(600),
            })
            .unwrap();

        for index in 0..BULK_ITEMS {
            let item =
                NormalizeService::normalize_text(&format!("e2e-bulk-{index:03}"), None).unwrap();
            repository.save_text_item(&item).unwrap();
            // 活动时间戳以 1ms 步进保证严格单调，溢出清理「保留最新」的
            // 语义才可断言（同毫秒并列时 SQLite 删哪些行不确定）
            thread::sleep(Duration::from_millis(1));
        }
        let favorite = NormalizeService::normalize_text("e2e-favorite-keeper", None).unwrap();
        let favorite_id = repository.save_text_item(&favorite).unwrap().id;
        repository.set_favorited(&favorite_id, true).unwrap();
    }

    // spawn 前清空剪贴板：被测实例启动即主动核对一次当前剪贴板，
    // 残留内容会作为首条录入污染计数
    let mut clipboard = arboard::Clipboard::new().unwrap();
    let original_text = clipboard.get_text().ok();
    retry_clipboard(|| clipboard.clear());

    let instance_id = format!("e2e-{stamp}");
    let envs = [
        (
            "APPDATA",
            data_root
                .to_str()
                .expect("临时目录路径应为 UTF-8")
                .to_string(),
        ),
        ("FLOATPASTE_INSTANCE_ID", instance_id.clone()),
        ("FLOATPASTE_RETENTION_FIRST_DELAY_MS", "1500".to_string()),
        ("FLOATPASTE_SKIP_AUTOSTART_SYNC", "1".to_string()),
    ];
    // 场景 6 托盘切换监听事件的信号侧副本（Cleanup 构造会 move 走原值）
    let instance_id_for_toggle = instance_id.clone();
    let mut cleanup = Cleanup {
        child: Some(spawn_instance(&envs, true)),
        instance_id,
        original_text: original_text.clone(),
        data_root: data_root.clone(),
    };

    // ── 场景 1：保留清理接线（启动 → 延迟 → 首轮溢出清理）──
    cleanup.assert_running("保留清理");
    let compressed = wait_until(Duration::from_secs(20), Duration::from_millis(300), || {
        count_active_unfavorited(&db_path) == HISTORY_LIMIT as i64
    });
    assert!(
        compressed,
        "溢出清理应把未收藏未删条目压缩到 history_limit={HISTORY_LIMIT}，实际 {}",
        count_active_unfavorited(&db_path)
    );
    assert!(
        !active_full_text_exists(&db_path, "e2e-bulk-000"),
        "最旧的溢出条目应被物理删除"
    );
    assert!(
        active_full_text_exists(&db_path, "e2e-favorite-keeper"),
        "收藏条目应豁免溢出清理"
    );

    // ── 场景 2：剪贴板文本录入（监听 → 归一化 → 入库）──
    cleanup.assert_running("文本录入");
    let marker = format!("e2e-live-{stamp}");
    retry_clipboard(|| clipboard.set_text(marker.clone()));
    assert!(
        wait_until(Duration::from_secs(10), Duration::from_millis(200), || {
            active_full_text_exists(&db_path, &marker)
        }),
        "复制文本后应经剪贴板监听录入"
    );

    // ── 场景 3：重复复制去重（bump 置顶而非新增）──
    cleanup.assert_running("去重");
    let count_before = count_active_unfavorited(&db_path);
    retry_clipboard(|| clipboard.set_text(marker.clone()));
    thread::sleep(Duration::from_secs(2));
    assert_eq!(
        count_active_unfavorited(&db_path),
        count_before,
        "重复复制相同文本应去重置顶，不得新增条目"
    );

    // ── 场景 4：图片录入（CF 读取 → 解码 → 落盘 images/）──
    cleanup.assert_running("图片录入");
    let mut pixels = Vec::with_capacity(8 * 8 * 4);
    for _ in 0..8 * 8 {
        pixels.extend_from_slice(&[255, 0, 0, 255]);
    }
    retry_clipboard(|| {
        clipboard.set_image(arboard::ImageData {
            width: 8,
            height: 8,
            bytes: std::borrow::Cow::Owned(pixels.clone()),
        })
    });
    assert!(
        wait_until(Duration::from_secs(10), Duration::from_millis(200), || {
            count_rows(
                &db_path,
                "SELECT COUNT(*) FROM clip_items WHERE deleted_at IS NULL AND type = 'image'",
                None,
            ) > 0
        }),
        "复制图片后应录入图片条目"
    );
    let images_written = wait_until(Duration::from_secs(5), Duration::from_millis(200), || {
        fs::read_dir(app_dir.join("images"))
            .map(|entries| entries.filter_map(Result::ok).count() > 0)
            .unwrap_or(false)
    });
    assert!(images_written, "图片条目应把文件写入 images/ 存储");

    // ── 场景 5：二次启动经唤醒事件开关速贴面板（屏内 ↔ 停屏）──
    cleanup.assert_running("唤醒面板");
    wake_via_second_launch(&envs);
    assert!(
        wait_until(
            Duration::from_secs(10),
            Duration::from_millis(150),
            picker_onscreen
        ),
        "唤醒事件后速贴面板应落位屏内"
    );
    wake_via_second_launch(&envs);
    assert!(
        wait_until(Duration::from_secs(10), Duration::from_millis(150), || {
            !picker_onscreen()
        }),
        "再次唤醒应切换关闭面板（平移回屏外停屏位）"
    );

    // ── 场景 6：托盘切换监听（暂停即停录；去抖窗口内恢复一次即生效）──
    // 回归：旧壳移植的去抖闸门曾把暂停后 1.5 秒内的恢复当重复事件吞掉
    // （「要点两次才恢复」）。模态菜单点击无法可靠自动化，经命名事件走
    // 与托盘菜单选择完全相同的 toggle 路径（翻转落库 + 运行时联动）
    cleanup.assert_running("切换监听");
    let toggle_monitoring = move || {
        assert!(
            floatpaste_core::platform::windows::single_instance::signal_test_toggle_monitoring(
                &instance_id_for_toggle
            ),
            "发送切换监听事件失败（被测实例未装载钩子）"
        );
    };
    // 暂停 → 确认落库：确认即代表上一次切换已受理，此后立刻恢复，
    // 间隔压在旧去抖窗口（1.5s）内才能回归「第二次被吞」
    toggle_monitoring();
    assert!(
        wait_until(Duration::from_secs(5), Duration::from_millis(50), || {
            monitoring_paused(&db_path)
        }),
        "托盘切换后 pause_monitoring 应翻转为 true 并落库"
    );
    toggle_monitoring();
    assert!(
        wait_until(Duration::from_secs(5), Duration::from_millis(50), || {
            !monitoring_paused(&db_path)
        }),
        "暂停后立即（旧去抖窗口 1.5s 内）再次切换应恢复监听，不得被吞"
    );
    // 行为验证：暂停期间复制不入库、恢复后复制恢复录入（监听停启是
    // 处理时读设置即时判断，无需等待异步启停）
    toggle_monitoring();
    assert!(
        wait_until(Duration::from_secs(5), Duration::from_millis(50), || {
            monitoring_paused(&db_path)
        }),
        "再次暂停应翻转落库"
    );
    let paused_marker = format!("e2e-paused-{stamp}");
    retry_clipboard(|| clipboard.set_text(paused_marker.clone()));
    thread::sleep(Duration::from_secs(2));
    assert!(
        !active_full_text_exists(&db_path, &paused_marker),
        "暂停监听期间复制的内容不应录入"
    );
    toggle_monitoring();
    assert!(
        wait_until(Duration::from_secs(5), Duration::from_millis(50), || {
            !monitoring_paused(&db_path)
        }),
        "恢复监听应翻转落库"
    );
    let resumed_marker = format!("e2e-resumed-{stamp}");
    retry_clipboard(|| clipboard.set_text(resumed_marker.clone()));
    assert!(
        wait_until(Duration::from_secs(10), Duration::from_millis(200), || {
            active_full_text_exists(&db_path, &resumed_marker)
        }),
        "恢复监听后复制的内容应重新录入"
    );

    // ── 场景 7：搜索窗顶边锚定（独立实例；落地收缩不移动上边沿）──
    // 回归：开窗首落重居中标记曾因 changed 值相等哑火而悬置，泄漏到后续
    // 高度变化时触发「以新尺寸重居中」，表现为筛选后高度变低、窗口跳回
    // 屏幕中心、顶边下移。矮数据集让「起步满高 → 落地收缩」必然发生，
    // 断言收缩后上边沿仍停在开窗居中基准上（顶边锚定）。独立实例隔离
    // 数据与信号，不与主场景链互扰
    {
        let search_stamp = format!("e2e-search-{stamp}");
        let search_root: PathBuf =
            std::env::temp_dir().join(format!("floatpaste-e2e-search-{stamp}"));
        let search_app_dir = search_root.join("com.floatpaste");
        fs::create_dir_all(&search_app_dir).unwrap();
        {
            let repository =
                SqliteRepository::new(&search_app_dir.join("floatpaste.db")).unwrap();
            for index in 0..3 {
                let item =
                    NormalizeService::normalize_text(&format!("e2e-search-row-{index}"), None)
                        .unwrap();
                repository.save_text_item(&item).unwrap();
            }
        }
        let search_envs = [
            (
                "APPDATA",
                search_root
                    .to_str()
                    .expect("临时目录路径应为 UTF-8")
                    .to_string(),
            ),
            ("FLOATPASTE_INSTANCE_ID", search_stamp.clone()),
            ("FLOATPASTE_SKIP_AUTOSTART_SYNC", "1".to_string()),
        ];
        let mut search_cleanup = Cleanup {
            child: Some(spawn_instance(&search_envs, true)),
            instance_id: search_stamp.clone(),
            original_text: None,
            data_root: search_root,
        };

        // 打开搜索窗：钩子在装配段装载，早于装载的信号打不开事件，轮询重试
        search_cleanup.assert_running("打开搜索窗");
        assert!(
            wait_until(Duration::from_secs(15), Duration::from_millis(200), || {
                floatpaste_core::platform::windows::single_instance::signal_test_open_search(
                    &search_stamp,
                )
            }),
            "打开搜索信号应送达被测实例（钩子装载后必然成功）"
        );

        // 屏内落位（首开无历史基线：起步高度=满高，按光标所在工作区居中）
        assert!(
            wait_until(Duration::from_secs(10), Duration::from_millis(150), || {
                onscreen_search_window().is_some()
            }),
            "搜索窗应落位屏内"
        );
        let (hwnd, _) = onscreen_search_window().unwrap();
        let full_height = search_window_full_height(hwnd);
        let baseline_top = expected_search_top_at_full_height(full_height);

        // 落地收缩到 3 行：起步满高 620 逻辑，落地整档更矮（阈值 550 逻辑）
        assert!(
            wait_until(Duration::from_secs(10), Duration::from_millis(150), || {
                onscreen_search_window()
                    .map(|(_, rect)| (rect.bottom - rect.top) < full_height * 550 / 620)
                    .unwrap_or(false)
            }),
            "矮数据集落地后搜索窗高度应收缩到满高以下"
        );
        let (_, shrunk) = onscreen_search_window().unwrap();
        assert_eq!(
            shrunk.top, baseline_top,
            "落地收缩不得移动上边沿（顶边锚定，保持开窗居中基准）"
        );
    }

    // 收尾（终止被测实例、恢复剪贴板、清理临时目录）由 Cleanup 守卫统一执行
}
