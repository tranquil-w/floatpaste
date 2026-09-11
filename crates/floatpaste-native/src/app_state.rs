//! 原生壳共享状态：核心状态 + 速贴会话 + 列表缓存。
//!
//! 所有窗口操作都经由 `slint::invoke_from_event_loop` 回到事件循环线程，
//! 这里只承载跨线程可安全访问的会话/缓存数据（Send + Sync）。

use std::sync::atomic::{AtomicBool, AtomicIsize, Ordering};
use std::sync::Mutex;

use floatpaste_core::domain::clip_item::ClipItemSummary;
use floatpaste_core::domain::settings::UserSetting;
use floatpaste_core::state::CoreState;

/// 速贴会话：显示时捕获的目标窗口（粘贴/关闭时恢复其前台与焦点）
#[derive(Debug, Clone, Copy, Default)]
pub struct TargetSession {
    pub target_window_hwnd: Option<isize>,
    pub target_focus_hwnd: Option<isize>,
}

/// 搜索会话：打开时捕获的回贴目标窗口
#[derive(Debug, Clone, Copy, Default)]
pub struct SearchSession {
    pub target_window_hwnd: Option<isize>,
}

pub struct SharedState {
    pub core: CoreState,
    session: Mutex<TargetSession>,
    picker_active: AtomicBool,
    pub picker_hwnd: AtomicIsize,
    pub tooltip_hwnd: AtomicIsize,
    search_active: AtomicBool,
    pub search_hwnd: AtomicIsize,
    search_session: Mutex<SearchSession>,
    /// 列表缓存：会话键（Enter/Esc 路径按 id 取条目，避免与 UI 行模型竞态）
    items: Mutex<Vec<ClipItemSummary>>,
    /// 搜索窗口结果列表缓存（分页累积，与速贴列表相互独立）
    search_items: Mutex<Vec<ClipItemSummary>>,
    /// 选中项 id 锚点：新剪贴插入列表头部时按 id 恢复，避免选区漂移
    selected_id: Mutex<Option<String>>,
    pub favorite_pending: AtomicBool,
    pub settings: Mutex<UserSetting>,
}

impl SharedState {
    pub fn new(core: CoreState) -> Self {
        let settings = core.current_settings().unwrap_or_default();
        Self {
            core,
            session: Mutex::new(TargetSession::default()),
            picker_active: AtomicBool::new(false),
            picker_hwnd: AtomicIsize::new(0),
            tooltip_hwnd: AtomicIsize::new(0),
            search_active: AtomicBool::new(false),
            search_hwnd: AtomicIsize::new(0),
            search_session: Mutex::new(SearchSession::default()),
            items: Mutex::new(Vec::new()),
            search_items: Mutex::new(Vec::new()),
            selected_id: Mutex::new(None),
            favorite_pending: AtomicBool::new(false),
            settings: Mutex::new(settings),
        }
    }

    pub fn refresh_settings(&self) -> UserSetting {
        let settings = self.core.current_settings().unwrap_or_default();
        *self
            .settings
            .lock()
            .unwrap_or_else(|error| error.into_inner()) = settings.clone();
        settings
    }

    pub fn current_settings(&self) -> UserSetting {
        self.settings
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .clone()
    }

    pub fn is_picker_active(&self) -> bool {
        self.picker_active.load(Ordering::SeqCst)
    }

    pub fn begin_picker_activation(&self) {
        self.picker_active.store(true, Ordering::SeqCst);
    }

    pub fn end_picker_activation(&self) {
        self.picker_active.store(false, Ordering::SeqCst);
    }

    pub fn set_picker_session(&self, target: TargetSession) {
        *self
            .session
            .lock()
            .unwrap_or_else(|error| error.into_inner()) = target;
    }

    pub fn picker_session(&self) -> TargetSession {
        self.session
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .clone()
    }

    pub fn is_search_active(&self) -> bool {
        self.search_active.load(Ordering::SeqCst)
    }

    pub fn begin_search_activation(&self) {
        self.search_active.store(true, Ordering::SeqCst);
    }

    pub fn end_search_activation(&self) {
        self.search_active.store(false, Ordering::SeqCst);
    }

    pub fn set_search_session(&self, session: SearchSession) {
        *self
            .search_session
            .lock()
            .unwrap_or_else(|error| error.into_inner()) = session;
    }

    pub fn search_session(&self) -> SearchSession {
        *self
            .search_session
            .lock()
            .unwrap_or_else(|error| error.into_inner())
    }

    pub fn set_search_items(&self, items: Vec<ClipItemSummary>) {
        self.search_items
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .clone_from(&items);
    }

    pub fn search_items(&self) -> Vec<ClipItemSummary> {
        self.search_items
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .clone()
    }

    pub fn search_item_at(&self, index: usize) -> Option<ClipItemSummary> {
        self.search_items
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .get(index)
            .cloned()
    }

    pub fn set_items(&self, items: Vec<ClipItemSummary>) {
        self.items
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .clone_from(&items);
    }

    pub fn items(&self) -> Vec<ClipItemSummary> {
        self.items
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .clone()
    }

    pub fn item_at(&self, index: usize) -> Option<ClipItemSummary> {
        self.items
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .get(index)
            .cloned()
    }

    pub fn selected_id(&self) -> Option<String> {
        self.selected_id
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .clone()
    }

    pub fn set_selected_id(&self, id: Option<String>) {
        *self
            .selected_id
            .lock()
            .unwrap_or_else(|error| error.into_inner()) = id;
    }
}
