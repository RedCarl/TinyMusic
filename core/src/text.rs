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

/// 解析 LRC 文本 → 歌词行。
///
/// 同一句可以挂多个时间戳（`[00:12.00][01:20.00]副歌`），
/// 网易云返回的歌词就是这样；`[ti:]`/`[ar:]` 这类元信息标签会被跳过。
pub fn parse_lrc(text: &str) -> Vec<crate::model::LyricLine> {
    let mut out = Vec::new();
    for raw in text.lines() {
        let mut rest = raw.trim();
        let mut stamps: Vec<u32> = Vec::new();
        while let Some(tail) = rest.strip_prefix('[') {
            let Some(end) = tail.find(']') else { break };
            let tag = &tail[..end];
            match tag.split_once(':').and_then(|(m, s)| {
                let m: f64 = m.trim().parse().ok()?;
                let s: f64 = s.trim().parse().ok()?;
                Some(m * 60_000.0 + s * 1000.0)
            }) {
                Some(ms) => {
                    stamps.push(ms.max(0.0) as u32);
                    rest = &tail[end + 1..];
                }
                // `[ti:晴天]` 之类：不是时间戳，整行忽略
                None => break,
            }
        }
        if stamps.is_empty() {
            continue;
        }
        let line = rest.trim();
        if line.is_empty() {
            continue;
        }
        for ms in stamps {
            out.push(crate::model::LyricLine {
                time_ms: ms,
                text: line.to_string(),
            });
        }
    }
    out.sort_by_key(|l| l.time_ms);
    out
}

#[cfg(test)]
mod lyric_tests {
    use super::parse_lrc;

    #[test]
    fn parses_multi_stamp_and_skips_meta() {
        let lrc = "[ti:Test]\n[ar:Someone]\n[00:01.50]first\n[00:03.00][00:09.25]chorus\n";
        let lines = parse_lrc(lrc);
        assert_eq!(lines.len(), 3);
        assert_eq!(lines[0].time_ms, 1500);
        assert_eq!(lines[0].text, "first");
        assert_eq!(lines[1].time_ms, 3000);
        assert_eq!(lines[2].time_ms, 9250);
        assert_eq!(lines[2].text, "chorus");
    }

    #[test]
    fn empty_and_garbage_are_safe() {
        assert!(parse_lrc("").is_empty());
        assert!(parse_lrc("纯文本没有时间戳").is_empty());
        assert!(parse_lrc("[99:99.99]").is_empty()); // 空歌词行不计入
    }
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
