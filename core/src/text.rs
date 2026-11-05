//! 文本清洗：酷我返回的字段里混了 HTML 实体与多层 `\uXXXX` 转义。
//!
//! 实测原始响应中艺人字段是 `王源\\\\u0026周传雄`（四个反斜杠），
//! 需要「折叠反斜杠 → 解 unicode → 解 HTML 实体」多轮循环才能还原成 `王源&周传雄`。

/// 完整清洗。幂等：对已清洗文本再调用不会继续变化。
///
/// 分两阶段：先对反斜杠/unicode 反复折叠（多层转义需要多轮），
/// 再对 HTML 实体只解一次——否则 `&amp;lt;` 会被解开两次变成 `<`。
pub fn clean(raw: &str) -> String {
    let mut s = raw.to_string();
    for _ in 0..4 {
        let before = s.clone();
        s = collapse_backslashes(&s);
        s = unescape_unicode_literal(&s);
        if s == before {
            break;
        }
    }
    s = unescape_html(&s);
    collapse_ws(&s)
}

/// `\\` → `\`（一轮）。用于剥离多层转义。
fn collapse_backslashes(s: &str) -> String {
    if !s.contains('\\') {
        return s.to_string();
    }
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\\' && chars.peek() == Some(&'\\') {
            chars.next();
            out.push('\\');
        } else {
            out.push(c);
        }
    }
    out
}

/// 把字面量 `\uXXXX`（反斜杠 + u + 4 位十六进制）替换为字符。
fn unescape_unicode_literal(s: &str) -> String {
    if !s.contains("\\u") {
        return s.to_string();
    }
    let chars: Vec<char> = s.chars().collect();
    let mut out = String::with_capacity(s.len());
    let mut i = 0;
    while i < chars.len() {
        if chars[i] == '\\'
            && i + 5 < chars.len()
            && (chars[i + 1] == 'u' || chars[i + 1] == 'U')
        {
            let hex: String = chars[i + 2..i + 6].iter().collect();
            if let Ok(cp) = u32::from_str_radix(&hex, 16) {
                if let Some(ch) = char::from_u32(cp) {
                    out.push(ch);
                    i += 6;
                    continue;
                }
            }
        }
        out.push(chars[i]);
        i += 1;
    }
    out
}

/// HTML 实体还原。`&amp;` 放最后，避免 `&amp;lt;` 被二次解析。
fn unescape_html(s: &str) -> String {
    if !s.contains('&') {
        return s.to_string();
    }
    let mut out = s
        .replace("&nbsp;", " ")
        .replace("&apos;", "'")
        .replace("&quot;", "\"")
        .replace("&lt;", "<")
        .replace("&gt;", ">");

    // 数字实体 &#123; / &#x1F600;
    out = replace_numeric_entities(&out);

    out.replace("&amp;", "&")
}

fn replace_numeric_entities(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let chars: Vec<char> = s.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        if chars[i] == '&' && i + 1 < chars.len() && chars[i + 1] == '#' {
            let mut j = i + 2;
            let hex = chars.get(j) == Some(&'x') || chars.get(j) == Some(&'X');
            if hex {
                j += 1;
            }
            let start = j;
            while j < chars.len() && chars[j].is_ascii_hexdigit() {
                j += 1;
            }
            let terminated = chars.get(j) == Some(&';');
            if j > start && terminated {
                let num: String = chars[start..j].iter().collect();
                let radix = if hex { 16 } else { 10 };
                if let Ok(cp) = u32::from_str_radix(&num, radix) {
                    if let Some(ch) = char::from_u32(cp) {
                        out.push(ch);
                        i = j + 1;
                        continue;
                    }
                }
            }
        }
        out.push(chars[i]);
        i += 1;
    }
    out
}

/// 合并连续空白并 trim（换行、全角空格一并视为空白）
fn collapse_ws(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut prev_ws = false;
    for c in s.chars() {
        if c.is_whitespace() || c == '\u{3000}' {
            if !prev_ws && !out.is_empty() {
                out.push(' ');
            }
            prev_ws = true;
        } else {
            out.push(c);
            prev_ws = false;
        }
    }
    out.trim().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn html_entities() {
        assert_eq!(clean("Jay&nbsp;Chou"), "Jay Chou");
        assert_eq!(clean("The State I&apos;m In"), "The State I'm In");
        assert_eq!(clean("a &amp; b"), "a & b");
        assert_eq!(clean("a &amp;lt; b"), "a &lt; b"); // 不二次解析
    }

    #[test]
    fn unicode_literal() {
        assert_eq!(clean(r"王源\u0026周传雄"), "王源&周传雄");
        // 实测最深的形态：解析器留下的两个反斜杠
        assert_eq!(clean(r"王源\\u0026周传雄"), "王源&周传雄");
        assert_eq!(clean(r"a\u4e2db"), "a中b");
    }

    #[test]
    fn numeric_entities() {
        assert_eq!(clean("&#65;&#x42;"), "AB");
    }

    #[test]
    fn whitespace_collapsed() {
        assert_eq!(clean("  a   b  "), "a b");
    }

    #[test]
    fn idempotent() {
        let once = clean("Jay&nbsp;Chou \\u0026 friends");
        let twice = clean(&once);
        assert_eq!(once, twice);
    }
}
