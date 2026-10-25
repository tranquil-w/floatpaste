//! 各窗口共享的应用上下文；窗口弱引用仅在事件循环线程升级与访问。
use std::sync::Arc;

use floatpaste_core::state::CoreState;

use crate::app_state::SharedState;
use crate::{QuickPasteWindow, SearchWindow, TooltipWindow};

#[derive(Clone)]
pub struct App {
    pub state: Arc<SharedState>,
    pub picker: slint::Weak<QuickPasteWindow>,
    pub tooltip: slint::Weak<TooltipWindow>,
    pub tray_menu: slint::Weak<crate::TrayMenuWindow>,
    pub search: slint::Weak<SearchWindow>,
    pub editor: slint::Weak<crate::EditorWindow>,
    pub settings: slint::Weak<crate::SettingsWindow>,
}

impl App {
    pub fn with_picker<R>(&self, f: impl FnOnce(&QuickPasteWindow) -> R) -> Option<R> {
        self.picker.upgrade().map(|win| f(&win))
    }

    pub fn with_search<R>(&self, f: impl FnOnce(&SearchWindow) -> R) -> Option<R> {
        self.search.upgrade().map(|win| f(&win))
    }

    pub fn core(&self) -> &CoreState {
        &self.state.core
    }
}
