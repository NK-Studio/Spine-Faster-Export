//! 포맷 규칙 한 벌(C# `SpineProjectSchema`). 클래스 번호 → 이름·종류, (클래스, 키) → 값 타입,
//! 참조 표, 4.3 타임라인 배치와 빌드별 옵션. 스키마 JSON 은 크레이트에 내장한다.

use std::collections::HashMap;
use std::sync::OnceLock;

use serde_json::Value as Json;

/// 스키마 안 클래스 번호(이름을 한 번만 저장한다).
pub type Cls = u16;

/// 객체 참조 표.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Table {
    A,
    B,
    /// 참조 추적 없음(표지도 없음).
    None,
    /// 참조 추적 없이 «null 아님» 표지(1)만.
    Flag,
}

/// 클래스 본문 종류.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Tagged,
    Timeline,
    Keyframe,
    Plain,
    List,
    Map,
    Str,
    Varint,
    Fixed(i64),
    RfArray,
    RvArray,
    RsArray,
    FArray,
    KStrArray,
    EnumArray,
    BoxFArray,
    DefVtx,
    DefVtx0,
    Unknown,
}

/// 필드 값 타입.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Ty {
    V,
    VB,
    F,
    S,
    O,
    T,
    N,
    LO,
    /// 모르는 타입(이름은 `Schema::ty_names`).
    Unknown(u16),
}

#[derive(Clone, Debug)]
pub struct TimelineLayout {
    pub vals: i64,
    pub curves: i64,
    pub extra: Option<Ty>,
    pub extra2: Option<Ty>,
    pub payload: Option<Ty>,
}

#[derive(Debug, Default)]
pub struct ClassInfo {
    pub name: String,
    pub kind: Option<Kind>,
    pub kind_name: String,
    pub table: Option<Table>,
    pub fields: Option<HashMap<String, Ty>>,
    pub slots: HashMap<String, Cls>,
    pub tail: i64,
    pub plain: Option<Vec<Ty>>,
    pub groups: Option<Vec<Vec<i32>>>,
}

#[derive(Debug)]
pub struct Schema {
    pub name: String,
    pub formats: Vec<String>,
    pub classes: Vec<ClassInfo>,
    index: HashMap<String, Cls>,
    pub names: HashMap<i64, Cls>,
    pub timelines: HashMap<i64, TimelineLayout>,
    /// «필드 수:첫 태그» → 클래스 (JSON 순서).
    pub any_a: Vec<(String, Cls)>,
    pub ascending: bool,
    pub timeline_refs: bool,
    /// None: 곡선 항상 · "own" · "prev".
    pub curve_condition: Option<String>,
    pub key_event_field: String,
    pub root_varint: bool,
    pub root_pad: i64,
    pub ty_names: Vec<String>,
    /// 3.8 프로젝트(Key38 클래스가 있다).
    pub keys38: bool,
    pub has_timeline_kind: bool,
}

/// 자주 쓰는 클래스(모든 스키마에서 같은 번호로 먼저 등록한다).
pub const STAR_A: Cls = 0;
pub const STRING: Cls = 1;
pub const PROJECT: Cls = 2;
pub const EDITOR_TAIL: Cls = 3;
pub const TOP: Cls = 4;
pub const VIDEO_ENUM_A: Cls = 5;
const FIXED_CLASSES: [&str; 6] = ["*A", "String", "Project", "EditorTail", "<top>", "VideoEnumA"];

impl Schema {
    pub fn class(&self, cls: Cls) -> &ClassInfo {
        &self.classes[cls as usize]
    }

    pub fn class_name(&self, cls: Cls) -> &str {
        &self.classes[cls as usize].name
    }

    pub fn find_class(&self, name: &str) -> Option<Cls> {
        self.index.get(name).copied()
    }

    pub fn table_of(&self, cls: Cls) -> Table {
        if cls == STAR_A {
            return Table::A;
        }
        self.classes[cls as usize].table.unwrap_or(Table::B)
    }

    pub fn ty_name(&self, ty: Ty) -> &str {
        match ty {
            Ty::V => "V",
            Ty::VB => "VB",
            Ty::F => "F",
            Ty::S => "S",
            Ty::O => "O",
            Ty::T => "T",
            Ty::N => "N",
            Ty::LO => "LO",
            Ty::Unknown(i) => &self.ty_names[i as usize],
        }
    }

    pub fn parse(name: &str, json: &str) -> Result<Schema, String> {
        let o: Json = serde_json::from_str(json).map_err(|e| format!("스키마 {name}: {e}"))?;
        let mut s = Schema {
            name: name.to_string(),
            formats: Vec::new(),
            classes: Vec::new(),
            index: HashMap::new(),
            names: HashMap::new(),
            timelines: HashMap::new(),
            any_a: Vec::new(),
            ascending: false,
            timeline_refs: true,
            curve_condition: None,
            key_event_field: "7".to_string(),
            root_varint: true,
            root_pad: 0,
            ty_names: Vec::new(),
            keys38: false,
            has_timeline_kind: false,
        };
        for c in FIXED_CLASSES {
            s.intern(c);
        }

        s.formats = match o.get("formats") {
            Some(Json::Array(a)) => a.iter().filter_map(|t| t.as_str().map(str::to_string)).collect(),
            _ => vec![name.to_string()],
        };

        if let Some(Json::Object(names)) = o.get("names") {
            for (k, v) in names {
                let Some(v) = v.as_str() else { continue };
                let cls = s.intern(v);
                if let Some(n) = canonical_number(k) {
                    s.names.insert(n, cls);
                }
            }
        }

        if let Some(Json::Object(kinds)) = o.get("kinds") {
            for (k, v) in kinds {
                let Some(v) = v.as_str() else { continue };
                let cls = s.intern(k);
                let kind = parse_kind(v);
                if v == "timeline" {
                    s.has_timeline_kind = true;
                }
                if k == "Key38" {
                    s.keys38 = true;
                }
                let info = &mut s.classes[cls as usize];
                info.kind = Some(kind);
                info.kind_name = v.to_string();
            }
        }

        if let Some(Json::Object(fields)) = o.get("fields") {
            for (k, v) in fields {
                let cls = s.intern(k);
                let mut map = HashMap::new();
                if let Json::Object(m) = v {
                    for (fk, fv) in m {
                        if let Some(t) = fv.as_str() {
                            let ty = s.parse_ty(t);
                            map.insert(fk.clone(), ty);
                        }
                    }
                }
                s.classes[cls as usize].fields = Some(map);
            }
        }

        if let Some(Json::Object(slots)) = o.get("slots") {
            for (k, v) in slots {
                let cls = s.intern(k);
                if let Json::Object(m) = v {
                    for (fk, fv) in m {
                        if let Some(t) = fv.as_str() {
                            let target = s.intern(t);
                            s.classes[cls as usize].slots.insert(fk.clone(), target);
                        }
                    }
                }
            }
        }

        if let Some(Json::Object(tail)) = o.get("tail") {
            for (k, v) in tail {
                let cls = s.intern(k);
                s.classes[cls as usize].tail = v.as_i64().unwrap_or(0);
            }
        }

        if let Some(Json::Object(tables)) = o.get("tables") {
            for (k, v) in tables {
                let cls = s.intern(k);
                let t = match v.as_str() {
                    Some("A") => Table::A,
                    Some("none") => Table::None,
                    Some("flag") => Table::Flag,
                    _ => Table::B,
                };
                s.classes[cls as usize].table = Some(t);
            }
        }

        if let Some(Json::Object(plain)) = o.get("plain") {
            for (k, v) in plain {
                let cls = s.intern(k);
                let mut types = Vec::new();
                if let Json::Array(a) = v {
                    for t in a {
                        let t = t.as_str().unwrap_or("");
                        let ty = s.parse_ty(t);
                        types.push(ty);
                    }
                }
                s.classes[cls as usize].plain = Some(types);
            }
        }

        if let Some(Json::Object(timelines)) = o.get("timelines") {
            for (k, v) in timelines {
                let Some(n) = canonical_number(k) else { continue };
                let get_ty = |s: &mut Schema, key: &str| v.get(key).and_then(|t| t.as_str()).map(|t| s.parse_ty(t));
                let extra = get_ty(&mut s, "extra");
                let extra2 = get_ty(&mut s, "extra2");
                let payload = get_ty(&mut s, "payload");
                s.timelines.insert(
                    n,
                    TimelineLayout {
                        vals: v.get("vals").and_then(|x| x.as_i64()).unwrap_or(0),
                        curves: v.get("curves").and_then(|x| x.as_i64()).unwrap_or(0),
                        extra,
                        extra2,
                        payload,
                    },
                );
            }
        }

        if let Some(Json::Object(any)) = o.get("anyA") {
            for (k, v) in any {
                if let Some(v) = v.as_str() {
                    let cls = s.intern(v);
                    s.any_a.push((k.clone(), cls));
                }
            }
        }

        if let Some(Json::Object(groups)) = o.get("groups") {
            for (k, v) in groups {
                let cls = s.intern(k);
                let mut list = Vec::new();
                if let Json::Array(gs) = v {
                    for g in gs {
                        let mut one = Vec::new();
                        if let Json::Array(tags) = g {
                            for t in tags {
                                one.push(t.as_i64().unwrap_or(0) as i32);
                            }
                        }
                        list.push(one);
                    }
                }
                s.classes[cls as usize].groups = Some(list);
            }
        }

        s.ascending = o.get("ascending").and_then(|v| v.as_bool()).unwrap_or(false);
        s.timeline_refs = o.get("tl_refs").and_then(|v| v.as_bool()).unwrap_or(true);
        match o.get("curve_cond") {
            Some(Json::String(c)) => s.curve_condition = Some(c.clone()),
            Some(Json::Bool(true)) => s.curve_condition = Some("prev".to_string()),
            _ => {}
        }
        if let Some(Json::String(k)) = o.get("key_event_field") {
            s.key_event_field = k.clone();
        }
        s.root_varint = o.get("root_varint").and_then(|v| v.as_bool()).unwrap_or(true);
        s.root_pad = o.get("root_pad").and_then(|v| v.as_i64()).unwrap_or(0);
        Ok(s)
    }

    fn intern(&mut self, name: &str) -> Cls {
        if let Some(&c) = self.index.get(name) {
            return c;
        }
        let c = self.classes.len() as Cls;
        self.classes.push(ClassInfo { name: name.to_string(), ..Default::default() });
        self.index.insert(name.to_string(), c);
        c
    }

    fn parse_ty(&mut self, t: &str) -> Ty {
        match t {
            "V" => Ty::V,
            "VB" => Ty::VB,
            "F" => Ty::F,
            "S" => Ty::S,
            "O" => Ty::O,
            "T" => Ty::T,
            "N" => Ty::N,
            "LO" => Ty::LO,
            other => {
                if let Some(i) = self.ty_names.iter().position(|n| n == other) {
                    return Ty::Unknown(i as u16);
                }
                self.ty_names.push(other.to_string());
                Ty::Unknown((self.ty_names.len() - 1) as u16)
            }
        }
    }
}

/// C# 는 숫자를 문자열 키("12")로 찾는다. 정규형 숫자 문자열만 같은 키가 된다.
fn canonical_number(k: &str) -> Option<i64> {
    let n: i64 = k.parse().ok()?;
    if n.to_string() == k {
        Some(n)
    } else {
        None
    }
}

fn parse_kind(v: &str) -> Kind {
    match v {
        "tagged" => Kind::Tagged,
        "timeline" => Kind::Timeline,
        "keyframe" => Kind::Keyframe,
        "plain" => Kind::Plain,
        "list" => Kind::List,
        "map" => Kind::Map,
        "string" => Kind::Str,
        "varint" => Kind::Varint,
        "rfarray" => Kind::RfArray,
        "rvarray" => Kind::RvArray,
        "rsarray" => Kind::RsArray,
        "farray" => Kind::FArray,
        "kstrarray" => Kind::KStrArray,
        "enumarray" => Kind::EnumArray,
        "boxfarray" => Kind::BoxFArray,
        "defvtx" => Kind::DefVtx,
        "defvtx0" => Kind::DefVtx0,
        other => match other.strip_prefix("fixed").and_then(|n| n.parse::<i64>().ok()) {
            Some(n) => Kind::Fixed(n),
            None => Kind::Unknown,
        },
    }
}

const EMBEDDED: [(&str, &str); 14] = [
    ("3.8.55", include_str!("../schemas/3.8.55.json")),
    ("3.8.75", include_str!("../schemas/3.8.75.json")),
    ("4.0.07", include_str!("../schemas/4.0.07.json")),
    ("4.0.64", include_str!("../schemas/4.0.64.json")),
    ("4.2.00", include_str!("../schemas/4.2.00.json")),
    ("4.2.43", include_str!("../schemas/4.2.43.json")),
    ("4.2.56b", include_str!("../schemas/4.2.56b.json")),
    ("4.3.00", include_str!("../schemas/4.3.00.json")),
    ("4.3.00r", include_str!("../schemas/4.3.00r.json")),
    ("4.3.03", include_str!("../schemas/4.3.03.json")),
    ("4.3.04", include_str!("../schemas/4.3.04.json")),
    ("4.3.05", include_str!("../schemas/4.3.05.json")),
    ("4.3.26", include_str!("../schemas/4.3.26.json")),
    ("4.3b", include_str!("../schemas/4.3b.json")),
];

/// 내장 스키마 전부(파일 이름 순서 — C# `SchemaLibrary` 와 같다).
pub fn all() -> &'static [Schema] {
    static ALL: OnceLock<Vec<Schema>> = OnceLock::new();
    ALL.get_or_init(|| {
        let mut list: Vec<(&str, &str)> = EMBEDDED.to_vec();
        list.sort_by(|a, b| a.0.cmp(b.0));
        list.into_iter()
            .map(|(name, json)| Schema::parse(name, json).expect("내장 스키마가 손상되었습니다"))
            .collect()
    })
}
