//! 上屏动作目录到 Slint 菜单模型的共同投影。
use std::rc::Rc;

use floatpaste_core::services::clip_display::PasteActionSummary;
use slint::{ModelRc, VecModel};

use crate::PasteActionItem;

pub(crate) fn action_items(summaries: &[PasteActionSummary]) -> ModelRc<PasteActionItem> {
    let items: Vec<PasteActionItem> = summaries
        .iter()
        .map(|summary| PasteActionItem {
            label: summary.label.clone().into(),
            shortcut: summary.shortcut.clone().into(),
        })
        .collect();
    ModelRc::new(Rc::new(VecModel::from(items)))
}
