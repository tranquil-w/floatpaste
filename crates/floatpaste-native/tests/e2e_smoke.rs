//! L3 真进程端到端冒烟：spawn 真实二进制，靠隔离钩子把被测实例的数据与
//! 系统副作用圈在临时目录内（APPDATA 重定向隔离数据库/图片/日志；
//! FLOATPASTE_INSTANCE_ID 隔离单实例互斥量与唤醒事件；
//! FLOATPASTE_RETENTION_FIRST_DELAY_MS 把首轮保留清理缩短到秒级；
//! FLOATPASTE_SKIP_AUTOSTART_SYNC 挡住对真实系统 Run 键与任务计划的同步）。
//!
//! 外部驱动只走真实用户路径：写系统剪贴板触发录入、二次启动进程经唤醒
//! 事件开关速贴面板、命名事件驱动托盘「切换监听」「打开搜索」与搜索窗
//! Esc 关闭（key-close 回调，与按键经键位解析后的终点相同）的处理路径；
//! 断言落在隔离库（只读连接）与窗口几何/前台归属上。
//! 上屏注入（SendInput）依赖交互桌面与目标应用，不属于此冒烟测试的覆盖范围。
//!
//! 单测试函数串行推进：系统剪贴板、全局热键、速贴窗口都是会话级全局
//! 资源，不可并行。运行期间被测实例会短暂占用 Ctrl+Q/Alt+S 并读写
//! 剪贴板，结束时尽力恢复原文本。

use std::{
    fs,
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    sync::{atomic::AtomicBool, atomic::Ordering, Arc},
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
use floatpaste_core::{
    domain::{
        clip_item::{ClipType, PasteMode, SearchFilters, SearchQuery, SearchSort},
        error::AppError,
    },
    platform::windows::{
        clipboard_formats::{write_text_formats, ClipboardSnapshot},
        file_clipboard::write_file_paths_to_clipboard,
    },
    services::{
        history_service::HistoryService,
        html_format::{encode_html_fragment, MAX_HTML_BYTES},
        image_storage::ImageStorage,
        paste_support::{publish_item, schedule_clipboard_restore},
    },
    state::CoreState,
};
use windows::{
    core::{BOOL, PCWSTR},
    Win32::{
        Foundation::{HWND, LPARAM, LRESULT, RECT, WPARAM},
        Graphics::Gdi::{GetSysColorBrush, COLOR_WINDOW},
        System::DataExchange::{CloseClipboard, OpenClipboard},
        System::LibraryLoader::GetModuleHandleW,
        UI::{
            HiDpi::GetDpiForWindow,
            WindowsAndMessaging::{
                CreateWindowExW, DefWindowProcW, DestroyWindow, DispatchMessageW, EnumWindows,
                GetForegroundWindow, GetWindowRect, GetWindowTextW, GetWindowThreadProcessId,
                IsWindowVisible, PeekMessageW, RegisterClassW, ShowWindow, TranslateMessage,
                UnregisterClassW, MSG, PM_REMOVE, SW_SHOW, WINDOW_EX_STYLE, WNDCLASSW,
                WS_OVERLAPPEDWINDOW,
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
fn retry_clipboard<T, E: std::fmt::Debug>(mut op: impl FnMut() -> Result<T, E>) -> T {
    let mut last = None;
    for _ in 0..15 {
        match op() {
            Ok(value) => return value,
            Err(error) => last = Some(error),
        }
        thread::sleep(Duration::from_millis(200));
    }
    panic!("剪贴板操作短重试后仍失败: {last:?}；保留隔离目录与实例日志，区分占用、格式转换和实例行为，排查步骤见 docs/testing.md");
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
fn picker_onscreen(process_id: u32) -> bool {
    onscreen_window(PICKER_TITLE, process_id).is_some()
}

struct WindowEnumState {
    title: &'static str,
    process_id: u32,
    found: Option<(isize, RECT)>,
}

unsafe extern "system" fn enum_onscreen_window_proc(hwnd: HWND, lparam: LPARAM) -> BOOL {
    let state = &mut *(lparam.0 as *mut WindowEnumState);
    let mut process_id = 0;
    GetWindowThreadProcessId(hwnd, Some(&mut process_id));
    if process_id != state.process_id {
        return BOOL::from(true);
    }
    let mut text = [0u16; 64];
    let len = GetWindowTextW(hwnd, &mut text);
    if len > 0 && String::from_utf16_lossy(&text[..len as usize]) == state.title {
        let mut rect = RECT::default();
        // 屏内过滤必须叠加可见性：winit 预建的隐藏窗口自进程启动起就停在
        // 级联默认位（CI 实测 top 130~156），矩形上过得了停屏位阈值；不看
        // IsWindowVisible 会把「尚未装配/落位」的隐藏窗口当成已落位，测试
        // 与应用赛跑（CI 实测测试先赢：应用随即将窗正确落位 top=50，断言
        // 却已拿默认位 130 失败）。装配后的搜索窗走 Slint show + 停屏平移，
        // Win32 恒可见，可见性过滤不影响正常路径
        if GetWindowRect(hwnd, &mut rect).is_ok()
            && rect.left > OFFSCREEN_THRESHOLD
            && rect.top > OFFSCREEN_THRESHOLD
            && IsWindowVisible(hwnd).as_bool()
        {
            state.found = Some((hwnd.0 as isize, rect));
            return BOOL::from(false);
        }
    }
    BOOL::from(true)
}

/// 按进程、标题与屏内可见性查找，隔离日常实例及同场景的其他被测实例。
fn onscreen_window(title: &'static str, process_id: u32) -> Option<(isize, RECT)> {
    let mut state = WindowEnumState {
        title,
        process_id,
        found: None,
    };
    unsafe {
        let _ = EnumWindows(
            Some(enum_onscreen_window_proc),
            LPARAM(&mut state as *mut _ as isize),
        );
    }
    state.found
}

fn onscreen_search_window(process_id: u32) -> Option<(isize, RECT)> {
    onscreen_window(SEARCH_TITLE, process_id)
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
    let work = match current_cursor_point().and_then(|cursor| work_area_from_point(cursor)) {
        Ok(work) => work,
        Err(error) => {
            eprintln!("[e2e] 光标/工作区不可得（{error}），基准退回主屏工作区");
            primary_monitor_work_area().expect("主屏工作区不可得")
        }
    };
    center_in_work_area(work, 0, full_height).y
}

/// Esc 焦点还原断言的目标窗口：测试进程创建的真实可见、屏外顶层窗口。搜索窗
/// 打开瞬间捕获当前前台为会话目标，关闭后「前台还原给目标」的断言要求
/// 该目标确定可控。无人值守会话（前台锁定策略拒绝转让，CI 断连环境必现）
/// 拿不到前台时返回 None，调用方跳过焦点段——焦点语义只能在交互桌面
/// 验证，窗口几何断言不受影响照常执行
struct FocusTargetGuard {
    hwnd: isize,
}

unsafe extern "system" fn focus_target_wndproc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    DefWindowProcW(hwnd, msg, wparam, lparam)
}

/// 泵掉目标窗口的待处理消息（绘制等）：不泵则 SetForegroundWindow 后的
/// 前台归属可能滞后于可见性建立
fn pump_window_messages(hwnd: HWND) {
    unsafe {
        let mut msg = MSG::default();
        while PeekMessageW(&mut msg, Some(hwnd), 0, 0, PM_REMOVE).as_bool() {
            let _ = TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
    }
}

impl FocusTargetGuard {
    fn acquire() -> Option<Self> {
        unsafe {
            let module = GetModuleHandleW(None).ok()?;
            let class_name: Vec<u16> = "FloatPasteE2ETarget"
                .encode_utf16()
                .chain(std::iter::once(0))
                .collect();
            let title: Vec<u16> = "FloatPaste · E2E 目标窗口"
                .encode_utf16()
                .chain(std::iter::once(0))
                .collect();
            let class = WNDCLASSW {
                lpfnWndProc: Some(focus_target_wndproc),
                hInstance: module.into(),
                hbrBackground: GetSysColorBrush(COLOR_WINDOW),
                lpszClassName: PCWSTR::from_raw(class_name.as_ptr()),
                ..Default::default()
            };
            if RegisterClassW(&class) == 0 {
                return None;
            }
            let created = CreateWindowExW(
                WINDOW_EX_STYLE(0),
                PCWSTR::from_raw(class_name.as_ptr()),
                PCWSTR::from_raw(title.as_ptr()),
                WS_OVERLAPPEDWINDOW,
                -32000,
                -32000,
                240,
                160,
                None,
                None,
                Some(module.into()),
                None,
            );
            let Ok(hwnd) = created else {
                let _ =
                    UnregisterClassW(PCWSTR::from_raw(class_name.as_ptr()), Some(module.into()));
                return None;
            };
            let _ = ShowWindow(hwnd, SW_SHOW);
            // 测试进程未必是前台进程（cargo 经工具链/CI 拉起，前台在终端/
            // IDE 上或无人值守会话无前台），裸 SetForegroundWindow 受前台锁
            // 拒绝——复用被测实例同款 AttachThreadInput 抢前台手法
            floatpaste_core::platform::windows::active_app::ActiveAppResolver::force_foreground_window(
                hwnd.0 as isize,
            );
            let owned = wait_until(Duration::from_secs(3), Duration::from_millis(50), || {
                pump_window_messages(hwnd);
                GetForegroundWindow() == hwnd
            });
            if !owned {
                let _ = DestroyWindow(hwnd);
                let _ =
                    UnregisterClassW(PCWSTR::from_raw(class_name.as_ptr()), Some(module.into()));
                return None;
            }
            Some(Self {
                hwnd: hwnd.0 as isize,
            })
        }
    }
}

impl Drop for FocusTargetGuard {
    fn drop(&mut self) {
        unsafe {
            let _ = DestroyWindow(HWND(self.hwnd as *mut _));
            let class_name: Vec<u16> = "FloatPasteE2ETarget"
                .encode_utf16()
                .chain(std::iter::once(0))
                .collect();
            let module = GetModuleHandleW(None).unwrap_or_default();
            let _ = UnregisterClassW(PCWSTR::from_raw(class_name.as_ptr()), Some(module.into()));
        }
    }
}

/// 停屏浮层装配闪现守卫：启动到首次主动打开速贴前，主实例的全部
/// 业务窗口必须停在屏外或隐藏；采样可见性和位置，防止先 show 后停屏。
/// 活跃会话允许合法悬浮气泡，因此首次打开速贴前收口。
struct RogueWindowGuard {
    hit: Arc<AtomicBool>,
    stop: Arc<AtomicBool>,
    worker: Option<thread::JoinHandle<()>>,
}

struct RogueScanState {
    process_id: u32,
    hit: Arc<AtomicBool>,
}

unsafe extern "system" fn rogue_window_proc(hwnd: HWND, lparam: LPARAM) -> BOOL {
    let state = &mut *(lparam.0 as *mut RogueScanState);
    let mut process_id = 0;
    GetWindowThreadProcessId(hwnd, Some(&mut process_id));
    if process_id != state.process_id {
        return BOOL::from(true);
    }
    let mut text = [0u16; 64];
    let len = GetWindowTextW(hwnd, &mut text);
    if len > 0 {
        let title = String::from_utf16_lossy(&text[..len as usize]);
        if title.starts_with("FloatPaste") || title.eq_ignore_ascii_case("floatpaste") {
            let mut rect = RECT::default();
            if IsWindowVisible(hwnd).as_bool()
                && GetWindowRect(hwnd, &mut rect).is_ok()
                && rect.left > OFFSCREEN_THRESHOLD
                && rect.top > OFFSCREEN_THRESHOLD
            {
                eprintln!("[e2e] 主实例停屏浮层意外上屏: {title}");
                state.hit.store(true, Ordering::SeqCst);
            }
        }
    }
    BOOL::from(true)
}

impl RogueWindowGuard {
    fn start(process_id: u32) -> Self {
        let hit = Arc::new(AtomicBool::new(false));
        let stop = Arc::new(AtomicBool::new(false));
        let hit_for_worker = hit.clone();
        let stop_for_worker = stop.clone();
        let worker = thread::spawn(move || {
            while !stop_for_worker.load(Ordering::Relaxed) {
                let mut state = RogueScanState {
                    process_id,
                    hit: hit_for_worker.clone(),
                };
                unsafe {
                    let _ = EnumWindows(
                        Some(rogue_window_proc),
                        LPARAM(&mut state as *mut _ as isize),
                    );
                }
                thread::sleep(Duration::from_millis(15));
            }
        });
        Self {
            hit,
            stop,
            worker: Some(worker),
        }
    }

    fn assert_no_rogue_window(self) {
        self.stop.store(true, Ordering::SeqCst);
        if let Some(worker) = self.worker {
            let _ = worker.join();
        }
        assert!(
            !self.hit.load(Ordering::SeqCst),
            "主实例在首次主动打开前的业务窗口不得屏内可见——\
             装配顺序回归：silent_assemble_* 必须在 show 前先停屏，\
             否则窗口在 winit 级联默认位闪现（overlay.rs）"
        );
    }
}

fn spawn_instance(envs: &[(&str, String)], silent: bool) -> Child {
    // 发布验收复用同一业务链路，直接启动安装后或解压后的交付程序。
    let executable = std::env::var_os("FLOATPASTE_E2E_EXE")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(env!("CARGO_BIN_EXE_floatpaste")));
    let mut command = Command::new(executable);
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
    original_clipboard: Option<ClipboardSnapshot>,
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
        for _ in 0..5 {
            if self
                .original_clipboard
                .as_ref()
                .is_none_or(|snapshot| snapshot.restore(None, None).is_ok())
            {
                break;
            }
            thread::sleep(Duration::from_millis(200));
        }
        // 诊断开关：FLOATPASTE_E2E_KEEP=1 时保留临时目录（实例日志在
        // floatpaste/logs），供间歇失败的事后取证；平时不留痕
        if std::env::var("FLOATPASTE_E2E_KEEP").as_deref() != Ok("1") {
            // 句柄延迟释放时删除失败不视作测试失败，留给系统临时目录自清
            let _ = fs::remove_dir_all(&self.data_root);
        } else {
            eprintln!(
                "[e2e] FLOATPASTE_E2E_KEEP=1，临时目录已保留: {}",
                self.data_root.display()
            );
        }
    }
}

#[test]
fn app_e2e_smoke_flow() {
    // 前置：环境剪贴板已锁死/被长占用时跳过（探测时序保证责任不在
    // 被测物，见 clipboard_unusable_before_start）
    if clipboard_unusable_before_start() {
        assert!(
            std::env::var("FLOATPASTE_E2E_STRICT").as_deref() != Ok("1"),
            "发布验收要求实际执行 E2E；系统剪贴板不可用，禁止跳过后发布"
        );
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
    let app_dir = data_root.join("floatpaste");
    fs::create_dir_all(&app_dir).unwrap();
    let db_path = app_dir.join("floatpaste.db");

    {
        let repository = SqliteRepository::new(&db_path).unwrap();
        let mut settings = UserSetting::default();
        settings.history_limit = HISTORY_LIMIT;
        settings.picker_position_mode = PickerPositionMode::LastPosition;
        // 录入断言以「复制」为驱动，采集来源进程取决于运行宿主：CI 的测试
        // 进程由终端拉起、前台即终端，默认排除表含终端——不清空会让
        // marker 撞排除被静默过滤，录入链路在 CI 上必失败
        settings.excluded_apps.clear();
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
    let original_clipboard = retry_clipboard(|| ClipboardSnapshot::capture(true));
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
    // 场景 6/6b 托盘、速贴收藏与删除事件的信号侧副本（Cleanup 构造会 move 走原值）
    let instance_id_for_toggle = instance_id.clone();
    let instance_id_for_signals = instance_id.clone();
    let mut cleanup = Cleanup {
        child: Some(spawn_instance(&envs, true)),
        instance_id,
        original_clipboard: Some(original_clipboard),
        data_root: data_root.clone(),
    };
    let picker_pid = cleanup.child.as_ref().unwrap().id();
    // 监测启动装配到首次主动打开速贴；活跃会话可能合法显示悬浮气泡。
    let rogue_windows = RogueWindowGuard::start(picker_pid);

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
        "复制文本后应经剪贴板监听录入；当前前台进程={:?}，剪贴板仍为测试内容={}",
        floatpaste_core::platform::windows::active_app::ActiveAppResolver::current_foreground_process_name(),
        clipboard.get_text().is_ok_and(|text| text == marker)
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

    // 多格式采集跨进程持久化；快照/发布契约串行使用同一剪贴板隔离范围。
    let html_text = format!("e2e-html-中文😀-{stamp}");
    let html_bytes = encode_html_fragment(&format!("<p><b>{html_text}</b></p>"));
    retry_clipboard(|| write_text_formats(&html_text, Some(&html_bytes), None));
    assert!(
        wait_until(Duration::from_secs(10), Duration::from_millis(200), || {
            count_rows(&db_path, "SELECT count(*) FROM clip_items ci JOIN clip_text_formats f ON f.item_id = ci.id WHERE ci.full_text = ?1 AND ci.deleted_at IS NULL", Some(&html_text)) == 1
        }),
        "HTML 与纯文本应作为同一条目持久化"
    );
    let snapshot = retry_clipboard(|| ClipboardSnapshot::capture(true));
    assert_eq!(snapshot.html(), Some(html_bytes.as_slice()));
    let repository = SqliteRepository::new(&db_path).unwrap();
    let persisted = repository
        .search(SearchQuery {
            keyword: html_text.clone(),
            filters: Default::default(),
            offset: 0,
            limit: 10,
            sort: SearchSort::RecentDesc,
        })
        .unwrap();
    let rich = repository
        .get_item_for_paste(&persisted.items[0].id)
        .unwrap();
    assert_eq!(rich.html_bytes, Some(html_bytes.clone()));
    let driver = CoreState::new(
        repository,
        ImageStorage::new(app_dir.clone()).unwrap(),
        UserSetting::default(),
    );
    // 复用真正上屏的发布逻辑，只验证写回、不向外部应用注入按键。
    retry_clipboard(|| publish_item(&driver, &rich, PasteMode::Original, None, Some(&snapshot)));
    assert!(
        HistoryService::ingest_text_formats(&driver, &html_text, Some(html_bytes.clone()), None)
            .unwrap()
            .is_none(),
        "发布须登记完整格式身份"
    );
    let plain_sequence = retry_clipboard(|| {
        publish_item(&driver, &rich, PasteMode::PlainText, None, Some(&snapshot))
    });
    assert!(retry_clipboard(|| { ClipboardSnapshot::capture(true) })
        .html()
        .is_none());
    assert!(
        HistoryService::ingest_text(&driver, &html_text, None)
            .unwrap()
            .is_none(),
        "发布须登记纯文本回退身份"
    );
    schedule_clipboard_restore(&driver, snapshot.clone(), None, 0, 1, plain_sequence).unwrap();
    let newer = format!("e2e-user-new-copy-{stamp}");
    retry_clipboard(|| clipboard.set_text(newer.clone()));
    thread::sleep(Duration::from_millis(700));
    assert_eq!(
        retry_clipboard(|| clipboard.get_text()),
        newer,
        "延迟恢复不得覆盖用户的新复制"
    );
    assert!(retry_clipboard(|| snapshot.restore(None, Some(plain_sequence))).is_none());
    let sequence = retry_clipboard(|| write_text_formats(&html_text, None, None));
    assert!(retry_clipboard(|| snapshot.restore(None, Some(sequence))).is_some());
    assert_eq!(
        retry_clipboard(|| { ClipboardSnapshot::capture(true) }).html(),
        Some(html_bytes.as_slice())
    );
    let extracted_marker = format!("e2e-html-only-中文😀-{stamp}");
    let html_only = encode_html_fragment(&format!("<b>{extracted_marker}</b>"));
    retry_clipboard(|| write_text_formats("", Some(&html_only), None));
    assert!(
        wait_until(Duration::from_secs(10), Duration::from_millis(100), || {
            active_full_text_exists(&db_path, &extracted_marker)
        }),
        "无可用系统纯文本时应从有效 HTML 片段提取文字"
    );
    let oversized_fallback = format!("e2e-html-oversized-{stamp}");
    let oversized = format!("<p>{}</p>", "x".repeat(MAX_HTML_BYTES));
    retry_clipboard(|| clipboard.set_html(oversized.as_str(), Some(oversized_fallback.as_str())));
    assert!(
        wait_until(Duration::from_secs(10), Duration::from_millis(100), || {
            active_full_text_exists(&db_path, &oversized_fallback)
        }),
        "超限 HTML 应保留可用纯文本"
    );
    let over_limit_error = retry_clipboard(|| match ClipboardSnapshot::capture(true) {
        Err(AppError::Clipboard(message)) if message.contains("4 MiB") => Ok(message),
        Err(error) => Err(error),
        Ok(_) => panic!("恢复快照不得接受超限 HTML"),
    });
    assert!(over_limit_error.contains("取消"));
    assert_eq!(retry_clipboard(|| clipboard.get_text()), oversized_fallback);
    let paths = vec![db_path.to_string_lossy().into_owned()];
    retry_clipboard(|| write_file_paths_to_clipboard(&paths, None, None));
    // 后续发布契约复用监听录入的文件条目；必须等后台采集完成再覆盖剪贴板。
    assert!(
        wait_until(Duration::from_secs(10), Duration::from_millis(100), || {
            count_rows(
                &db_path,
                "SELECT count(*) FROM clip_items WHERE type = 'file' AND deleted_at IS NULL",
                None,
            ) == 1
        }),
        "文件样本应经剪贴板监听录入后再验证发布"
    );
    let files_snapshot = retry_clipboard(|| ClipboardSnapshot::capture(true));
    assert_eq!(files_snapshot.files.as_ref(), Some(&paths));
    retry_clipboard(|| clipboard.set_text("文件快照恢复前"));
    assert!(retry_clipboard(|| files_snapshot.restore(None, None)).is_some());
    assert_eq!(
        retry_clipboard(|| { ClipboardSnapshot::capture(true) })
            .files
            .as_ref(),
        Some(&paths)
    );

    // 文件条目的路径/文件名文本形态：均逐行（CRLF）作为文本发布，
    // 并登记自写抑制身份（发布内容不得再入库）
    let file_items = driver
        .repository
        .search(SearchQuery {
            keyword: String::new(),
            filters: SearchFilters {
                clip_type: Some(ClipType::File),
                ..Default::default()
            },
            offset: 0,
            limit: 10,
            sort: SearchSort::RecentDesc,
        })
        .unwrap();
    let file_detail = driver
        .repository
        .get_item_for_paste(&file_items.items[0].id)
        .unwrap();
    retry_clipboard(|| publish_item(&driver, &file_detail, PasteMode::PathText, None, None));
    assert_eq!(
        retry_clipboard(|| clipboard.get_text()),
        paths.join("\r\n"),
        "完整路径形态应逐行发布路径"
    );
    retry_clipboard(|| publish_item(&driver, &file_detail, PasteMode::FileNameText, None, None));
    let expected_names: Vec<String> = paths
        .iter()
        .map(|path| {
            std::path::Path::new(path)
                .file_name()
                .map(|name| name.to_string_lossy().into_owned())
                .unwrap_or_else(|| path.clone())
        })
        .collect();
    assert_eq!(
        retry_clipboard(|| clipboard.get_text()),
        expected_names.join("\r\n"),
        "文件名形态应逐行只发布文件名"
    );
    assert!(
        HistoryService::ingest_text(&driver, &expected_names.join("\r\n"), None)
            .unwrap()
            .is_none(),
        "文件名发布须登记自写抑制身份"
    );
    drop(driver);

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

    let image_snapshot = retry_clipboard(|| ClipboardSnapshot::capture(true));
    let image = image_snapshot.image().expect("图片快照应可解码");
    retry_clipboard(|| {
        floatpaste_core::platform::windows::image_clipboard::write_image_to_clipboard(
            None,
            &image,
            Some(&image_snapshot),
        )
    });
    let restored_image = retry_clipboard(|| ClipboardSnapshot::capture(true))
        .image()
        .unwrap();
    assert_eq!(
        restored_image.rgba, image.rgba,
        "PNG/DIB 共用发布器应保留像素"
    );

    // 图片文件形态：保存的 PNG 以文件列表（CF_HDROP）发布，目标应用按
    // 文件附件接收；发布的文件必须真实存在
    let image_driver = CoreState::new(
        SqliteRepository::new(&db_path).unwrap(),
        ImageStorage::new(app_dir.clone()).unwrap(),
        UserSetting::default(),
    );
    let image_items = image_driver
        .repository
        .search(SearchQuery {
            keyword: String::new(),
            filters: SearchFilters {
                clip_type: Some(ClipType::Image),
                ..Default::default()
            },
            offset: 0,
            limit: 10,
            sort: SearchSort::RecentDesc,
        })
        .unwrap();
    let image_detail = image_driver
        .repository
        .get_item_for_paste(&image_items.items[0].id)
        .unwrap();
    retry_clipboard(|| {
        publish_item(
            &image_driver,
            &image_detail,
            PasteMode::ImageFile,
            None,
            None,
        )
    });
    let image_file_snapshot = retry_clipboard(|| ClipboardSnapshot::capture(true));
    let published_image_files = image_file_snapshot
        .files
        .expect("图片文件形态应以 CF_HDROP 文件列表发布");
    assert_eq!(published_image_files.len(), 1, "应只发布该图片的保存文件");
    assert!(
        published_image_files[0].to_lowercase().ends_with(".png"),
        "发布引用的应是保存的 PNG：{}",
        published_image_files[0]
    );
    assert!(
        std::path::Path::new(&published_image_files[0]).is_file(),
        "发布引用的图片文件必须存在"
    );
    drop(image_driver);

    // ── 场景 5：二次启动经唤醒事件开关速贴面板（屏内 ↔ 停屏）──
    cleanup.assert_running("唤醒面板");
    rogue_windows.assert_no_rogue_window();
    wake_via_second_launch(&envs);
    assert!(
        wait_until(Duration::from_secs(10), Duration::from_millis(150), || {
            picker_onscreen(picker_pid)
        }),
        "唤醒事件后速贴面板应落位屏内"
    );
    // 坐标与 WS_VISIBLE 不能证明用户看得到面板：后台冷启动时必须持续
    // 位于置顶带，不能只在 SetWindowPos 后短暂带位、随样式重排落回底部。
    let mut topmost_samples = 0;
    assert!(
        wait_until(Duration::from_secs(3), Duration::from_millis(50), || {
            let topmost = onscreen_window(PICKER_TITLE, picker_pid).is_some_and(|(hwnd, _)| {
                floatpaste_core::platform::windows::window_control::is_topmost(hwnd)
            });
            topmost_samples = if topmost { topmost_samples + 1 } else { 0 };
            topmost_samples >= 6
        }),
        "未打开设置的冷启动速贴应持续置顶，不能只有屏内几何而被其他窗口遮挡"
    );
    wake_via_second_launch(&envs);
    assert!(
        wait_until(Duration::from_secs(10), Duration::from_millis(150), || {
            !picker_onscreen(picker_pid)
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

    // ── 场景 6b：速贴收藏切换（选中行 = 列表首行 = 最新条目）──
    // 收藏是键盘/点击路径，经命名事件触发 toggle_favorite（与会话键处理后
    // 的终点相同）。开面板即选中 0（refresh_list_reset），列表按活动时间
    // 排序，选中条目即 resumed_marker
    cleanup.assert_running("收藏切换");
    wake_via_second_launch(&envs);
    assert!(
        wait_until(Duration::from_secs(10), Duration::from_millis(150), || {
            picker_onscreen(picker_pid)
        }),
        "收藏场景前置：唤醒事件后速贴面板应落位屏内"
    );
    let assert_favorited = |text: &str, expected: bool, stage: &str| {
        assert!(
            wait_until(Duration::from_secs(5), Duration::from_millis(100), || {
                count_rows(
                    &db_path,
                    "SELECT is_favorited FROM clip_items WHERE deleted_at IS NULL AND full_text = ?1",
                    Some(text),
                ) == i64::from(expected)
            }),
            "{stage}: 条目「{text}」的收藏态应为 {expected}"
        );
    };
    assert!(
        floatpaste_core::platform::windows::single_instance::signal_test_picker_favorite(
            &instance_id_for_signals
        ),
        "发送速贴收藏信号失败（被测实例未装载钩子）"
    );
    assert_favorited(&resumed_marker, true, "首次收藏");
    thread::sleep(Duration::from_millis(400));
    assert!(
        floatpaste_core::platform::windows::single_instance::signal_test_picker_favorite(
            &instance_id_for_signals
        ),
        "发送速贴取消收藏信号失败"
    );
    assert_favorited(&resumed_marker, false, "再次取消");

    // 同一速贴会话内两段式删除：第一击保留条目，第二击软删且面板继续显示。
    assert!(
        floatpaste_core::platform::windows::single_instance::signal_test_picker_delete(
            &instance_id_for_signals
        )
    );
    thread::sleep(Duration::from_millis(400));
    assert_eq!(
        count_rows(
            &db_path,
            "SELECT COUNT(*) FROM clip_items WHERE deleted_at IS NULL AND full_text = ?1",
            Some(&resumed_marker)
        ),
        1,
        "首次速贴删除只进入待确认"
    );
    assert!(
        floatpaste_core::platform::windows::single_instance::signal_test_picker_delete(
            &instance_id_for_signals
        )
    );
    assert!(
        wait_until(Duration::from_secs(5), Duration::from_millis(100), || {
            count_rows(
                &db_path,
                "SELECT COUNT(*) FROM clip_items WHERE deleted_at IS NOT NULL AND full_text = ?1",
                Some(&resumed_marker),
            ) == 1
        }),
        "再次确认后应软删除条目"
    );
    assert!(picker_onscreen(picker_pid), "删除后速贴应保持打开");
    wake_via_second_launch(&envs);
    assert!(
        wait_until(Duration::from_secs(10), Duration::from_millis(150), || {
            !picker_onscreen(picker_pid)
        }),
        "收藏场景收尾：应关闭面板（平移回屏外停屏位）"
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
        let search_app_dir = search_root.join("floatpaste");
        fs::create_dir_all(&search_app_dir).unwrap();
        {
            let repository = SqliteRepository::new(&search_app_dir.join("floatpaste.db")).unwrap();
            for index in 0..3 {
                let item =
                    NormalizeService::normalize_text(&format!("e2e-search-row-{index}"), None)
                        .unwrap();
                repository.save_text_item(&item).unwrap();
                // 活动时间戳 1ms 步进保证排序确定：场景 9 的删除按选中 0
                // （列表首行）定位目标，同毫秒并列时首行不确定
                thread::sleep(Duration::from_millis(1));
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
            original_clipboard: None,
            data_root: search_root,
        };
        let search_pid = search_cleanup.child.as_ref().unwrap().id();

        // 场景 8 前置：目标窗口持前台后再开窗，被测实例捕获的会话目标即它。
        // 无人值守会话拿不到前台（前台锁定策略）时场景 8 整段跳过
        let focus_target = FocusTargetGuard::acquire();
        if focus_target.is_none() {
            if std::env::var("GITHUB_ACTIONS").as_deref() == Ok("true") {
                println!(
                    "::warning::当前 runner 无交互前台；焦点还原未覆盖，其他 E2E 场景继续执行"
                );
            }
            eprintln!(
                "[e2e] 3 秒内未取得前台目标窗口（无人值守会话前台锁定策略），\n\
                 跳过 Esc 关闭焦点还原断言段；几何断言不受影响"
            );
        }

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
                onscreen_search_window(search_pid).is_some()
            }),
            "搜索窗应落位屏内"
        );
        let (hwnd, _) = onscreen_search_window(search_pid).unwrap();
        let full_height = search_window_full_height(hwnd);
        let baseline_top = expected_search_top_at_full_height(full_height);

        // 落地收缩到 3 行：起步满高 620 逻辑，落地整档更矮（阈值 550 逻辑）
        assert!(
            wait_until(Duration::from_secs(10), Duration::from_millis(150), || {
                onscreen_search_window(search_pid)
                    .map(|(_, rect)| (rect.bottom - rect.top) < full_height * 550 / 620)
                    .unwrap_or(false)
            }),
            "矮数据集落地后搜索窗高度应收缩到满高以下"
        );
        let (_, shrunk) = onscreen_search_window(search_pid).unwrap();
        assert_eq!(
            shrunk.top, baseline_top,
            "落地收缩不得移动上边沿（顶边锚定，保持开窗居中基准）"
        );

        // ── 场景 8：搜索窗 Esc 关闭还原会话目标前台 ──
        // 回归：Esc 关闭曾被归入「失焦自关」一档不还原前台——失焦自关的
        // 前提（用户点击处已持有前台）对键盘路径不成立，Esc 时前台在搜索
        // 窗自己手上，停屏后键盘焦点须显式还给会话目标。
        // 测试路径无 RegisterHotKey 的前台锁豁免（热键路径用户按键即显式
        // 输入事件，命名事件没有），被测实例打开搜索后拿不到前台是预期——
        // 由测试进程把前台交给搜索窗（跨进程 AttachThreadInput 手法），
        // 才能区分「显式还原给目标」与「停屏后前台自然流转」
        if let Some(target) = focus_target {
            let search_hwnd = hwnd;
            let handed =
                floatpaste_core::platform::windows::active_app::ActiveAppResolver::force_foreground_window(
                    search_hwnd,
                );
            assert!(
                handed
                    && wait_until(Duration::from_secs(5), Duration::from_millis(100), || {
                        pump_window_messages(HWND(search_hwnd as *mut _));
                        (unsafe { GetForegroundWindow().0 as isize }) == search_hwnd
                    }),
                "测试进程应能把前台交给搜索窗（Esc 还原的前置态）"
            );
            assert!(
                floatpaste_core::platform::windows::single_instance::signal_test_search_escape(
                    &search_stamp,
                ),
                "发送搜索 Esc 信号失败（被测实例未装载钩子）"
            );
            assert!(
                wait_until(Duration::from_secs(5), Duration::from_millis(100), || {
                    onscreen_search_window(search_pid).is_none()
                }),
                "Esc 后搜索窗应停屏（平移回屏外）"
            );
            assert!(
                wait_until(Duration::from_secs(5), Duration::from_millis(100), || {
                    pump_window_messages(HWND(target.hwnd as *mut _));
                    (unsafe { GetForegroundWindow().0 as isize }) == target.hwnd
                }),
                "Esc 关闭后前台应还原给会话目标窗口"
            );
        }

        // ── 场景 9：搜索窗两段式删除（选中行 = 列表首行 = 最新条目）──
        // 首次触发进入待确认态（库无变化），3 秒内再次触发同一目标才真删。
        // 删除目标=选中 0=列表首行=库中活动时间最新的未删条目（与列表排序
        // 同源；预置数据之外还可能有启动核对录入的剪贴板残留，故动态读取
        // 而非假设为最后一条预置数据）
        let delete_marker: String = {
            let connection = rusqlite::Connection::open_with_flags(
                search_app_dir.join("floatpaste.db"),
                rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
            )
            .unwrap();
            connection.busy_timeout(Duration::from_secs(5)).unwrap();
            connection
                .query_row(
                    "SELECT full_text FROM clip_items WHERE deleted_at IS NULL \
                     ORDER BY COALESCE(last_used_at, created_at) DESC, created_at DESC LIMIT 1",
                    [],
                    |row| row.get(0),
                )
                .expect("删除场景前置：库中应有未删条目")
        };
        let delete_soft_count = || {
            count_rows(
                search_app_dir.join("floatpaste.db").as_path(),
                "SELECT COUNT(*) FROM clip_items WHERE full_text = ?1 AND deleted_at IS NOT NULL",
                Some(&delete_marker),
            )
        };
        assert!(
            floatpaste_core::platform::windows::single_instance::signal_test_open_search(
                &search_stamp,
            ),
            "删除场景前置：打开搜索信号应送达"
        );
        assert!(
            wait_until(Duration::from_secs(10), Duration::from_millis(150), || {
                onscreen_search_window(search_pid).is_some()
            }),
            "删除场景前置：搜索窗应落位屏内"
        );
        // 等列表数据落地再发删除：落位先于异步加载（fetch_next），选中行
        // 此时读不到条目会静默返回；高度收缩完成 = 数据已入库渲染
        let full = search_window_full_height(hwnd);
        assert!(
            wait_until(Duration::from_secs(10), Duration::from_millis(150), || {
                onscreen_search_window(search_pid)
                    .map(|(_, rect)| (rect.bottom - rect.top) < full * 550 / 620)
                    .unwrap_or(false)
            }),
            "删除场景前置：搜索窗应收缩到结果高度（数据落地）"
        );
        assert!(
            floatpaste_core::platform::windows::single_instance::signal_test_search_delete(
                &search_stamp,
            ),
            "发送搜索删除信号失败（被测实例未装载钩子）"
        );
        thread::sleep(Duration::from_millis(300));
        assert_eq!(delete_soft_count(), 0, "首次删除只进入待确认态，不得软删");
        // 待确认窗口 3 秒，间隔 300ms 的第二次触发必落在窗口内
        assert!(
            floatpaste_core::platform::windows::single_instance::signal_test_search_delete(
                &search_stamp,
            ),
            "发送搜索确认删除信号失败"
        );
        assert!(
            wait_until(Duration::from_secs(5), Duration::from_millis(100), || {
                delete_soft_count() > 0
            }),
            "待确认窗口内的第二次触发应软删条目"
        );
    }

    // 收尾（终止被测实例、恢复剪贴板、清理临时目录）由 Cleanup 守卫统一执行
}
