//! .spine 본문(압축을 푼 바이트)을 스키마대로 읽어 객체 그래프로 만든다(Kryo 참조 규칙).
//! C# `SpineProjectDocument`(+ `KryoGraph`)를 옮겼다. 객체 본문은 «원래 바이트 조각»과 «슬롯»의 나열로 기록한다.
//! 슬롯은 처음 나온 곳에서는 객체 본문을 품고(새 객체), 그 뒤로는 번호로 가리킨다(역참조).
//! 역참조는 읽을 때 바로 노드 번호로 풀어 둔다(C# 의 NodeRef → ById 조회와 결과가 같다).

use std::collections::HashMap;
use std::fmt::Write as _;
use std::sync::Arc;

use spine_model::SpineError;

use crate::schema::{self, Cls, Kind, Schema, Table, Ty};

pub type NodeId = u32;

/// 필드·리스트 원소 값. C# object(long·float·string·Node·NodeRef·List&lt;object&gt;·null).
#[derive(Clone, Debug)]
pub enum Value {
    Null,
    Int(i64),
    Float(f32),
    Str(Arc<str>),
    Node(NodeId),
    List(Box<[Value]>),
}

#[derive(Clone, Debug)]
pub struct Field {
    pub key: u32,
    pub at: u32,
    pub end: Option<u32>,
    pub value: Value,
}

#[derive(Clone, Copy, Debug)]
pub enum Part {
    /// 본문 배열의 [start, end) 구간.
    Span { start: u32, end: u32 },
    /// 다른 객체를 담는 자리. prefix: 클래스 번호(없으면 표지부터 쓴다).
    Slot { start: u32, prefix: Option<i32>, obj: NodeId },
}

impl Part {
    pub fn start(&self) -> u32 {
        match *self {
            Part::Span { start, .. } | Part::Slot { start, .. } => start,
        }
    }
}

#[derive(Debug)]
pub struct Node {
    pub cls: Cls,
    pub table: Table,
    pub start: u32,
    pub end: u32,
    last: u32,
    pub fields: Vec<Field>,
    pub items: Vec<Value>,
    pub parts: Vec<Part>,
}

impl Node {
    pub fn field(&self, key: u32) -> Option<&Field> {
        self.fields.iter().find(|f| f.key == key)
    }

    pub fn has(&self, key: u32) -> bool {
        self.fields.iter().any(|f| f.key == key)
    }
}

/// 필드 키 문자열 표. 키마다 태그 번호(ProjectView.KeyTag)와 'payload' 접두사 여부를 미리 계산한다.
#[derive(Debug)]
pub struct Keys {
    map: HashMap<Box<str>, u32>,
    names: Vec<Box<str>>,
    pub tags: Vec<Option<i32>>,
    pub payload: Vec<bool>,
}

pub const KEY_S: u32 = 0;
pub const KEY_V: u32 = 1;
pub const KEY_TYPE: u32 = 2;
pub const KEY_KEYS: u32 = 3;
pub const KEY_TIME: u32 = 4;
pub const KEY_FLAG: u32 = 5;
pub const KEY_P0: u32 = 6;
pub const KEY_P1: u32 = 7;
pub const KEY_0: u32 = 8;
pub const KEY_8: u32 = 9;
pub const KEY_9: u32 = 10;
const FIXED_KEYS: [&str; 11] = ["s", "v", "type", "keys", "time", "flag", "p0", "p1", "0", "8", "9"];

impl Keys {
    fn new() -> Keys {
        let mut k = Keys { map: HashMap::with_capacity(256), names: Vec::new(), tags: Vec::new(), payload: Vec::new() };
        for name in FIXED_KEYS {
            k.intern(name);
        }
        k
    }

    fn intern(&mut self, s: &str) -> u32 {
        if let Some(&id) = self.map.get(s) {
            return id;
        }
        let id = self.names.len() as u32;
        self.names.push(s.into());
        self.map.insert(s.into(), id);
        self.tags.push(key_tag(s));
        self.payload.push(s.starts_with("payload"));
        id
    }

    pub fn find(&self, s: &str) -> Option<u32> {
        self.map.get(s).copied()
    }

    pub fn name(&self, id: u32) -> &str {
        &self.names[id as usize]
    }
}

/// 필드 키 → 태그 번호(키프레임의 'time' 같은 이름 키는 None). C# ProjectView.KeyTag.
fn key_tag(key: &str) -> Option<i32> {
    let k = key.trim_start_matches(['t', 'd']);
    let k = match k.find(':') {
        Some(i) => &k[i + 1..],
        None => k,
    };
    if k.is_empty() || !k.bytes().all(|c| c.is_ascii_digit()) {
        return None;
    }
    k.parse::<i32>().ok()
}

/// 읽은 그래프.
#[derive(Debug)]
pub struct Document {
    pub schema: &'static Schema,
    pub nodes: Vec<Node>,
    /// 다 읽은 객체(C# Nodes, 읽기를 마친 순서).
    pub done: Vec<NodeId>,
    pub keys: Keys,
    pub version: Option<String>,
    pub top: NodeId,
    /// 부분 읽기에서 멈췄을 때의 Project 객체.
    pub partial_root: Option<NodeId>,
    pub partial_error: Option<String>,
}

impl Document {
    pub fn node(&self, id: NodeId) -> &Node {
        &self.nodes[id as usize]
    }

    /// 클래스 이름으로 다 읽은 객체들(본문 시작 위치 순).
    pub fn of(&self, cls: &str) -> Vec<NodeId> {
        let Some(c) = self.schema.find_class(cls) else { return Vec::new() };
        let mut v: Vec<NodeId> = self.done.iter().copied().filter(|&n| self.nodes[n as usize].cls == c).collect();
        v.sort_by_key(|&n| self.nodes[n as usize].start);
        v
    }

    /// 본문을 읽는다. allow_partial: Project 안 뒤쪽 편집기 상태에서 막혀도 스켈레톤 목록(필드 0)을
    /// 끝까지 읽었다면 오류 대신 그때까지의 그래프를 남긴다.
    pub fn parse(body: &[u8], schema: &'static Schema, allow_partial: bool) -> Result<Document, SpineError> {
        let expected = (body.len() / 24).max(64);
        let mut p = Parser {
            b: body,
            p: 0,
            s: schema,
            nodes: Vec::with_capacity(expected),
            done: Vec::with_capacity(expected),
            keys: Keys::new(),
            objs: [Vec::with_capacity(expected / 2), Vec::with_capacity(expected / 2)],
            stack: Vec::with_capacity(64),
            tl_type: None,
            kl_prev: 0,
            kb: String::with_capacity(32),
            u16buf: Vec::with_capacity(64),
        };
        let top = p.new_node(schema::TOP, 0);
        p.stack.push(top);
        for _ in 0..3 {
            p.varint().map_err(SpineError::Format)?;
        }
        let version = p.kstring().map_err(SpineError::Format)?;
        let mut partial_root = None;
        let mut partial_error = None;
        if let Err(e) = p.run(top) {
            let root = if allow_partial { p.keep_partial() } else { None };
            match root {
                Some(r) => {
                    partial_root = Some(r);
                    partial_error = Some(e);
                }
                None => return Err(SpineError::Format(e)),
            }
        }
        Ok(Document {
            schema,
            nodes: p.nodes,
            done: p.done,
            keys: p.keys,
            version: version.map(|s| s.to_string()),
            top,
            partial_root,
            partial_error,
        })
    }
}

type R<T> = Result<T, String>;

struct Parser<'a> {
    b: &'a [u8],
    p: usize,
    s: &'static Schema,
    nodes: Vec<Node>,
    done: Vec<NodeId>,
    keys: Keys,
    objs: [Vec<NodeId>; 2],
    stack: Vec<NodeId>,
    tl_type: Option<i32>,
    kl_prev: i32,
    kb: String,
    u16buf: Vec<u16>,
}

enum Marker {
    Int(i64),
    Ref(NodeId),
}

fn table_index(t: Table) -> usize {
    if t == Table::A {
        0
    } else {
        1
    }
}

impl<'a> Parser<'a> {
    // ------------------------------------------------------------ 바이트 읽기

    fn fail_at(&self, message: impl std::fmt::Display, at: usize) -> String {
        format!("@0x{at:x}: {message}")
    }

    fn fail(&self, message: impl std::fmt::Display) -> String {
        self.fail_at(message, self.p)
    }

    fn eof(&self) -> String {
        self.fail("본문이 끝났습니다")
    }

    #[inline]
    fn take_byte(&mut self) -> R<u8> {
        match self.b.get(self.p) {
            Some(&x) => {
                self.p += 1;
                Ok(x)
            }
            None => Err(self.eof()),
        }
    }

    #[inline]
    fn varint(&mut self) -> R<i64> {
        let mut r: i64 = 0;
        let mut sh: u32 = 0;
        loop {
            let x = self.take_byte()?;
            if sh < 63 {
                r |= ((x & 0x7F) as i64) << sh;
            }
            sh = sh.saturating_add(7);
            if x & 0x80 == 0 {
                return Ok(r);
            }
        }
    }

    /// Kryo 문자열. 길이 0 이면 null.
    fn kstring(&mut self) -> R<Option<Arc<str>>> {
        let b0 = self.take_byte()?;
        if b0 & 0x80 == 0 {
            let start = self.p - 1;
            loop {
                let x = self.take_byte()?;
                if x & 0x80 != 0 {
                    let mut s = String::with_capacity(self.p - start);
                    for &c in &self.b[start..self.p] {
                        s.push((c & 0x7F) as char);
                    }
                    return Ok(Some(s.into()));
                }
            }
        }

        let mut n: i32 = (b0 & 0x3F) as i32;
        let mut sh: u32 = 6;
        if b0 & 0x40 != 0 {
            loop {
                let x = self.take_byte()?;
                n |= ((x & 0x7F) as i32).wrapping_shl(sh);
                sh += 7;
                if x & 0x80 == 0 {
                    break;
                }
            }
        }
        if n == 0 {
            return Ok(None);
        }

        let mut chars = std::mem::take(&mut self.u16buf);
        chars.clear();
        let mut i = 0;
        let mut result = Ok(());
        while i < n - 1 {
            let mut c = match self.take_byte() {
                Ok(c) => c as i32,
                Err(e) => {
                    result = Err(e);
                    break;
                }
            };
            if c >= 0xE0 {
                let r = (|| -> R<i32> {
                    let b1 = self.take_byte()? as i32;
                    let b2 = self.take_byte()? as i32;
                    Ok(((c & 0x0F) << 12) | ((b1 & 0x3F) << 6) | (b2 & 0x3F))
                })();
                match r {
                    Ok(v) => c = v,
                    Err(e) => {
                        result = Err(e);
                        break;
                    }
                }
            } else if c >= 0xC0 {
                match self.take_byte() {
                    Ok(b1) => c = ((c & 0x1F) << 6) | (b1 as i32 & 0x3F),
                    Err(e) => {
                        result = Err(e);
                        break;
                    }
                }
            }
            chars.push(c as u16);
            i += 1;
        }
        let s = result.map(|_| Some(String::from_utf16_lossy(&chars).into()));
        self.u16buf = chars;
        s
    }

    fn float(&mut self) -> R<f32> {
        if self.p + 4 > self.b.len() {
            return Err(self.eof());
        }
        let bits = u32::from_be_bytes([self.b[self.p], self.b[self.p + 1], self.b[self.p + 2], self.b[self.p + 3]]);
        self.p += 4;
        Ok(f32::from_bits(bits))
    }

    fn skip(&mut self, n: i64) -> R<()> {
        if n < 0 || (self.p as i64).saturating_add(n) > self.b.len() as i64 {
            return Err(self.eof());
        }
        self.p += n as usize;
        Ok(())
    }

    // ------------------------------------------------------------ 그래프 기록

    fn flush(&mut self, node: NodeId) {
        let p = self.p as u32;
        let n = &mut self.nodes[node as usize];
        if p > n.last {
            n.parts.push(Part::Span { start: n.last, end: p });
        }
        n.last = p;
    }

    fn add_slot(&mut self, s0: usize, prefix: Option<i32>, obj: NodeId) {
        let parent = *self.stack.last().expect("스택이 비었습니다");
        let p = self.p as u32;
        let s0 = s0 as u32;
        let n = &mut self.nodes[parent as usize];
        if s0 > n.last {
            n.parts.push(Part::Span { start: n.last, end: s0 });
        }
        n.parts.push(Part::Slot { start: s0, prefix, obj });
        n.last = p;
    }

    fn new_node(&mut self, cls: Cls, start: usize) -> NodeId {
        let table = self.s.table_of(cls);
        let id = self.nodes.len() as NodeId;
        if (table == Table::A || table == Table::B) && cls != schema::TOP {
            self.objs[table_index(table)].push(id);
        }
        self.nodes.push(Node {
            cls,
            table,
            start: start as u32,
            end: start as u32,
            last: start as u32,
            fields: Vec::new(),
            items: Vec::new(),
            parts: Vec::new(),
        });
        id
    }

    fn finish(&mut self, node: NodeId) {
        self.flush(node);
        self.stack.pop();
        self.nodes[node as usize].end = self.p as u32;
        self.done.push(node);
    }

    fn set_field(&mut self, node: NodeId, key: u32, at: usize, end: Option<usize>, value: Value) {
        let f = Field { key, at: at as u32, end: end.map(|e| e as u32), value };
        let fields = &mut self.nodes[node as usize].fields;
        if let Some(old) = fields.iter_mut().find(|x| x.key == key) {
            *old = f;
        } else {
            fields.push(f);
        }
    }

    /// 참조 표지: 0 = null, 1 = 새 객체, n = 이미 읽은 n-2번 객체(클래스가 속한 표에서).
    fn marker(&mut self, cls: Cls) -> R<Marker> {
        let at = self.p;
        let r = self.varint()?;
        if r < 2 {
            return Ok(Marker::Int(r));
        }
        let tb = self.s.table_of(cls);
        let tid = r - 2;
        let bad = match tb {
            Table::None | Table::Flag => true,
            _ => {
                let list = &self.objs[table_index(tb)];
                tid >= list.len() as i64 || (cls != schema::STAR_A && self.nodes[list[tid as usize] as usize].cls != cls)
            }
        };
        if bad {
            let tname = match tb {
                Table::A => "A",
                Table::B => "B",
                Table::None => "none",
                Table::Flag => "flag",
            };
            return Err(self.fail_at(format!("역참조 {}{}가 {}가 아닙니다", tname, tid, self.s.class_name(cls)), at));
        }
        Ok(Marker::Ref(self.objs[table_index(tb)][tid as usize]))
    }

    fn string_slot(&mut self) -> R<Value> {
        let s0 = self.p;
        match self.marker(schema::STRING)? {
            Marker::Int(1) => {
                let node = self.new_node(schema::STRING, s0);
                self.nodes[node as usize].last = self.p as u32;
                self.stack.push(node);
                let at = self.p;
                let v = match self.kstring()? {
                    Some(s) => Value::Str(s),
                    None => Value::Null,
                };
                self.set_field(node, KEY_S, at, None, v.clone());
                self.finish(node);
                self.add_slot(s0, None, node);
                Ok(v)
            }
            Marker::Ref(r) => {
                self.add_slot(s0, None, r);
                Ok(self.nodes[r as usize].field(KEY_S).map(|f| f.value.clone()).unwrap_or(Value::Null))
            }
            Marker::Int(_) => Ok(Value::Null),
        }
    }

    fn obj_slot(&mut self) -> R<Value> {
        let at = self.p;
        let c = self.varint()?;
        if c == 0 {
            return Ok(Value::Null);
        }
        let Some(&cls) = self.s.names.get(&c) else {
            return Err(self.fail_at(format!("모르는 클래스 번호 {c}"), at));
        };
        if self.s.table_of(cls) == Table::None {
            let plain = self.body(cls, at)?;
            self.add_slot(at, Some(c as i32), plain);
            return Ok(Value::Node(plain));
        }
        match self.marker(cls)? {
            Marker::Int(1) => {
                let node = self.body(cls, at)?;
                self.add_slot(at, Some(c as i32), node);
                Ok(Value::Node(node))
            }
            Marker::Ref(r) => {
                self.add_slot(at, Some(c as i32), r);
                Ok(Value::Node(r))
            }
            Marker::Int(_) => Err(self.fail_at(format!("클래스 {} 뒤 표지가 0입니다", self.s.class_name(cls)), at)),
        }
    }

    fn typed_slot(&mut self, cls: Cls) -> R<Value> {
        let at = self.p;
        match self.marker(cls)? {
            Marker::Int(1) => {
                let mut cls = cls;
                if cls == schema::STAR_A {
                    // 대상 클래스가 바이트에 없다: «필드 수:첫 태그»로 판별(스키마 anyA)
                    let save = self.p;
                    let a = self.varint()?;
                    let b = self.varint()?;
                    let key = format!("{a}:{b}");
                    self.p = save;
                    let mut found = self.s.any_a.iter().find(|(k, _)| *k == key).map(|(_, c)| *c);
                    if found.is_none() {
                        let count = a.to_string();
                        let mut same: Vec<Cls> = Vec::new();
                        for (k, c) in &self.s.any_a {
                            if k.split(':').next() == Some(count.as_str()) && !same.contains(c) {
                                same.push(*c);
                            }
                        }
                        if same.len() == 1 {
                            found = Some(same[0]);
                        }
                    }
                    match found {
                        Some(f) => cls = f,
                        None => {
                            return Err(self.fail_at(format!("*A 슬롯의 새 객체를 판별할 수 없습니다(필드 수:첫 태그 = {key})"), at))
                        }
                    }
                }
                let node = self.body(cls, at)?;
                self.add_slot(at, None, node);
                Ok(Value::Node(node))
            }
            Marker::Ref(r) => {
                self.add_slot(at, None, r);
                Ok(Value::Node(r))
            }
            Marker::Int(_) => Ok(Value::Null),
        }
    }

    fn body(&mut self, cls: Cls, at: usize) -> R<NodeId> {
        let s = self.s;
        let info = s.class(cls);
        let Some(kind) = info.kind else {
            return Err(self.fail_at(format!("모르는 클래스 {}", info.name), at));
        };
        let node = self.new_node(cls, at);
        self.nodes[node as usize].last = self.p as u32;
        self.stack.push(node);
        match kind {
            Kind::Tagged => match &info.groups {
                Some(groups) => self.read_grouped(node, cls, groups)?,
                None => self.read_tagged(node, cls)?,
            },
            Kind::Timeline => {
                if s.timeline_refs {
                    self.field(node, cls, "group", Ty::T)?;
                }
                let type_at = self.p;
                let ty = self.varint()? as i32;
                self.set_field(node, KEY_TYPE, type_at, None, Value::Int(ty as i64));
                let Some(lay) = s.timelines.get(&(ty as i64)) else {
                    return Err(self.fail(format!("모르는 타임라인 종류 {ty}")));
                };
                let saved = self.tl_type;
                self.tl_type = Some(ty);
                self.field(node, cls, "keys", Ty::T)?;
                self.tl_type = saved;
                if let Some(extra) = lay.extra {
                    self.field(node, cls, &format!("extra{ty}"), extra)?;
                }
                if let Some(extra2) = lay.extra2 {
                    self.field(node, cls, &format!("extraB{ty}"), extra2)?; // 시퀀스 타임라인: 슬롯 + 첨부
                }
            }
            Kind::Keyframe => {
                let lay = match self.tl_type.and_then(|t| s.timelines.get(&(t as i64)).map(|l| (t, l))) {
                    Some(x) => x,
                    None => return Err(self.fail("타임라인 밖의 키")),
                };
                let (tl, lay) = lay;
                if s.timeline_refs {
                    self.field(node, cls, "group", Ty::T)?;
                    self.field(node, cls, "timeline", Ty::T)?;
                }
                self.field(node, cls, "time", Ty::F)?;
                if let Some(cond) = &s.curve_condition {
                    // 핸들 블록(들어오는·나가는 핸들): 이 키나 앞 키가 베지어(1)일 때만
                    self.skip(lay.vals)?;
                    let vat = self.p;
                    let flag = self.varint()?;
                    self.set_field(node, KEY_FLAG, vat, Some(self.p), Value::Int(flag));
                    if flag == 1 || (self.kl_prev == 1 && cond != "own") {
                        self.skip(20 * lay.curves)?;
                    }
                    self.kl_prev = flag as i32;
                } else {
                    self.skip(lay.vals + 20 * lay.curves)?;
                    self.field(node, cls, "flag", Ty::V)?;
                }
                if let Some(payload) = lay.payload {
                    self.field(node, cls, &format!("payload{tl}"), payload)?;
                }
            }
            Kind::Plain => {
                let Some(types) = &info.plain else {
                    return Err(self.fail_at(format!("모르는 클래스 {}", info.name), at));
                };
                for (i, &ty) in types.iter().enumerate() {
                    self.field(node, cls, &format!("p{i}"), ty)?;
                }
            }
            Kind::List | Kind::Map => {
                let n = self.varint()?;
                let saved = self.kl_prev;
                self.kl_prev = 0;
                let total = n.wrapping_mul(if kind == Kind::Map { 2 } else { 1 });
                let mut i = 0i64;
                while i < total {
                    let v = self.obj_slot()?;
                    self.nodes[node as usize].items.push(v);
                    i += 1;
                }
                self.kl_prev = saved;
            }
            Kind::Str => {
                let vat = self.p;
                let v = match self.kstring()? {
                    Some(s) => Value::Str(s),
                    None => Value::Null,
                };
                self.set_field(node, KEY_S, vat, None, v);
            }
            Kind::Varint => {
                let vat = self.p;
                let v = self.varint()?;
                self.set_field(node, KEY_V, vat, None, Value::Int(v));
            }
            Kind::Fixed(n) => self.skip(n)?,
            Kind::RfArray | Kind::RvArray | Kind::RsArray | Kind::FArray => {
                let mut n = self.varint()?;
                if kind == Kind::FArray {
                    n = (n - 1).max(0);
                }
                if kind == Kind::RvArray {
                    let mut i = 0;
                    while i < n {
                        self.varint()?;
                        i += 1;
                    }
                } else {
                    let size = if kind == Kind::RsArray { 2 } else { 4 };
                    self.skip(n.wrapping_mul(size) as i32 as i64)?;
                }
            }
            Kind::KStrArray => {
                let n = (self.varint()? - 1).max(0);
                for _ in 0..n {
                    self.kstring()?;
                }
            }
            Kind::EnumArray => {
                let n = (self.varint()? - 1).max(0);
                for _ in 0..n {
                    self.typed_slot(schema::VIDEO_ENUM_A)?;
                }
            }
            Kind::BoxFArray => {
                let n = (self.varint()? - 1).max(0);
                for _ in 0..n {
                    if self.varint()? != 0 {
                        self.skip(4)?;
                    }
                }
            }
            Kind::DefVtx | Kind::DefVtx0 => {
                let total = self.varint()?;
                let chunks = self.varint()?;
                let mut i = 0;
                while i < chunks {
                    // 묶음 머리는 varint 가 아니라 1바이트다(0x80 도 나온다)
                    if self.take_byte()? != 0 || kind == Kind::DefVtx {
                        let n = 128.min(total - 128 * i);
                        self.skip((4 * n) as i32 as i64)?;
                    }
                    i += 1;
                }
            }
            Kind::Unknown => return Err(self.fail(format!("모르는 종류 {}", info.kind_name))),
        }
        self.finish(node);
        Ok(node)
    }

    fn read_grouped(&mut self, node: NodeId, cls: Cls, groups: &[Vec<i32>]) -> R<()> {
        // 묶음 방식(4.3 정식 첨부): 묶음 순서대로 나오고 묶음 안 순서는 빌드마다 다르다.
        // 적힌 필드 수는 실제보다 작으므로 쓰지 않고, 묶음마다 모든 필드를 읽으면 다음 묶음으로 간다.
        self.varint()?;
        let mut need: Vec<Vec<i32>> = groups
            .iter()
            .map(|g| {
                let mut v: Vec<i32> = Vec::with_capacity(g.len());
                for &t in g {
                    if !v.contains(&t) {
                        v.push(t);
                    }
                }
                v
            })
            .collect();
        let total: usize = need.iter().map(|g| g.len()).sum();
        let mut gi = 0;
        for _ in 0..total {
            while need[gi].is_empty() {
                gi += 1;
            }
            let tat = self.p;
            let tag = self.varint()? as i32;
            let mut kb = std::mem::take(&mut self.kb);
            kb.clear();
            if gi == 0 {
                let _ = write!(kb, "{tag}");
            } else {
                let _ = write!(kb, "{gi}:{tag}");
            }
            let removed = match need[gi].iter().position(|&t| t == tag) {
                Some(i) => {
                    need[gi].swap_remove(i);
                    true
                }
                None => false,
            };
            if !removed {
                let e = self.fail_at(format!("모르는 필드 {}.{}", self.s.class_name(cls), kb), tat);
                self.kb = kb;
                return Err(e);
            }
            let key = self.keys.intern(&kb);
            self.kb = kb;
            self.read_field(node, cls, key, tat)?;
        }
        Ok(())
    }

    fn read_tagged(&mut self, node: NodeId, cls: Cls) -> R<()> {
        let tail = self.s.class(cls).tail;
        let n0 = self.varint()?;
        let n = n0 + tail;
        self.nodes[node as usize].fields = Vec::with_capacity(n.clamp(0, 64) as usize);
        let mut last = -1i32;
        let mut level = 0;
        let mut i = 0i64;
        while i < n {
            let tat = self.p;
            let tag = self.varint()? as i32;
            let mut kb = std::mem::take(&mut self.kb);
            kb.clear();
            let key;
            if self.s.ascending {
                // 4.3 베타: 클래스 단계마다 태그 오름차순, 내려가면 상위 단계
                if tag <= last {
                    level += 1;
                }
                last = tag;
                if level == 0 {
                    let _ = write!(kb, "{tag}");
                } else {
                    let _ = write!(kb, "{level}:{tag}");
                }
                key = self.keys.intern(&kb);
            } else {
                // 4.2: 개수 뒤 꼬리 필드는 t<태그>, 같은 태그가 다시 나오면 d<태그>
                if i < n0 {
                    let _ = write!(kb, "{tag}");
                } else {
                    let _ = write!(kb, "t{tag}");
                }
                let mut k = self.keys.intern(&kb);
                while self.nodes[node as usize].has(k) {
                    kb.insert(0, 'd');
                    k = self.keys.intern(&kb);
                }
                key = k;
            }
            self.kb = kb;
            self.read_field(node, cls, key, tat)?;
            i += 1;
        }
        Ok(())
    }

    fn read_field(&mut self, node: NodeId, cls: Cls, key: u32, tat: usize) -> R<()> {
        let ty = self.s.class(cls).fields.as_ref().and_then(|f| f.get(self.keys.name(key)).copied());
        let Some(ty) = ty else {
            return Err(self.fail_at(format!("모르는 필드 {}.{}", self.s.class_name(cls), self.keys.name(key)), tat));
        };
        let vat = self.p;
        let val = self.value(cls, key, ty)?;
        self.set_field(node, key, vat, Some(self.p), val);
        Ok(())
    }

    fn field(&mut self, node: NodeId, cls: Cls, key: &str, ty: Ty) -> R<()> {
        let key = self.keys.intern(key);
        let vat = self.p;
        let val = self.value(cls, key, ty)?;
        self.set_field(node, key, vat, Some(self.p), val);
        Ok(())
    }

    fn value(&mut self, cls: Cls, key: u32, ty: Ty) -> R<Value> {
        match ty {
            Ty::V => Ok(Value::Int(self.varint()?)),
            Ty::VB => {
                // 정수 + 표지 1바이트(참조 번호를 쓰지 않는다)
                let v = self.varint()?;
                self.take_byte()?;
                Ok(Value::Int(v))
            }
            Ty::F => Ok(Value::Float(self.float()?)),
            Ty::S => self.string_slot(),
            Ty::O => self.obj_slot(),
            Ty::T => {
                let target = self.s.class(cls).slots.get(self.keys.name(key)).copied();
                match target {
                    Some(t) => self.typed_slot(t),
                    None => Err(self.fail(format!("{}.{}의 선언 클래스가 없습니다", self.s.class_name(cls), self.keys.name(key)))),
                }
            }
            Ty::N => {
                // 본문 없는 새 객체(클래스 + 표지 1)
                let at = self.p;
                let c = self.varint()?;
                let cls2 = self.s.names.get(&c).copied();
                if self.varint()? != 1 {
                    return Err(self.fail_at("N 슬롯이 새 객체가 아닙니다", at));
                }
                let Some(cls2) = cls2 else {
                    return Err(self.fail_at(format!("모르는 클래스 번호 {c}"), at));
                };
                let node = self.new_node(cls2, at);
                self.nodes[node as usize].end = self.p as u32;
                self.done.push(node);
                self.add_slot(at, Some(c as i32), node);
                Ok(Value::Null)
            }
            Ty::LO => {
                let n = self.varint()?;
                let mut list = Vec::new();
                let mut i = 0;
                while i < n {
                    list.push(self.obj_slot()?);
                    i += 1;
                }
                Ok(Value::List(list.into_boxed_slice()))
            }
            Ty::Unknown(_) => Err(self.fail(format!("모르는 값 타입 {}", self.s.ty_name(ty)))),
        }
    }

    // ------------------------------------------------------------ 루트

    fn run(&mut self, top: NodeId) -> R<()> {
        if self.s.root_varint {
            self.varint()?; // 루트 앞 값(4.x: 스켈레톤 수)
        }
        if self.s.root_pad > 0 {
            self.skip(self.s.root_pad)?; // 4.3.07~: 16바이트 값(프로젝트 ID로 보임)
        }
        self.typed_slot(schema::PROJECT)?; // 루트는 클래스 없이 표지부터 쓰는 Project

        // 뒤쪽 에디터 상태 구간: 첫 값 1은 새 객체 표지라 참조 번호를 하나 쓴다
        let tail_at = self.p;
        if self.varint()? == 1 {
            let tail = self.new_node(schema::EDITOR_TAIL, tail_at);
            self.nodes[tail as usize].last = self.p as u32;
            self.stack.push(tail);
            self.read_tail()?;
            self.finish(tail);
            self.add_slot(tail_at, None, tail);
        } else {
            self.read_tail()?;
        }
        self.flush(top);
        self.nodes[top as usize].end = self.p as u32;
        Ok(())
    }

    fn read_tail(&mut self) -> R<()> {
        self.varint()?;
        while self.p < self.b.len() {
            self.obj_slot()?;
        }
        Ok(())
    }

    fn keep_partial(&self) -> Option<NodeId> {
        let root = *self.stack.get(1)?;
        let n = &self.nodes[root as usize];
        if n.cls != schema::PROJECT || !n.has(KEY_0) {
            return None;
        }
        Some(root)
    }
}

/// 본문 머리의 포맷 문자열만 읽는다(스키마 고르기용). ASCII 가 아니면 None.
pub fn read_version(body: &[u8]) -> Result<Option<String>, SpineError> {
    let short = || SpineError::Format("본문이 너무 짧습니다".to_string());
    let mut p = 0usize;
    let next = |p: &mut usize| -> Result<u8, SpineError> {
        let x = *body.get(*p).ok_or_else(short)?;
        *p += 1;
        Ok(x)
    };
    for _ in 0..3 {
        while next(&mut p)? & 0x80 != 0 {}
    }
    let b0 = next(&mut p)?;
    if b0 & 0x80 != 0 {
        return Ok(None); // 포맷 문자열은 ASCII다
    }
    let mut s = String::new();
    s.push(b0 as char);
    loop {
        let x = next(&mut p)?;
        s.push((x & 0x7F) as char);
        if x & 0x80 != 0 {
            return Ok(Some(s));
        }
    }
}

// ------------------------------------------------------------ 다시 쓰기 확인

/// 그래프를 본문으로 다시 쓴 결과가 원본과 같은지(C# `SpineProjectWriter.Serialize(doc) == body`).
/// 바이트를 만들지 않고 원본과 바로 견준다.
pub fn rewrite_equals(doc: &Document, body: &[u8]) -> bool {
    struct W<'a> {
        doc: &'a Document,
        body: &'a [u8],
        pos: usize,
        ids: Vec<u32>,
        count: [u32; 2],
    }

    impl W<'_> {
        fn put(&mut self, bytes: &[u8]) -> bool {
            if self.body.len() - self.pos >= bytes.len() && &self.body[self.pos..self.pos + bytes.len()] == bytes {
                self.pos += bytes.len();
                true
            } else {
                false
            }
        }

        fn put_byte(&mut self, b: u8) -> bool {
            if self.body.get(self.pos) == Some(&b) {
                self.pos += 1;
                true
            } else {
                false
            }
        }

        fn put_varint(&mut self, v: i64) -> bool {
            let mut u = v as u64;
            loop {
                let x = (u & 0x7F) as u8;
                u >>= 7;
                if u == 0 {
                    return self.put_byte(x);
                }
                if !self.put_byte(x | 0x80) {
                    return false;
                }
            }
        }

        fn put_body(&mut self, node: NodeId) -> bool {
            let doc = self.doc;
            for part in &doc.nodes[node as usize].parts {
                match *part {
                    Part::Span { start, end } => {
                        let (s, e) = (start as usize, end as usize);
                        if self.pos == s {
                            self.pos = e;
                        } else {
                            let body = self.body;
                            if !self.put(&body[s..e]) {
                                return false;
                            }
                        }
                    }
                    Part::Slot { prefix, obj, .. } => {
                        if let Some(p) = prefix {
                            if !self.put_varint(p as i64) {
                                return false;
                            }
                        }
                        let table = doc.nodes[obj as usize].table;
                        match table {
                            Table::None => {
                                if !self.put_body(obj) {
                                    return false;
                                }
                            }
                            Table::Flag => {
                                if !self.put_byte(1) || !self.put_body(obj) {
                                    return false;
                                }
                            }
                            Table::A | Table::B => {
                                let id = self.ids[obj as usize];
                                if id != u32::MAX {
                                    if !self.put_varint(id as i64 + 2) {
                                        return false;
                                    }
                                    continue;
                                }
                                let t = table_index(table);
                                self.ids[obj as usize] = self.count[t];
                                self.count[t] += 1;
                                if !self.put_byte(1) || !self.put_body(obj) {
                                    return false;
                                }
                            }
                        }
                    }
                }
            }
            true
        }
    }

    let mut w = W { doc, body, pos: 0, ids: vec![u32::MAX; doc.nodes.len()], count: [0, 0] };
    w.put_body(doc.top) && w.pos == body.len()
}
