//! 순서를 지키는 JSON 값과 직렬화(C# `JsonMap`·`JsonNum`·`Dumps`). 공백 없이 쓴다.

use std::collections::HashMap;

use crate::natural::natural_cmp;

#[derive(Debug, Clone)]
pub(crate) enum J {
    Null,
    Bool(bool),
    Int(i64),
    /// 미리 문자열로 만든 숫자(따옴표 없이 들어간다).
    Num(String),
    Str(String),
    List(Vec<J>),
    Map(JsonMap),
}

impl J {
    /// C# 에서 null 일 수 있는 문자열: null 이면 "null" 로 쓴다.
    pub fn opt(s: &Option<String>) -> J {
        match s {
            Some(v) => J::Str(v.clone()),
            None => J::Null,
        }
    }

    pub fn str(s: &str) -> J {
        J::Str(s.to_string())
    }

    pub fn strings(items: &[String]) -> J {
        J::List(items.iter().map(|s| J::Str(s.clone())).collect())
    }
}

/// 순서를 지키는 JSON 객체. 이미 있는 키에 다시 쓰면 자리는 그대로 두고 값만 바꾼다.
#[derive(Debug, Clone, Default)]
pub(crate) struct JsonMap {
    entries: Vec<(String, J)>,
    index: HashMap<String, usize>,
}

impl JsonMap {
    pub fn new() -> JsonMap {
        JsonMap::default()
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn contains(&self, key: &str) -> bool {
        self.index.contains_key(key)
    }

    pub fn get(&self, key: &str) -> Option<&J> {
        self.index.get(key).map(|&i| &self.entries[i].1)
    }

    pub fn set(&mut self, key: &str, value: J) {
        match self.index.get(key) {
            Some(&i) => self.entries[i].1 = value,
            None => {
                self.index.insert(key.to_string(), self.entries.len());
                self.entries.push((key.to_string(), value));
            }
        }
    }

    pub fn get_or_add_map(&mut self, key: &str) -> &mut JsonMap {
        if !self.contains(key) {
            self.set(key, J::Map(JsonMap::new()));
        }
        let i = self.index[key];
        match &mut self.entries[i].1 {
            J::Map(m) => m,
            _ => panic!("JSON 값이 객체가 아닙니다: {}", key),
        }
    }

    /// 키를 이름 순서(자연 정렬, 안정 정렬)로 다시 늘어놓은 새 객체.
    pub fn sorted_natural(self) -> JsonMap {
        let mut entries = self.entries;
        entries.sort_by(|a, b| natural_cmp(&a.0, &b.0));
        let mut out = JsonMap::new();
        for (k, v) in entries {
            out.set(&k, v);
        }
        out
    }

    pub fn entries(&self) -> &[(String, J)] {
        &self.entries
    }

    pub fn into_entries(self) -> Vec<(String, J)> {
        self.entries
    }
}

pub(crate) fn dumps(value: &J) -> String {
    let mut sb = String::with_capacity(1 << 16);
    write(&mut sb, value);
    sb
}

pub(crate) fn write(sb: &mut String, value: &J) {
    match value {
        J::Null => sb.push_str("null"),
        J::Num(s) => sb.push_str(s),
        J::Map(map) => write_map_entries(sb, map.entries().iter().map(|(k, v)| (k, v))),
        J::Str(s) => write_string(sb, s),
        J::Bool(b) => sb.push_str(if *b { "true" } else { "false" }),
        J::Int(i) => sb.push_str(&i.to_string()),
        J::List(list) => {
            sb.push('[');
            let mut first = true;
            for item in list {
                if !first {
                    sb.push(',');
                }
                first = false;
                write(sb, item);
            }
            sb.push(']');
        }
    }
}

pub(crate) fn write_map_entries<'a>(sb: &mut String, entries: impl Iterator<Item = (&'a String, &'a J)>) {
    sb.push('{');
    let mut first = true;
    for (key, v) in entries {
        if !first {
            sb.push(',');
        }
        first = false;
        write_string(sb, key);
        sb.push(':');
        write(sb, v);
    }
    sb.push('}');
}

fn write_string(sb: &mut String, s: &str) {
    sb.push('"');
    for ch in s.chars() {
        match ch {
            '"' => sb.push_str("\\\""),
            '\\' => sb.push_str("\\\\"),
            '\n' => sb.push_str("\\n"),
            c if (c as u32) < 0x20 => {
                sb.push_str(&format!("\\u{:04x}", c as u32));
            }
            c => sb.push(c),
        }
    }
    sb.push('"');
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn escape_and_order() {
        let mut m = JsonMap::new();
        m.set("b", J::Int(1));
        m.set("a", J::str("x\"y\\z\n\t\u{1}한글"));
        m.set("b", J::Num("2.5".into()));
        m.set("n", J::Null);
        m.set("l", J::List(vec![J::Bool(true), J::Bool(false), J::Map(JsonMap::new()), J::List(vec![])]));
        assert_eq!(dumps(&J::Map(m)), r#"{"b":2.5,"a":"x\"y\\z\n\u0009\u0001한글","n":null,"l":[true,false,{},[]]}"#);
    }
}
