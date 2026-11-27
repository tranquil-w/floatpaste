use std::collections::VecDeque;

use slint::SharedString;

const MAX_HISTORY_BYTES: usize = 16 * 1024 * 1024;
const MAX_HISTORY_EDITS: usize = 1024;

#[derive(Clone, Copy, Default)]
pub(super) struct Selection {
    pub anchor: i32,
    pub cursor: i32,
}

impl Selection {
    pub fn caret(offset: usize) -> Self {
        Self {
            anchor: offset as i32,
            cursor: offset as i32,
        }
    }
}

struct Edit {
    start: usize,
    removed: String,
    inserted: String,
    before: Selection,
    after: Selection,
    cleanup: bool,
}

impl Edit {
    fn bytes(&self) -> usize {
        self.removed.len() + self.inserted.len()
    }
}

#[derive(Default)]
pub(super) struct DraftHistory {
    text: SharedString,
    selection: Selection,
    undo: VecDeque<Edit>,
    redo: Vec<Edit>,
    bytes: usize,
}

impl DraftHistory {
    pub fn reset(&mut self, text: SharedString) {
        *self = Self {
            text,
            ..Self::default()
        };
    }

    pub fn sync(&mut self, text: SharedString) {
        if self.text != text {
            self.reset(text);
        }
    }

    pub fn observe_selection(&mut self, text: &str, selection: Selection) {
        if self.text.as_str() == text {
            self.selection = selection;
        }
    }

    pub fn record_input(&mut self, text: SharedString, after: Selection) {
        let before = std::mem::replace(&mut self.selection, after);
        if self.text == text {
            return;
        }
        // 先按编辑后的光标定位公共后缀，以免重复字符把编辑位置推到行尾。
        let after_cursor = text.floor_char_boundary(after.cursor.max(0) as usize);
        let suffix = self
            .text
            .chars()
            .rev()
            .zip(text[after_cursor..].chars().rev())
            .take_while(|(old, new)| old == new)
            .map(|(ch, _)| ch.len_utf8())
            .sum::<usize>();
        let old = &self.text[..self.text.len() - suffix];
        let new = &text[..text.len() - suffix];
        let start = old
            .chars()
            .zip(new.chars())
            .take_while(|(old, new)| old == new)
            .map(|(ch, _)| ch.len_utf8())
            .sum::<usize>();
        let edit = Edit {
            start,
            removed: old[start..].into(),
            inserted: new[start..].into(),
            before,
            after,
            cleanup: false,
        };
        self.text = text;
        self.push(edit);
    }

    pub fn replace(
        &mut self,
        range: std::ops::Range<usize>,
        replacement: &str,
        before: Selection,
        keep_selected: bool,
    ) -> (SharedString, Selection) {
        let end = range.start + replacement.len();
        let after = if keep_selected {
            Selection {
                anchor: range.start as i32,
                cursor: end as i32,
            }
        } else {
            Selection::caret(end)
        };
        let edit = Edit {
            start: range.start,
            removed: self.text[range.clone()].into(),
            inserted: replacement.into(),
            before,
            after,
            cleanup: true,
        };
        let mut updated = self.text.to_string();
        updated.replace_range(range, replacement);
        self.text = updated.into();
        self.selection = after;
        self.push(edit);
        (self.text.clone(), after)
    }

    fn push(&mut self, mut edit: Edit) {
        self.bytes -= self
            .redo
            .drain(..)
            .map(|entry| entry.bytes())
            .sum::<usize>();
        self.bytes += edit.bytes();
        if let Some(previous) = self
            .undo
            .back_mut()
            .filter(|previous| !previous.cleanup && !edit.cleanup)
        {
            if previous.removed.is_empty()
                && edit.removed.is_empty()
                && previous.start + previous.inserted.len() == edit.start
                && previous.inserted != "\n"
                && edit.inserted != "\n"
            {
                previous.inserted.push_str(&edit.inserted);
                previous.after = edit.after;
            } else if previous.inserted.is_empty()
                && edit.inserted.is_empty()
                && edit.start + edit.removed.len() == previous.start
            {
                edit.removed.push_str(&previous.removed);
                edit.before = previous.before;
                *previous = edit;
            } else {
                self.undo.push_back(edit);
            }
        } else {
            self.undo.push_back(edit);
        }
        while self.undo.len() > MAX_HISTORY_EDITS
            || (self.bytes > MAX_HISTORY_BYTES && self.undo.len() > 1)
        {
            self.bytes -= self.undo.pop_front().expect("历史至少有两步").bytes();
        }
    }

    pub fn step(&mut self, redo: bool) -> Option<(SharedString, Selection)> {
        let edit = if redo {
            self.redo.pop()?
        } else {
            self.undo.pop_back()?
        };
        let (from, to, selection) = if redo {
            (&edit.removed, &edit.inserted, edit.after)
        } else {
            (&edit.inserted, &edit.removed, edit.before)
        };
        let mut text = self.text.to_string();
        text.replace_range(edit.start..edit.start + from.len(), to);
        self.text = text.into();
        self.selection = selection;
        if redo {
            self.undo.push_back(edit);
        } else {
            self.redo.push(edit);
        }
        Some((self.text.clone(), selection))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cleanup_keeps_its_own_step_between_normal_edits() {
        let mut history = DraftHistory::default();
        history.reset("甲  ".into());
        history.record_input("甲  乙".into(), Selection::caret(8));
        history.record_input("甲  乙!".into(), Selection::caret(9));
        let reverse = Selection {
            anchor: 5,
            cursor: 3,
        };
        let (trimmed, caret) = history.replace(3..5, "", reverse, true);
        assert_eq!(trimmed, "甲乙!");
        assert_eq!(caret.cursor, 3);
        history.record_input("甲😀乙!".into(), Selection::caret(7));
        assert_eq!(history.step(false).unwrap().0, "甲乙!");
        let (original, restored) = history.step(false).unwrap();
        assert_eq!(original, "甲  乙!");
        assert_eq!((restored.anchor, restored.cursor), (5, 3));
        assert_eq!(history.step(false).unwrap().0, "甲  ");
        assert_eq!(history.step(true).unwrap().0, "甲  乙!");
        history.record_input("甲  乙!?".into(), Selection::caret(10));
        assert!(history.step(true).is_none());
    }
}
