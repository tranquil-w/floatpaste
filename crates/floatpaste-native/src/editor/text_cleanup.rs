use std::borrow::Cow;

use crate::EditorCleanup;

pub(super) fn clean(action: EditorCleanup, text: &str) -> Option<Cow<'_, str>> {
    Some(match action {
        EditorCleanup::JoinLines if text.contains(['\r', '\n']) => {
            let mut joined = String::new();
            for line in text
                .split(['\r', '\n'])
                .map(str::trim)
                .filter(|line| !line.is_empty())
            {
                if !joined.is_empty() {
                    joined.push(' ');
                }
                joined.push_str(line);
            }
            joined.into()
        }
        EditorCleanup::JoinLines => text.into(),
        EditorCleanup::RemoveLineBreaks => text.replace(['\r', '\n'], "").into(),
        EditorCleanup::Trim => text.trim().into(),
        EditorCleanup::PowerShell => powershell_line(text)?.into(),
    })
}

/// 只删除 PowerShell 在参数之间识别为 LineContinuation 的反引号换行。
/// 引号内或紧贴词尾的反引号换行可能是参数内容，遇到时保留原文。
fn powershell_line(text: &str) -> Option<String> {
    let text = text
        .strip_suffix("\r\n")
        .or_else(|| text.strip_suffix('\n'))
        .unwrap_or(text);
    if text.contains("--%") {
        return None;
    }
    let mut chars = text.chars().peekable();
    let mut output = String::with_capacity(text.len());
    let mut quote = None;
    let mut token_start = true;
    while let Some(ch) = chars.next() {
        match ch {
            '\r' | '\n' | '‘' | '’' | '‚' | '‛' | '“' | '”' | '„' | '‟' => {
                return None
            }
            '`' if quote != Some('\'') => {
                let escaped = chars.next()?;
                if matches!(escaped, '\r' | '\n') {
                    if !token_start || quote.is_some() {
                        return None;
                    }
                    if escaped == '\r' && chars.next() != Some('\n') {
                        return None;
                    }
                    continue;
                }
                output.push(ch);
                output.push(escaped);
                token_start = false;
                continue;
            }
            '\'' | '"' if quote == Some(ch) => {
                if chars.next_if_eq(&ch).is_some() {
                    output.push(ch);
                } else {
                    quote = None;
                }
            }
            '\'' | '"' if quote.is_none() => quote = Some(ch),
            '#' | '@' | '(' | ')' | '{' | '}' | '[' | ']' | '|' | ';' | '&' | '<' | '>'
                if quote.is_none() =>
            {
                return None
            }
            '$' if quote == Some('"') && chars.peek() == Some(&'(') => return None,
            _ => {}
        }
        output.push(ch);
        token_start = quote.is_none() && matches!(ch, ' ' | '\t');
    }
    quote.is_none().then_some(output)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plain_text_actions_have_distinct_whitespace_rules() {
        let sample = " \t甲  乙\r\n\r\n  丙\n\t丁 \t";
        for (action, expected) in [
            (EditorCleanup::JoinLines, "甲  乙 丙 丁"),
            (EditorCleanup::RemoveLineBreaks, " \t甲  乙  丙\t丁 \t"),
            (EditorCleanup::Trim, "甲  乙\r\n\r\n  丙\n\t丁"),
        ] {
            assert_eq!(clean(action, sample).as_deref(), Some(expected));
        }
        assert_eq!(
            clean(EditorCleanup::JoinLines, "  单行  ").as_deref(),
            Some("  单行  ")
        );
        assert_eq!(
            clean(EditorCleanup::JoinLines, "\r\n\t\n").as_deref(),
            Some("")
        );
    }

    #[test]
    fn powershell_preserves_arguments_and_declines_ambiguous_newlines() {
        let command = "Write-Output `\r\n  '中文  😀' `\n  \"a`\"b\" 'don''t'\n";
        assert_eq!(
            powershell_line(command).as_deref(),
            Some("Write-Output   '中文  😀'   \"a`\"b\" 'don''t'")
        );
        for unsupported in [
            "Write-Output foo`\nbar",
            "Write-Output \"foo`\nbar\"",
            "Write-Output 'foo`\nbar'",
            "Write-Output foo ``\n bar",
            "Write-Output ` \n bar",
            "Write-Output foo\nWrite-Output bar",
            "Write-Output @'\nfoo\n'@",
            "Write-Output `\n # comment",
            "Write-Output --% foo `\n bar",
            "Write-Output \"$(Get-Item `\n foo)\"",
        ] {
            assert_eq!(powershell_line(unsupported), None, "{unsupported:?}");
        }
    }
}
