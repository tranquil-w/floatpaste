pub const CLIPS_CHANGED_EVENT: &str = "clips://changed";
pub const SETTINGS_CHANGED_EVENT: &str = "settings://changed";
pub const MANAGER_OPEN_SETTINGS_EVENT: &str = "manager://open-settings";
pub const PICKER_SESSION_START_EVENT: &str = "picker://session-start";
pub const PICKER_SESSION_END_EVENT: &str = "picker://session-end";
pub const PICKER_NAVIGATE_EVENT: &str = "picker://navigate";
pub const PICKER_CONFIRM_EVENT: &str = "picker://confirm";
pub const PICKER_SELECT_INDEX_EVENT: &str = "picker://select-index";

// Workbench 相关事件
pub const WORKBENCH_SESSION_START_EVENT: &str = "workbench://session-start";
pub const WORKBENCH_SESSION_END_EVENT: &str = "workbench://session-end";
#[allow(dead_code)] // 预留给未来 Workbench 编辑操作使用
pub const WORKBENCH_EDIT_ITEM_EVENT: &str = "workbench://edit-item";
#[allow(dead_code)] // 预留给未来 Workbench 搜索操作使用
pub const WORKBENCH_SEARCH_EVENT: &str = "workbench://search";
#[allow(dead_code)] // 预留给未来 Workbench 回贴操作使用
pub const WORKBENCH_PASTE_EVENT: &str = "workbench://paste";
pub const WORKBENCH_NAVIGATE_EVENT: &str = "workbench://navigate";

// Picker 跳转事件（将在 Task 6 中删除，待 Editor 完全接管后移除）
pub const PICKER_OPEN_WORKBENCH_EDIT_EVENT: &str = "picker://open-workbench-edit";
pub const PICKER_OPEN_WORKBENCH_SEARCH_EVENT: &str = "picker://open-workbench-search";

// Editor 相关事件
pub const EDITOR_SESSION_START_EVENT: &str = "editor://session-start";
pub const EDITOR_SESSION_END_EVENT: &str = "editor://session-end";
