//! 객체 그래프를 읽기 쉽게 감싼다(C# `ProjectView`, 파이썬 PoC project_reader.py).
//! 참조는 읽을 때 이미 풀려 있다. 버전마다 다르게 적히는 필드 키(12, t12, d12, 1:12…)를
//! 태그 번호 하나로 찾고, 해석기가 건너뛰는 값(배열·색·열거형)을 바이트에서 꺼낸다.

use std::sync::Arc;

use spine_model::{SpineError, SpineResult};

use crate::document::{Document, NodeId, Value, KEY_S, KEY_V};
use crate::schema::{Cls, Kind};

/// 그래프가 예상과 다를 때(C# NullReference·KeyNotFound·InvalidCast·IndexOutOfRange …)의 오류.
pub fn broken(detail: impl std::fmt::Display) -> SpineError {
    SpineError::Format(format!("프로젝트 구조를 해석하지 못했습니다: {detail}"))
}

/// Get 의 값 종류: 문자열·숫자·객체 또는 특정 클래스 객체.
#[derive(Clone, Copy)]
pub enum GK {
    Str,
    Num,
    Node,
    Cls(Option<Cls>),
}

/// 잎 객체의 값(C# Leaf 결과: long, string, int[], List&lt;double&gt;, List&lt;long&gt;, List&lt;double?&gt;, null).
#[derive(Debug)]
pub enum Leaf {
    Null,
    Long(i64),
    #[allow(dead_code)]
    Str(Arc<str>),
    Color([i32; 4]),
    Doubles(Vec<f64>),
    Longs(Vec<i64>),
    Boxed(Vec<Option<f64>>),
    /// 그 밖의 값(float 등 — 쓰는 곳이 없다).
    Other,
}

pub struct View<'a> {
    pub doc: &'a Document,
    pub body: &'a [u8],
    pub format: &'a str,
}

impl<'a> View<'a> {
    pub fn keys38(&self) -> bool {
        self.doc.schema.keys38
    }

    pub fn curve_conditional(&self) -> bool {
        self.doc.schema.curve_condition.is_some()
    }

    pub fn root(&self) -> Option<NodeId> {
        self.doc.of("Project").first().copied().or(self.doc.partial_root)
    }

    pub fn cls(&self, name: &str) -> Option<Cls> {
        self.doc.schema.find_class(name)
    }

    pub fn class_name(&self, n: NodeId) -> &'a str {
        self.doc.schema.class_name(self.doc.node(n).cls)
    }

    /// 태그 번호로 값(같은 태그가 여럿이면 kind 로 고른다).
    pub fn get(&self, node: Option<NodeId>, tag: i32, kind: GK) -> Option<&'a Value> {
        let node = self.doc.node(node?);
        let tags = &self.doc.keys.tags;
        for f in &node.fields {
            if tags[f.key as usize] != Some(tag) {
                continue;
            }
            let ok = match (&f.value, kind) {
                (Value::Str(_), GK::Str) => true,
                (Value::Int(_) | Value::Float(_), GK::Num) => true,
                (Value::Node(_), GK::Node) => true,
                (Value::Node(n), GK::Cls(Some(c))) => self.doc.node(*n).cls == c,
                _ => false,
            };
            if ok {
                return Some(&f.value);
            }
        }
        None
    }

    pub fn get_node(&self, node: Option<NodeId>, tag: i32, kind: GK) -> Option<NodeId> {
        match self.get(node, tag, kind) {
            Some(Value::Node(n)) => Some(*n),
            _ => None,
        }
    }

    pub fn get_string(&self, node: Option<NodeId>, tag: i32) -> Option<&'a str> {
        match self.get(node, tag, GK::Str) {
            Some(Value::Str(s)) => Some(s),
            _ => None,
        }
    }

    /// 리스트·맵 노드의 원소. 빈 자리(객체가 아닌 값)는 None 으로 남는다.
    pub fn items(&self, node: Option<NodeId>) -> Vec<Option<NodeId>> {
        match node {
            Some(n) => self
                .doc
                .node(n)
                .items
                .iter()
                .map(|v| match v {
                    Value::Node(id) => Some(*id),
                    _ => None,
                })
                .collect(),
            None => Vec::new(),
        }
    }

    /// 새 객체 본문의 시작 위치(표지 뒤).
    pub fn body_start(&self, n: NodeId) -> usize {
        let node = self.doc.node(n);
        match node.parts.first() {
            Some(p) => p.start() as usize,
            None => node.end as usize,
        }
    }

    fn byte(&self, p: usize) -> SpineResult<u8> {
        self.body.get(p).copied().ok_or_else(|| broken("인덱스가 배열 범위를 벗어났습니다"))
    }

    pub fn float_at(&self, at: usize) -> SpineResult<f64> {
        match self.body.get(at..at.wrapping_add(4)) {
            Some(b) if at.checked_add(4).is_some() => Ok(f32::from_be_bytes([b[0], b[1], b[2], b[3]]) as f64),
            _ => Err(broken("인덱스가 배열 범위를 벗어났습니다")),
        }
    }

    pub fn floats(&self, at: usize, count: i64) -> SpineResult<Vec<f64>> {
        if count < 0 {
            return Err(broken("개수가 음수입니다"));
        }
        let count = count as usize;
        let end = count.checked_mul(4).and_then(|n| n.checked_add(at));
        match end {
            Some(e) if e <= self.body.len() => {}
            _ => return Err(broken("인덱스가 배열 범위를 벗어났습니다")),
        }
        let mut v = Vec::with_capacity(count);
        for i in 0..count {
            let p = at + 4 * i;
            let b = &self.body[p..p + 4];
            v.push(f32::from_be_bytes([b[0], b[1], b[2], b[3]]) as f64);
        }
        Ok(v)
    }

    fn varint(&self, p: &mut usize) -> SpineResult<i64> {
        let mut value: i64 = 0;
        let mut shift: u32 = 0;
        loop {
            let x = self.byte(*p)?;
            *p += 1;
            if shift < 63 {
                value |= ((x & 0x7F) as i64) << shift;
            }
            shift = shift.saturating_add(7);
            if x & 0x80 == 0 {
                return Ok(value);
            }
        }
    }

    /// 배열·색·Vec3·열거형·문자열 객체의 값.
    pub fn leaf(&self, node: Option<NodeId>) -> SpineResult<Leaf> {
        let Some(n) = node else { return Ok(Leaf::Null) };
        let nd = self.doc.node(n);
        let info = self.doc.schema.class(nd.cls);
        let Some(kind) = info.kind else {
            return Err(SpineError::Format(format!("{}()는 잎 값이 아닙니다", info.name)));
        };
        let value_leaf = |key: u32| -> SpineResult<Leaf> {
            match nd.field(key) {
                None => Err(broken("주어진 키가 사전에 없습니다")),
                Some(f) => Ok(match &f.value {
                    Value::Null => Leaf::Null,
                    Value::Int(v) => Leaf::Long(*v),
                    Value::Str(s) => Leaf::Str(s.clone()),
                    _ => Leaf::Other,
                }),
            }
        };
        match kind {
            Kind::Varint => return value_leaf(KEY_V),
            Kind::Str => return value_leaf(KEY_S),
            _ => {}
        }
        let mut p = self.body_start(n);
        match kind {
            Kind::Fixed(4) => Ok(Leaf::Color([
                self.byte(p)? as i32,
                self.byte(p + 1)? as i32,
                self.byte(p + 2)? as i32,
                self.byte(p + 3)? as i32,
            ])),
            Kind::Fixed(12) => Ok(Leaf::Doubles(self.floats(p, 3)?)),
            Kind::RfArray => {
                let n = self.varint(&mut p)?;
                Ok(Leaf::Doubles(self.floats(p, n as i32 as i64)?))
            }
            Kind::FArray => {
                let n = self.varint(&mut p)?;
                if n == 0 {
                    Ok(Leaf::Null)
                } else {
                    Ok(Leaf::Doubles(self.floats(p, (n as i32 as i64) - 1)?))
                }
            }
            Kind::RsArray => {
                let n = self.varint(&mut p)?;
                if n < 0 || (n as u64).saturating_mul(2) > self.body.len() as u64 {
                    return Err(broken("인덱스가 배열 범위를 벗어났습니다"));
                }
                let mut v = Vec::with_capacity(n as usize);
                for _ in 0..n {
                    v.push(((self.byte(p)? as i64) << 8) | self.byte(p + 1)? as i64);
                    p += 2;
                }
                Ok(Leaf::Longs(v))
            }
            Kind::RvArray => {
                let n = self.varint(&mut p)?;
                let mut v = Vec::new();
                let mut i = 0;
                while i < n {
                    v.push(self.varint(&mut p)?);
                    i += 1;
                }
                Ok(Leaf::Longs(v))
            }
            Kind::BoxFArray => {
                let n = self.varint(&mut p)?;
                let mut v = Vec::new();
                let mut i = 0;
                while i < n - 1 {
                    let present = self.varint(&mut p)?;
                    if present != 0 {
                        v.push(Some(self.float_at(p)?));
                        p += 4;
                    } else {
                        v.push(None);
                    }
                    i += 1;
                }
                Ok(Leaf::Boxed(v))
            }
            Kind::DefVtx | Kind::DefVtx0 => {
                let total = self.varint(&mut p)? as i32;
                let chunks = self.varint(&mut p)? as i32;
                if total < 0 || total as usize > self.body.len().saturating_mul(128) {
                    return Err(broken("인덱스가 배열 범위를 벗어났습니다"));
                }
                let mut out = vec![0.0f64; total as usize];
                for i in 0..chunks.max(0) {
                    let head = self.byte(p)?;
                    p += 1;
                    if head != 0 || kind == Kind::DefVtx {
                        let n = 128.min(total - 128 * i);
                        for j in 0..n.max(0) {
                            let idx = (128 * i + j) as usize;
                            let v = self.float_at(p)?;
                            *out.get_mut(idx).ok_or_else(|| broken("인덱스가 범위를 벗어났습니다"))? = v;
                            p += 4;
                        }
                    }
                }
                Ok(Leaf::Doubles(out))
            }
            _ => Err(SpineError::Format(format!("{}({})는 잎 값이 아닙니다", info.name, info.kind_name))),
        }
    }
}
