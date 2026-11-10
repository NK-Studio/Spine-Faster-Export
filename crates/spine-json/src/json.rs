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

// ---------------------------------------------------------------- 형식·Pretty print

/// Spine 내보내기 대화상자의 Format(libgdx `JsonWriter.OutputType`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum JsonFormat {
    /// 표준 JSON.
    #[default]
    Json,
    /// 이름은 꼭 필요할 때만 따옴표를 쓴다(올바른 JavaScript).
    JavaScript,
    /// 이름·값 대부분에 따옴표를 쓰지 않는다(libgdx 처럼 너그러운 파서가 필요).
    Minimal,
}

/// Pretty print 에서 한 줄에 두는 객체의 최대 항목 수(Spine 이 내보낸 공식 예제에서 확인: 6개까지 한 줄, 7개부터 줄 나눔).
const SINGLE_LINE_ENTRIES: usize = 6;

/// libgdx `OutputType`: `\`·CR·LF·탭만 바꾼 문자열(따옴표 없이 쓸 때의 모양).
fn bare(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            '\r' => out.push_str("\\r"),
            '\n' => out.push_str("\\n"),
            '\t' => out.push_str("\\t"),
            c => out.push(c),
        }
    }
    out
}

/// `^[a-zA-Z_$][a-zA-Z_$0-9]*$`
fn js_identifier(s: &str) -> bool {
    let mut chars = s.chars();
    let head = |c: char| c.is_ascii_alphabetic() || c == '_' || c == '$';
    matches!(chars.next(), Some(c) if head(c)) && chars.all(|c| head(c) || c.is_ascii_digit())
}

/// `^[^":,}/ ][^:]*$`
fn minimal_name(s: &str) -> bool {
    let mut chars = s.chars();
    matches!(chars.next(), Some(c) if !"\":,}/ ".contains(c)) && chars.all(|c| c != ':')
}

/// `^[^":,{\[\]/ ][^}\],]*$`
fn minimal_value(s: &str) -> bool {
    let mut chars = s.chars();
    matches!(chars.next(), Some(c) if !"\":,{[]/ ".contains(c)) && chars.all(|c| !"}],".contains(c))
}

fn write_name(sb: &mut String, name: &str, format: JsonFormat) {
    if format != JsonFormat::Json {
        let b = bare(name);
        let minimal = format == JsonFormat::Minimal && !name.contains("//") && !name.contains("/*") && minimal_name(&b);
        if minimal || js_identifier(&b) {
            sb.push_str(&b);
            return;
        }
    }
    write_string(sb, name);
}

fn write_value_string(sb: &mut String, s: &str, format: JsonFormat) {
    if format == JsonFormat::Minimal && !matches!(s, "true" | "false" | "null") && !s.contains("//") && !s.contains("/*") {
        let b = bare(s);
        if !b.is_empty() && !b.ends_with(' ') && minimal_value(&b) {
            sb.push_str(&b);
            return;
        }
    }
    write_string(sb, s);
}

fn write_scalar(sb: &mut String, value: &J, format: JsonFormat) {
    match value {
        J::Str(s) => write_value_string(sb, s, format),
        other => write(sb, other),
    }
}

/// 형식과 Pretty print 를 골라 쓴다. `Json` + pretty 끔은 [`dumps`] 와 같다.
pub(crate) fn dumps_styled(value: &J, format: JsonFormat, pretty: bool) -> String {
    if format == JsonFormat::Json && !pretty {
        return dumps(value);
    }
    let mut sb = String::with_capacity(1 << 16);
    if pretty {
        write_pretty(&mut sb, value, 0, format);
    } else {
        write_compact(&mut sb, value, format);
    }
    sb
}

fn write_compact(sb: &mut String, value: &J, format: JsonFormat) {
    match value {
        J::Map(map) => {
            sb.push('{');
            for (i, (k, v)) in map.entries().iter().enumerate() {
                if i > 0 {
                    sb.push(',');
                }
                write_name(sb, k, format);
                sb.push(':');
                write_compact(sb, v, format);
            }
            sb.push('}');
        }
        J::List(list) => {
            sb.push('[');
            for (i, v) in list.iter().enumerate() {
                if i > 0 {
                    sb.push(',');
                }
                write_compact(sb, v, format);
            }
            sb.push(']');
        }
        scalar => write_scalar(sb, scalar, format),
    }
}

fn is_container(v: &J) -> bool {
    matches!(v, J::Map(_) | J::List(_))
}

fn tabs(sb: &mut String, n: usize) {
    for _ in 0..n {
        sb.push('\t');
    }
}

/// Spine 의 Pretty print: 스칼라만 든 객체는 항목이 6개 이하면 한 줄에, 스칼라만 든 배열은 길어도 한 줄에 쓰고,
/// 객체·배열이 든 것은 항목마다 줄을 나눈다. 들여쓰기는 탭이고 맨 바깥 객체의 항목은 들여쓰지 않는다.
fn write_pretty(sb: &mut String, value: &J, indent: usize, format: JsonFormat) {
    // (이름, 값) 목록으로 본 자식. 배열이면 이름이 없다.
    let (open, close, children): (char, char, Vec<(Option<&str>, &J)>) = match value {
        J::Map(map) => ('{', '}', map.entries().iter().map(|(k, v)| (Some(k.as_str()), v)).collect()),
        J::List(list) => ('[', ']', list.iter().map(|v| (None, v)).collect()),
        scalar => return write_scalar(sb, scalar, format),
    };
    sb.push(open);
    if children.is_empty() {
        sb.push(close);
        return;
    }
    let new_lines = children.iter().any(|(_, v)| is_container(v)) || (open == '{' && children.len() > SINGLE_LINE_ENTRIES);
    sb.push(if new_lines { '\n' } else { ' ' });
    for (i, (name, child)) in children.iter().enumerate() {
        if new_lines {
            tabs(sb, indent);
        }
        if let Some(name) = name {
            write_name(sb, name, format);
            sb.push_str(": ");
        }
        write_pretty(sb, child, indent + 1, format);
        if (!new_lines || format != JsonFormat::Minimal) && i + 1 < children.len() {
            sb.push(',');
        }
        sb.push(if new_lines { '\n' } else { ' ' });
    }
    if new_lines {
        tabs(sb, indent.saturating_sub(1));
    }
    sb.push(close);
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

    fn sample() -> J {
        let mut bone = JsonMap::new();
        bone.set("name", J::str("front arm"));
        bone.set("parent", J::str("root"));
        bone.set("x", J::Num("1.5".into()));
        let mut m = JsonMap::new();
        m.set("bones", J::List(vec![J::Map(bone)]));
        m.set("my-key", J::List(vec![J::Int(1), J::Int(2)]));
        m.set("flag", J::str("true"));
        J::Map(m)
    }

    #[test]
    fn formats() {
        let v = sample();
        assert_eq!(dumps_styled(&v, JsonFormat::Json, false), dumps(&v));
        assert_eq!(dumps_styled(&v, JsonFormat::JavaScript, false), r#"{bones:[{name:"front arm",parent:"root",x:1.5}],"my-key":[1,2],flag:"true"}"#);
        assert_eq!(dumps_styled(&v, JsonFormat::Minimal, false), r#"{bones:[{name:front arm,parent:root,x:1.5}],my-key:[1,2],flag:"true"}"#);
    }

    #[test]
    fn pretty() {
        let v = sample();
        assert_eq!(
            dumps_styled(&v, JsonFormat::Json, true),
            "{\n\"bones\": [\n\t{ \"name\": \"front arm\", \"parent\": \"root\", \"x\": 1.5 }\n],\n\"my-key\": [ 1, 2 ],\n\"flag\": \"true\"\n}"
        );
        assert_eq!(dumps_styled(&v, JsonFormat::Minimal, true), "{\nbones: [\n\t{ name: front arm, parent: root, x: 1.5 }\n]\nmy-key: [ 1, 2 ]\nflag: \"true\"\n}");
    }
}
