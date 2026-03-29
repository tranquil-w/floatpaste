use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EditorSession {
    pub item_id: String,
    pub source: EditorSource,
    pub return_to: EditorReturnTarget,
    pub target_window_hwnd: Option<isize>,
    pub target_focus_hwnd: Option<isize>,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum EditorSource {
    Picker,
    Search,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum EditorReturnTarget {
    Picker,
    Search,
}

#[cfg(test)]
mod tests {
    use super::{EditorReturnTarget, EditorSession, EditorSource};

    #[test]
    fn editor_session_source_and_return_target_should_roundtrip() {
        let session = EditorSession {
            item_id: "clip-1".to_string(),
            source: EditorSource::Picker,
            return_to: EditorReturnTarget::Picker,
            target_window_hwnd: None,
            target_focus_hwnd: None,
        };

        let json = serde_json::to_string(&session).unwrap();
        assert!(json.contains("\"source\":\"picker\""));
        assert!(json.contains("\"returnTo\":\"picker\""));
    }
}
