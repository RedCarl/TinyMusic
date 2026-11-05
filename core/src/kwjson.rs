//! 酷我搜索接口返回的不是标准 JSON，而是 Python dict 字面量：
//!
//! ```text
//! {'PN':'0','abslist':[{'SONGNAME':'稻香','payInfo':{'down':'1111','extendAttr':0}}]}
//! ```
//!
//! 特点：单引号字符串与单引号键名、未加引号的整数、`None/True/False`、
//! 残留的 `\\` 转义。不能靠「把 ' 换成 "」的偷懒办法（值里存在 `&apos;` 与
//! 多层反斜杠），所以这里实现一个递归下降解析器。

use crate::error::{Error, Result};
use serde_json::{Map, Value};

pub fn parse(src: &str) -> Result<Value> {
    let chars: Vec<char> = src.chars().collect();
    let mut p = Parser { s: &chars, i: 0 };
    p.skip_ws();
    let v = p.value()?;
    p.skip_ws();
    // 尾部若还有内容（如广告片段），忽略即可，不视为错误
    Ok(v)
}

struct Parser<'a> {
    s: &'a [char],
    i: usize,
}

impl<'a> Parser<'a> {
    fn peek(&self) -> Option<char> {
        self.s.get(self.i).copied()
    }

    fn bump(&mut self) -> Option<char> {
        let c = self.peek();
        if c.is_some() {
            self.i += 1;
        }
        c
    }

    fn skip_ws(&mut self) {
        while let Some(c) = self.peek() {
            if c.is_whitespace() {
                self.i += 1;
            } else {
                break;
            }
        }
    }

    fn err<T>(&self, msg: &str) -> Result<T> {
        Err(Error::Parse(format!("{} @ 字符偏移 {}", msg, self.i)))
    }

    fn value(&mut self) -> Result<Value> {
        self.skip_ws();
        match self.peek() {
            Some('\'') => Ok(Value::String(self.string()?)),
            Some('"') => Ok(Value::String(self.double_quoted()?)),
            Some('{') => self.dict(),
            Some('[') => self.list(),
            Some(c) if c == '-' || c.is_ascii_digit() => self.number(),
            Some(_) => self.ident(),
            None => self.err("意外的输入结束"),
        }
    }

    /// 单引号字符串。裸 `'` 是终止符，`\'` 是转义。
    fn string(&mut self) -> Result<String> {
        if self.bump() != Some('\'') {
            return self.err("期望单引号");
        }
        let mut out = String::new();
        loop {
            match self.bump() {
                None => return self.err("字符串未闭合"),
                Some('\'') => break,
                Some('\\') => match self.bump() {
                    None => return self.err("字符串以反斜杠结尾"),
                    Some('n') => out.push('\n'),
                    Some('t') => out.push('\t'),
                    Some('r') => out.push('\r'),
                    Some('\\') => out.push('\\'),
                    Some('\'') => out.push('\''),
                    Some('"') => out.push('"'),
                    Some('u') => {
                        // \uXXXX —— 4 位十六进制
                        let mut hex = String::new();
                        for _ in 0..4 {
                            match self.bump() {
                                Some(c) => hex.push(c),
                                None => return self.err("\\u 转义不完整"),
                            }
                        }
                        let cp = u32::from_str_radix(&hex, 16)
                            .map_err(|_| Error::Parse(format!("非法 \\u 转义: {}", hex)))?;
                        if let Some(ch) = char::from_u32(cp) {
                            out.push(ch);
                        }
                    }
                    Some(other) => {
                        // 未知转义：原样保留反斜杠与字符，避免丢信息
                        out.push('\\');
                        out.push(other);
                    }
                },
                Some(c) => out.push(c),
            }
        }
        Ok(out)
    }

    /// 兼容真 JSON 的双引号字符串
    fn double_quoted(&mut self) -> Result<String> {
        self.bump();
        let mut out = String::new();
        loop {
            match self.bump() {
                None => return self.err("字符串未闭合"),
                Some('"') => break,
                Some('\\') => match self.bump() {
                    Some('n') => out.push('\n'),
                    Some('t') => out.push('\t'),
                    Some('r') => out.push('\r'),
                    Some('\\') => out.push('\\'),
                    Some('"') => out.push('"'),
                    Some('u') => {
                        let mut hex = String::new();
                        for _ in 0..4 {
                            hex.push(self.bump().unwrap_or('0'));
                        }
                        if let Ok(cp) = u32::from_str_radix(&hex, 16) {
                            if let Some(ch) = char::from_u32(cp) {
                                out.push(ch);
                            }
                        }
                    }
                    Some(other) => out.push(other),
                    None => return self.err("字符串以反斜杠结尾"),
                },
                Some(c) => out.push(c),
            }
        }
        Ok(out)
    }

    fn dict(&mut self) -> Result<Value> {
        self.bump(); // {
        let mut map = Map::new();
        loop {
            self.skip_ws();
            match self.peek() {
                Some('}') => {
                    self.bump();
                    break;
                }
                None => return self.err("对象未闭合"),
                _ => {}
            }
            // 键：单引号 / 双引号 / 裸标识符
            let key = match self.peek() {
                Some('\'') => self.string()?,
                Some('"') => self.double_quoted()?,
                _ => self.raw_ident()?,
            };
            self.skip_ws();
            match self.bump() {
                Some(':') => {}
                _ => return self.err("键后缺少冒号"),
            }
            let v = self.value()?;
            map.insert(key, v);
            self.skip_ws();
            match self.peek() {
                Some(',') => {
                    self.bump();
                }
                Some('}') => {}
                _ => return self.err("对象内缺少逗号或右花括号"),
            }
        }
        Ok(Value::Object(map))
    }

    fn list(&mut self) -> Result<Value> {
        self.bump(); // [
        let mut arr = Vec::new();
        loop {
            self.skip_ws();
            if self.peek() == Some(']') {
                self.bump();
                break;
            }
            let v = self.value()?;
            arr.push(v);
            self.skip_ws();
            match self.peek() {
                Some(',') => {
                    self.bump();
                }
                Some(']') => {}
                _ => return self.err("数组内缺少逗号或右方括号"),
            }
        }
        Ok(Value::Array(arr))
    }

    fn number(&mut self) -> Result<Value> {
        let start = self.i;
        if self.peek() == Some('-') {
            self.bump();
        }
        while let Some(c) = self.peek() {
            if c.is_ascii_digit() || c == '.' || c == 'e' || c == 'E' || c == '+' || c == '-' {
                self.bump();
            } else {
                break;
            }
        }
        let raw: String = self.s[start..self.i].iter().collect();
        if let Ok(i) = raw.parse::<i64>() {
            return Ok(Value::Number(i.into()));
        }
        if let Ok(f) = raw.parse::<f64>() {
            return Ok(serde_json::Number::from_f64(f)
                .map(Value::Number)
                .unwrap_or(Value::Null));
        }
        self.err(&format!("非法数字: {}", raw))
    }

    fn raw_ident(&mut self) -> Result<String> {
        let start = self.i;
        while let Some(c) = self.peek() {
            if c.is_alphanumeric() || c == '_' || c == '.' {
                self.bump();
            } else {
                break;
            }
        }
        let raw: String = self.s[start..self.i].iter().collect();
        if raw.is_empty() {
            return self.err("期望一个标识符");
        }
        Ok(raw)
    }

    fn ident(&mut self) -> Result<Value> {
        let raw = self.raw_ident()?;
        match raw.as_str() {
            "None" | "none" | "null" => Ok(Value::Null),
            "True" | "true" => Ok(Value::Bool(true)),
            "False" | "false" => Ok(Value::Bool(false)),
            // 未加引号的裸值，按字符串处理（例如 tag 列表里偶发的情况）
            other => Ok(Value::String(other.to_string())),
        }
    }
}

/// 取字符串字段，按需清洗
pub fn s(v: &Value, key: &str) -> String {
    v.get(key)
        .map(|x| match x {
            Value::String(s) => s.clone(),
            Value::Number(n) => n.to_string(),
            Value::Bool(b) => b.to_string(),
            Value::Null => String::new(),
            other => other.to_string(),
        })
        .unwrap_or_default()
}

/// 取整数字段（服务端常把数字放在字符串里）
pub fn u(v: &Value, key: &str) -> u32 {
    match v.get(key) {
        Some(Value::Number(n)) => n.as_u64().unwrap_or(0) as u32,
        Some(Value::String(s)) => s.trim().parse::<u32>().unwrap_or(0),
        _ => 0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_simple_dict() {
        let v = parse("{'PN':'0','TOTAL':'3600','abslist':[]}").unwrap();
        assert_eq!(s(&v, "PN"), "0");
        assert_eq!(u(&v, "TOTAL"), 3600);
        assert_eq!(v.get("abslist").unwrap().as_array().unwrap().len(), 0);
    }

    #[test]
    fn parses_nested_and_unquoted_numbers() {
        let v = parse("{'a':{'b':1,'c':[1,2,3]},'d':None,'e':True}").unwrap();
        assert_eq!(v["a"]["b"], Value::Number(1.into()));
        assert_eq!(v["a"]["c"].as_array().unwrap().len(), 3);
        assert_eq!(v["d"], Value::Null);
        assert_eq!(v["e"], Value::Bool(true));
    }

    #[test]
    fn handles_escaped_quote_and_unicode_escape() {
        let v = parse(r"{'k':'it\'s \u4e2d\u6587'}").unwrap();
        assert_eq!(s(&v, "k"), "it's 中文");
    }

    #[test]
    fn handles_multi_backslash() {
        // 原始响应里是 4 个反斜杠 + u0026
        let v = parse(r"{'a':'x\\\\u0026y'}").unwrap();
        // 解析后保留为 \\u0026，交给 text::clean 逐轮还原
        assert_eq!(s(&v, "a"), r"x\\u0026y");
    }

    #[test]
    fn tolerates_trailing_garbage() {
        let v = parse("{'a':'1'} trailing").unwrap();
        assert_eq!(s(&v, "a"), "1");
    }
}
