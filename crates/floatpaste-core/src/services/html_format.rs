//! CF_HTML 的有界解析。只提取文字，不执行 HTML、不加载外部资源。
use std::cell::RefCell;

use html5ever::{
    tendril::StrTendril,
    tokenizer::{
        states::RawKind, BufferQueue, TagKind, Token, TokenSink, TokenSinkResult, Tokenizer,
    },
};

pub const MAX_HTML_BYTES: usize = 4 * 1024 * 1024;
const MAX_TEXT_BYTES: usize = 1024 * 1024;
const MAX_NODES: usize = 50_000;
const MAX_DEPTH: usize = 128;

/// 偏移以原载荷 UTF-8 字节为单位；无上下文时允许 StartHTML/EndHTML 同为 -1。
pub fn html_fragment(bytes: &[u8]) -> Option<&str> {
    if bytes.len() > MAX_HTML_BYTES {
        return None;
    }
    let source = std::str::from_utf8(bytes.strip_suffix(&[0]).unwrap_or(bytes)).ok()?;
    if !source.starts_with("Version:") {
        return None;
    }
    let mut offsets = [None; 4];
    let mut header_end = 0;
    for line in source.split_inclusive(['\r', '\n']) {
        let line_content = line.trim_end_matches(['\r', '\n']);
        if line_content.is_empty() {
            header_end += line.len();
            continue;
        }
        let Some((key, value)) = line_content.split_once(':') else {
            break;
        };
        if !key.bytes().all(|b| b.is_ascii_alphabetic()) {
            break;
        }
        if let Some(index) = ["StartHTML", "EndHTML", "StartFragment", "EndFragment"]
            .iter()
            .position(|k| *k == key)
        {
            if offsets[index].is_some() {
                return None;
            }
            offsets[index] = Some(value.trim().parse::<i64>().ok()?);
        }
        header_end += line.len();
        if header_end > 16 * 1024 {
            return None;
        }
    }
    let [Some(start_html), Some(end_html), Some(start), Some(end)] = offsets else {
        return None;
    };
    let (start, end) = (usize::try_from(start).ok()?, usize::try_from(end).ok()?);
    if start < header_end || start >= end || end > source.len() {
        return None;
    }
    if (start_html, end_html) != (-1, -1) {
        let (hs, he) = (
            usize::try_from(start_html).ok()?,
            usize::try_from(end_html).ok()?,
        );
        if hs < header_end
            || hs > start
            || he < end
            || he > source.len()
            || source.get(hs..he).is_none()
        {
            return None;
        }
    }
    source.get(start..end)
}

#[derive(Default)]
struct TextSink(RefCell<TextState>);

#[derive(Default)]
struct TextState {
    text: String,
    stack: Vec<String>,
    hidden: usize,
    nodes: usize,
    exceeded: bool,
}

fn hidden_tag(name: &str) -> bool {
    matches!(
        name,
        "script" | "style" | "template" | "head" | "title" | "noscript"
    )
}

impl TokenSink for TextSink {
    type Handle = ();
    fn process_token(&self, token: Token, _: u64) -> TokenSinkResult<()> {
        let mut s = self.0.borrow_mut();
        s.nodes += 1;
        if s.exceeded || s.nodes > MAX_NODES {
            s.exceeded = true;
            return TokenSinkResult::Continue;
        }
        match token {
            Token::CharacterTokens(text) if s.hidden == 0 => s.text.push_str(&text),
            Token::TagToken(tag) => {
                let name = tag.name.as_ref();
                if s.hidden == 0
                    && matches!(
                        name,
                        "br" | "p"
                            | "div"
                            | "li"
                            | "tr"
                            | "pre"
                            | "h1"
                            | "h2"
                            | "h3"
                            | "blockquote"
                    )
                    && !s.text.ends_with('\n')
                {
                    s.text.push('\n');
                }
                if s.hidden == 0 && matches!(name, "td" | "th") && tag.kind == TagKind::EndTag {
                    s.text.push('\t');
                }
                if tag.kind == TagKind::StartTag {
                    if !matches!(
                        name,
                        "area"
                            | "base"
                            | "br"
                            | "col"
                            | "embed"
                            | "hr"
                            | "img"
                            | "input"
                            | "link"
                            | "meta"
                            | "param"
                            | "source"
                            | "track"
                            | "wbr"
                    ) {
                        // HTML 中非 void 元素的自闭合斜杠不改变栈语义。
                        if hidden_tag(name) {
                            s.hidden += 1;
                        }
                        s.stack.push(name.to_string());
                    }
                    let raw = match name {
                        "script" => Some(RawKind::ScriptData),
                        "style" | "noscript" | "xmp" | "iframe" | "noembed" | "noframes" => {
                            Some(RawKind::Rawtext)
                        }
                        "title" | "textarea" => Some(RawKind::Rcdata),
                        _ => None,
                    };
                    if s.stack.len() > MAX_DEPTH {
                        s.exceeded = true;
                    }
                    if !s.exceeded {
                        if let Some(raw) = raw {
                            return TokenSinkResult::RawData(raw);
                        }
                    }
                } else if let Some(index) = s.stack.iter().rposition(|n| n == name) {
                    let removed_hidden = s.stack[index..].iter().filter(|n| hidden_tag(n)).count();
                    s.hidden -= removed_hidden;
                    s.stack.truncate(index);
                }
            }
            _ => {}
        }
        if s.text.len() > MAX_TEXT_BYTES {
            s.exceeded = true;
        }
        TokenSinkResult::Continue
    }
}

pub fn extract_html_text(bytes: &[u8]) -> Option<String> {
    parse_html_text(bytes).ok().flatten()
}

/// Err 表示损坏或超限，Ok(None) 表示有效但无可读文字（例如纯图片包装）。
pub fn parse_html_text(bytes: &[u8]) -> Result<Option<String>, &'static str> {
    let fragment = html_fragment(bytes).ok_or("HTML 头或字节偏移无效")?;
    let tokenizer = Tokenizer::new(TextSink::default(), Default::default());
    let input = BufferQueue::default();
    input.push_back(StrTendril::from(fragment));
    let _ = tokenizer.feed(&input);
    if tokenizer.sink.0.borrow().exceeded {
        return Err("HTML 解析超限");
    }
    tokenizer.end();
    let state = tokenizer.sink.0.into_inner();
    if state.exceeded {
        return Err("HTML 解析超限");
    }
    Ok((!state.text.trim().is_empty()).then(|| state.text.trim().to_string()))
}

/// 样本及平台契约测试使用的标准 CF_HTML 编码；生产捕获保存来源原载荷。
pub fn encode_html_fragment(fragment: &str) -> Vec<u8> {
    let header = |start, end, fs, fe| {
        format!("Version:1.0\r\nStartHTML:{start:010}\r\nEndHTML:{end:010}\r\nStartFragment:{fs:010}\r\nEndFragment:{fe:010}\r\n")
    };
    let prefix = "<html><body><!--StartFragment-->";
    let suffix = "<!--EndFragment--></body></html>";
    let hs = header(0, 0, 0, 0).len();
    let fs = hs + prefix.len();
    let fe = fs + fragment.len();
    format!(
        "{}{prefix}{fragment}{suffix}",
        header(hs, fe + suffix.len(), fs, fe)
    )
    .into_bytes()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn validates_byte_offsets_and_extracts_only_bounded_readable_fragment() {
        let bytes = encode_html_fragment("<div>中文😀 &amp; <b>bold</b><br>下一行<script>secret <p>x</script><style>bad</style></div>");
        assert_eq!(extract_html_text(&bytes).unwrap(), "中文😀 & bold\n下一行");
        let mut broken = bytes.clone();
        let offset = std::str::from_utf8(&broken)
            .unwrap()
            .find("StartFragment:")
            .unwrap()
            + 14;
        broken[offset..offset + 10].copy_from_slice(b"9999999999");
        assert!(html_fragment(&broken).is_none());
        assert!(extract_html_text(&encode_html_fragment(
            "<img src='https://example.com/x' alt='picture'>"
        ))
        .is_none());
        assert!(extract_html_text(&encode_html_fragment(&format!(
            "{}x{}",
            "<div>".repeat(MAX_DEPTH + 1),
            "</div>".repeat(MAX_DEPTH + 1)
        )))
        .is_none());
        assert!(
            extract_html_text(&encode_html_fragment(&"x".repeat(MAX_TEXT_BYTES + 1))).is_none()
        );
        assert!(html_fragment(&vec![b'x'; MAX_HTML_BYTES + 1]).is_none());
        assert!(parse_html_text(&encode_html_fragment(&"<b>x</b>".repeat(MAX_NODES / 2))).is_err());
        let mut split_unicode = bytes.clone();
        let chinese_offset = std::str::from_utf8(&bytes).unwrap().find("中文").unwrap();
        split_unicode[offset..offset + 10]
            .copy_from_slice(format!("{:010}", chinese_offset + 1).as_bytes());
        assert!(
            html_fragment(&split_unicode).is_none(),
            "不得从中文 UTF-8 字节中间截取"
        );
        let no_context_header = "Version:1.0\r\nStartHTML:-1\r\nEndHTML:-1\r\nStartFragment:0000000000\r\nEndFragment:0000000000\r\n";
        let fragment = "<b>中文😀</b>";
        let no_context = no_context_header
            .replacen("0000000000", &format!("{:010}", no_context_header.len()), 1)
            .replacen(
                "0000000000",
                &format!("{:010}", no_context_header.len() + fragment.len()),
                1,
            )
            + fragment;
        assert_eq!(extract_html_text(no_context.as_bytes()).unwrap(), "中文😀");

        assert_eq!(
            extract_html_text(&encode_html_fragment(
                "<table><tr><td>甲</td><td>&#x1f600;</td></tr></table>"
            ))
            .unwrap(),
            "甲\t😀"
        );
    }
}
