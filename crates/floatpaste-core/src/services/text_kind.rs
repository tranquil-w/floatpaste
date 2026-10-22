//! 文本用途识别：判断一段文字更像链接、路径还是结构明确的代码。
//!
//! 保守规则：识别不确定时回到普通文本；规则顺序为整段链接 → 路径 →
//! 结构明确的代码。识别只产出 [`TextKind`]，不改写原文，不访问磁盘与
//! 网络，不参与内容身份（hash）计算。

use crate::domain::clip_item::TextKind;
use std::net::{Ipv4Addr, Ipv6Addr};

/// JSON 结构化尝试的长度上限：超限直接不按 JSON 识别（解析成本与
/// 误判风险都不值得）；围栏与裸代码检查为线性扫描，不设上限。
const JSON_ANALYZE_LIMIT: usize = 256 * 1024;

/// URL 中不应出现的字符：空白与控制符之外，还排除引号、括号类与
/// 中英文句读——它们通常意味着 URL 被夹在正文里或带上了句末标点。
fn is_url_disallowed(c: char) -> bool {
    c.is_whitespace()
        || c.is_control()
        || matches!(
            c,
            '<' | '>'
                | '"'
                | '{'
                | '}'
                | '|'
                | '\\'
                | '^'
                | '`'
                | '（'
                | '）'
                | '【'
                | '】'
                | '《'
                | '》'
                | '「'
                | '」'
                | '『'
                | '』'
                | '、'
                | '。'
                | '，'
                | '；'
                | '：'
                | '！'
                | '？'
                | '…'
                | '—'
                | '“'
                | '”'
                | '’'
        )
}

/// 本地校验整段 HTTP(S) URL，返回主机与路径/查询/片段供摘要复用。
/// 简化: 域名只接受字母、数字、连字符标签；升级：有特殊编码主机需求时采用 URL 解析器。
pub(crate) fn http_url_parts(text: &str) -> Option<(&str, &str)> {
    let (scheme, rest) = text.trim().split_once("://")?;
    if !scheme.eq_ignore_ascii_case("http") && !scheme.eq_ignore_ascii_case("https") {
        return None;
    }
    if rest.is_empty() || rest.chars().any(is_url_disallowed) {
        return None;
    }

    let (authority, suffix) = rest.split_at(rest.find(['/', '?', '#']).unwrap_or(rest.len()));
    let host_port = if let Some((userinfo, host_port)) = authority.split_once('@') {
        if userinfo.is_empty() || userinfo.contains(['[', ']']) || host_port.contains('@') {
            return None;
        }
        host_port
    } else {
        authority
    };
    let (host, port_suffix) = if let Some(ipv6) = host_port.strip_prefix('[') {
        let end = ipv6.find(']')?;
        ipv6[..end].parse::<Ipv6Addr>().ok()?;
        host_port.split_at(end + 2)
    } else {
        let end = host_port.find(':').unwrap_or(host_port.len());
        let (host, port) = host_port.split_at(end);
        let domain = host.strip_suffix('.').unwrap_or(host);
        if domain.is_empty()
            || (!domain.contains('.') && !domain.eq_ignore_ascii_case("localhost"))
            || !domain.split('.').all(|label| {
                !label.is_empty()
                    && !label.starts_with('-')
                    && !label.ends_with('-')
                    && label.chars().all(|c| c.is_alphanumeric() || c == '-')
            })
        {
            return None;
        }
        if domain.bytes().all(|b| b.is_ascii_digit() || b == b'.') {
            domain.parse::<Ipv4Addr>().ok()?;
        }
        (host, port)
    };
    if !port_suffix.is_empty() {
        let port = port_suffix.strip_prefix(':')?;
        if port.is_empty() || !port.bytes().all(|b| b.is_ascii_digit()) {
            return None;
        }
        port.parse::<u16>().ok()?;
    }
    Some((host, suffix))
}

/// 整段文本（已去除首尾空白）是否为明确的 Windows 绝对路径、
/// UNC 路径或逐行路径列表。不扫描磁盘、不访问网络共享。
fn is_path_list(text: &str) -> bool {
    let lines = text.lines().map(str::trim).collect::<Vec<_>>();
    !lines.is_empty() && lines.iter().all(|line| is_windows_path(line))
}

/// 单行是否为明确的 Windows 路径：盘符根（`C:\`、`C:/`）或
/// UNC（`\\server\share`；排除 `\\?\`、`\\.\` 设备命名空间）。
/// 相对路径不识别；含 Windows 路径非法字符（`<>"|?*` 与控制符）不识别。
fn is_windows_path(line: &str) -> bool {
    if line.is_empty() {
        return false;
    }
    if line
        .chars()
        .any(|c| c.is_control() || matches!(c, '<' | '>' | '"' | '|' | '?' | '*'))
    {
        return false;
    }

    let bytes = line.as_bytes();
    if bytes.len() >= 3
        && bytes[0].is_ascii_alphabetic()
        && bytes[1] == b':'
        && (bytes[2] == b'\\' || bytes[2] == b'/')
    {
        return true;
    }

    if let Some(rest) = line.strip_prefix(r"\\") {
        let mut parts = rest.splitn(3, ['\\', '/']);
        let server = parts.next().unwrap_or_default();
        let share = parts.next().unwrap_or_default();
        return !server.is_empty() && server != "?" && server != "." && !share.is_empty();
    }

    false
}

/// 整段文本（已去除首尾空白）是否为结构明确的代码：
/// 有效 JSON 对象/数组、完整代码围栏，或同时满足多种强特征的裸代码。
fn is_structured_code(text: &str) -> bool {
    is_json_object_or_array(text) || is_complete_fence(text) || is_bare_code(text)
}

/// 有效且顶层为对象或数组的 JSON：JSON 字符串、数字等标量不是代码。
fn is_json_object_or_array(text: &str) -> bool {
    if !text.starts_with('{') && !text.starts_with('[') {
        return false;
    }
    if text.len() > JSON_ANALYZE_LIMIT {
        return false;
    }
    matches!(
        serde_json::from_str::<serde_json::Value>(text),
        Ok(serde_json::Value::Object(_) | serde_json::Value::Array(_))
    )
}

/// 完整代码围栏：首行以 ``` 开头（可带语言标注），末行为独立的 ```，
/// 中间至少有一行非空内容。未闭合围栏不识别。
fn is_complete_fence(text: &str) -> bool {
    let lines = text.lines().collect::<Vec<_>>();
    let Some((first, rest)) = lines.split_first() else {
        return false;
    };
    let Some((last, middle)) = rest.split_last() else {
        return false;
    };
    first.trim_start().starts_with("```")
        && last.trim() == "```"
        && middle.iter().any(|line| !line.trim().is_empty())
}

/// 列表符号行（`- item`、`* item`、`1. item` 等）：列表语境的关键字
/// （如 "return the item"）不算代码形。
fn is_bullet_line(line: &str) -> bool {
    let trimmed = line.trim_start();
    if trimmed.starts_with(['-', '*', '+', '•']) {
        return true;
    }
    let digits = trimmed.bytes().take_while(|b| b.is_ascii_digit()).count();
    digits > 0 && matches!(trimmed.as_bytes().get(digits), Some(b'.' | b')'))
}

/// 代码关键字行首集合：命中一个即可视为"代码形"的强特征之一。
/// 只匹配行首（允许先有缩进），不剥标点。
fn starts_with_code_keyword(line: &str) -> bool {
    let identifier = line.trim_start();
    [
        "fn",
        "function",
        "def",
        "class",
        "import",
        "export",
        "from",
        "const",
        "let",
        "var",
        "pub",
        "use",
        "package",
        "using",
        "namespace",
        "#include",
        "require",
        "return",
        "public",
        "private",
        "static",
        "void",
        "struct",
        "impl",
        "enum",
    ]
    .iter()
    .any(|keyword| {
        identifier
            .get(..keyword.len())
            .is_some_and(|prefix| prefix.eq_ignore_ascii_case(keyword))
            && identifier
                .get(keyword.len()..)
                .is_some_and(|rest| rest.starts_with(|c: char| !c.is_alphanumeric() && c != '_'))
    })
}

/// 裸代码（无围栏）识别门槛：至少两行非空内容，且"至少三分之一行有
/// 缩进"与"至少两行具备代码形"两类强特征同时出现；列表符号行不参与
/// 代码形判定。仅凭标点或缩进不足以判定。
fn is_bare_code(text: &str) -> bool {
    // 保留行首空白供缩进判断，仅去掉行尾
    let lines = text
        .lines()
        .map(str::trim_end)
        .filter(|line| !line.trim().is_empty())
        .collect::<Vec<_>>();
    if lines.len() < 2 {
        return false;
    }

    let indented = lines
        .iter()
        .filter(|line| line.starts_with(' ') || line.starts_with('\t'))
        .count();
    let shaped = lines
        .iter()
        .filter(|line| {
            let trimmed = line.trim_start();
            if is_bullet_line(trimmed) {
                return false;
            }
            trimmed.ends_with(';')
                || trimmed.ends_with('{')
                || trimmed.ends_with('}')
                || starts_with_code_keyword(trimmed)
        })
        .count();

    indented * 3 >= lines.len() && shaped >= 2
}

/// 识别文本用途。输入为完整原文（函数内部自行去除首尾空白）。
pub fn detect_text_kind(full_text: &str) -> TextKind {
    let trimmed = full_text.trim();
    if trimmed.is_empty() {
        return TextKind::Plain;
    }
    if http_url_parts(trimmed).is_some() {
        return TextKind::Link;
    }
    if is_path_list(trimmed) {
        return TextKind::Path;
    }
    if is_structured_code(trimmed) {
        return TextKind::Code;
    }
    TextKind::Plain
}

#[cfg(test)]
mod tests {
    use super::detect_text_kind;
    use crate::domain::clip_item::TextKind;

    fn kind_of(text: &str) -> TextKind {
        detect_text_kind(text)
    }

    #[test]
    fn recognizes_single_http_and_https_urls() {
        assert_eq!(kind_of("https://example.com"), TextKind::Link);
        assert_eq!(kind_of("http://example.com/a/b?x=1#frag"), TextKind::Link);
        assert_eq!(kind_of("  https://example.com/path \n"), TextKind::Link);
        assert_eq!(kind_of("https://localhost:8080/api"), TextKind::Link);
        assert_eq!(kind_of("HTTPS://EXAMPLE.COM/PATH"), TextKind::Link);
        assert_eq!(
            kind_of("https://zh.wikipedia.org/wiki/剪贴板"),
            TextKind::Link
        );
        assert_eq!(
            kind_of("http://user:pass@example.com/resource"),
            TextKind::Link
        );
        assert_eq!(kind_of("https://192.168.0.1/admin"), TextKind::Link);
        assert_eq!(kind_of("https://[::1]:8080/api"), TextKind::Link);
        assert_eq!(kind_of("https://[2001:db8::1]/"), TextKind::Link);
        assert_eq!(kind_of("https://example.com:65535/"), TextKind::Link);
    }

    #[test]
    fn invalid_authorities_and_ports_stay_plain() {
        for text in [
            "https://[not-an-ip]/path",
            "https://[::1]garbage/",
            "https://[::1]:65536/",
            "https://example.com:999999/path",
            "https://example.com:/",
            "https://example.com:+80/",
            "https://a@@example.com/",
            "https://bad..example.com/",
            "https://-bad.example.com/",
            "https://256.1.2.3/",
        ] {
            assert_eq!(kind_of(text), TextKind::Plain, "{text}");
        }
    }

    #[test]
    fn multiline_unicode_prose_does_not_panic_or_become_code() {
        for text in [
            "第一行\n第二行",
            "a中文\n  return x",
            "🙂第一行\n第二行",
            "éclair\n  обычный текст",
        ] {
            assert_eq!(kind_of(text), TextKind::Plain, "{text}");
        }
    }

    #[test]
    fn urls_in_prose_or_with_sentence_punctuation_stay_plain() {
        // 带句末标点、非 http(s) scheme、裸域名都不是链接
        assert_eq!(kind_of("https://example.com。"), TextKind::Plain);
        assert_eq!(kind_of("ftp://example.com/file"), TextKind::Plain);
        assert_eq!(kind_of("example.com"), TextKind::Plain);
        assert_eq!(kind_of("https://"), TextKind::Plain);
        assert_eq!(kind_of("https://host with space"), TextKind::Plain);
        assert_eq!(kind_of("file:///C:/Temp/a.txt"), TextKind::Plain);
    }

    #[test]
    fn recognizes_windows_absolute_unc_and_path_lists() {
        assert_eq!(kind_of(r"C:\Temp\a.txt"), TextKind::Path);
        assert_eq!(kind_of("C:/Users/wumen/Documents"), TextKind::Path);
        assert_eq!(kind_of(r"D:\"), TextKind::Path);
        assert_eq!(kind_of(r"\\server\share\file.txt"), TextKind::Path);
        assert_eq!(kind_of(r"\\NAS-01\public"), TextKind::Path);
        assert_eq!(kind_of("C:\\a.txt\r\nD:\\dir\\b.txt"), TextKind::Path);
        // 带空格的合法路径
        assert_eq!(kind_of(r"C:\Program Files\App\app.exe"), TextKind::Path);
    }

    #[test]
    fn relative_paths_mixed_lists_and_wildcards_stay_plain() {
        assert_eq!(kind_of(r"src\main.rs"), TextKind::Plain);
        assert_eq!(kind_of(r"\\?\C:\Temp\a.txt"), TextKind::Plain);
        assert_eq!(kind_of(r"\\.\pipe\name"), TextKind::Plain);
        assert_eq!(kind_of("C:\\a.txt\nsome notes"), TextKind::Plain);
        assert_eq!(kind_of(r"C:\Temp\*.txt"), TextKind::Plain);
        assert_eq!(kind_of(r"C:\Temp<a>.txt"), TextKind::Plain);
    }

    #[test]
    fn recognizes_json_objects_and_arrays_but_not_scalars() {
        assert_eq!(kind_of(r#"{"a": 1, "b": [2, 3]}"#), TextKind::Code);
        assert_eq!(kind_of("[1, 2, 3]"), TextKind::Code);
        assert_eq!(
            kind_of("{\n  \"name\": \"值\",\n  \"ok\": true\n}"),
            TextKind::Code
        );
        // JSON 字符串与普通数字不是结构明确的代码
        assert_eq!(kind_of(r#""just a string""#), TextKind::Plain);
        assert_eq!(kind_of("12345"), TextKind::Plain);
        // 括号形态但非法 JSON
        assert_eq!(kind_of("{not json}"), TextKind::Plain);
    }

    #[test]
    fn recognizes_complete_fences_only() {
        assert_eq!(
            kind_of("```rust\nfn main() {\n    println!(\"hi\");\n}\n```"),
            TextKind::Code
        );
        assert_eq!(kind_of("```\nplain fenced content\n```"), TextKind::Code);
        // 未闭合围栏与空围栏不是完整围栏
        assert_eq!(kind_of("```rust\nfn main() {}"), TextKind::Plain);
        assert_eq!(kind_of("``````"), TextKind::Plain);
    }

    #[test]
    fn bare_code_requires_indentation_and_code_shape_together() {
        assert_eq!(
            kind_of("function add(a, b) {\n  return a + b;\n}\n"),
            TextKind::Code
        );
        assert_eq!(
            kind_of("def greet(name):\n    print(name)\n    return 0"),
            TextKind::Code
        );
        // 全部顶格：缺缩进特征
        assert_eq!(kind_of("let x = 1;\nlet y = 2;"), TextKind::Plain);
        // 有缩进但没有代码形：嵌套列表式的散文
        assert_eq!(
            kind_of("- 第一项\n  - 子项 A\n  - 子项 B\n- 第二项"),
            TextKind::Plain
        );
        // 单行伪代码：行数不足
        assert_eq!(kind_of("    return 0;"), TextKind::Plain);
    }

    #[test]
    fn prose_and_empty_text_stay_plain() {
        assert_eq!(kind_of("今天天气不错，适合写代码。"), TextKind::Plain);
        assert_eq!(
            kind_of("first line\nsecond line\nthird line"),
            TextKind::Plain
        );
        assert_eq!(kind_of(""), TextKind::Plain);
        assert_eq!(kind_of("   \n  "), TextKind::Plain);
    }

    #[test]
    fn urls_with_backslash_or_prose_fragments_stay_plain() {
        // 反斜杠不是合法 URL 字符；正文片段含 URL 也不是链接
        assert_eq!(kind_of("https://example.com/a\\b"), TextKind::Plain);
        assert_eq!(
            kind_of("详见 https://example.com 中的说明"),
            TextKind::Plain
        );
    }
}
