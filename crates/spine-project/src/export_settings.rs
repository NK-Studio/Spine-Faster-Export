//! 프로젝트에 저장된 내보내기 설정(Spine Export 대화상자에서 마지막으로 쓴 값).
//!
//! - `Project.36`: 종류 이름(`"json"`, `"binary"`, `"png"` …) → 설정 객체 맵.
//! - `Project.13`: 마지막으로 내보낸 종류 이름.
//!
//! 결과는 Spine CLI `--last-export-settings` 가 쓰는 JSON 과 같은 모양이다(`"class": "export-json" | "export-binary"`).
//! 태그 번호는 Spine 4.3.26 CLI 의 `--last-export-settings` 출력과 대조해 확정했다(값 하나씩 바꾼 표본으로).
//! 정수(패킹 크기·여백 등)는 zigzag varint, 열거형은 «순번 + 1» 이다.

use serde_json::{Map, Value as Json};

use crate::document::{Part, Value, KEY_V};
use crate::view::{Leaf, View, GK};
use crate::document::NodeId;

const TEXTURE_FILTER: [&str; 7] = ["Nearest", "Linear", "MipMap", "MipMapNearestNearest", "MipMapLinearNearest", "MipMapNearestLinear", "MipMapLinearLinear"];
const TEXTURE_WRAP: [&str; 3] = ["MirroredRepeat", "ClampToEdge", "Repeat"];
const PIXEL_FORMAT: [&str; 7] = ["Alpha", "Intensity", "LuminanceAlpha", "RGB565", "RGBA4444", "RGB888", "RGBA8888"];
const PACKING: [&str; 3] = ["grid", "rectangles", "polygons"];
const RESAMPLING: [&str; 5] = ["auto", "nearest", "bilinear", "bilinearProgressive", "bicubic"];

/// 마지막 데이터 내보내기(JSON·바이너리) 설정. 마지막 종류가 이미지·영상이면 저장된 JSON, 없으면 바이너리 설정을 쓴다.
pub fn last(view: &View) -> Option<Json> {
    let root = view.root()?;
    let map = view.get_node(Some(root), 36, GK::Node)?;
    let items = &view.doc.node(map).items;
    let mut entries: Vec<(String, NodeId)> = Vec::new();
    for pair in items.chunks_exact(2) {
        let (Value::Node(k), Value::Node(v)) = (&pair[0], &pair[1]) else { continue };
        if let Ok(Leaf::Str(name)) = view.leaf(Some(*k)) {
            entries.push((name.to_string(), *v));
        }
    }
    let data = |name: &str| {
        entries
            .iter()
            .find(|(k, v)| k.eq_ignore_ascii_case(name) && matches!(view.class_name(*v), "JsonExport" | "BinaryExport"))
            .map(|&(_, v)| v)
    };
    let last = view.get_string(Some(root), 13).unwrap_or("");
    let node = data(last).or_else(|| data("json")).or_else(|| data("binary"))?;
    Some(match view.class_name(node) {
        "JsonExport" => json_export(view, node),
        _ => binary_export(view, node),
    })
}

fn json_export(view: &View, n: NodeId) -> Json {
    let f = Fields { view, n };
    let mut m = Map::new();
    m.insert("class".into(), "export-json".into());
    f.put_str(&mut m, "extension", 0);
    f.put_str(&mut m, "format", 1);
    f.put_bool(&mut m, "prettyPrint", 2);
    f.put_bool(&mut m, "nonessential", 3);
    f.put_bool(&mut m, "cleanUp", 4);
    m.insert("packAtlas".into(), pack(view, f.node(5)));
    f.put_str(&mut m, "packSource", 6);
    f.put_str(&mut m, "packTarget", 7);
    f.put_bool(&mut m, "warnings", 8);
    f.put_str(&mut m, "version", 9);
    f.put_bool(&mut m, "all", 10);
    f.put_str(&mut m, "output", 127);
    Json::Object(m)
}

fn binary_export(view: &View, n: NodeId) -> Json {
    let f = Fields { view, n };
    let mut m = Map::new();
    m.insert("class".into(), "export-binary".into());
    f.put_str(&mut m, "extension", 0);
    f.put_bool(&mut m, "nonessential", 1);
    f.put_bool(&mut m, "cleanUp", 2);
    m.insert("packAtlas".into(), pack(view, f.node(3)));
    f.put_str(&mut m, "packSource", 4);
    f.put_str(&mut m, "packTarget", 5);
    f.put_bool(&mut m, "warnings", 6);
    f.put_str(&mut m, "output", 127);
    Json::Object(m)
}

/// Texture Packer Settings(`pack.json` 과 같은 키). 없으면(팩 끔) null.
fn pack(view: &View, n: Option<NodeId>) -> Json {
    let Some(n) = n.filter(|&n| view.class_name(n) == "PackSettings") else { return Json::Null };
    let f = Fields { view, n };
    let mut m = Map::new();
    f.put_bool(&mut m, "stripWhitespaceX", 1);
    f.put_bool(&mut m, "stripWhitespaceY", 2);
    f.put_bool(&mut m, "rotation", 3);
    f.put_bool(&mut m, "alias", 4);
    f.put_bool(&mut m, "ignoreBlankImages", 5);
    f.put_int(&mut m, "alphaThreshold", 6);
    f.put_int(&mut m, "minWidth", 7);
    f.put_int(&mut m, "minHeight", 8);
    f.put_int(&mut m, "maxWidth", 9);
    f.put_int(&mut m, "maxHeight", 10);
    f.put_bool(&mut m, "pot", 11);
    f.put_bool(&mut m, "multipleOfFour", 12);
    f.put_bool(&mut m, "square", 13);
    f.put_str(&mut m, "outputFormat", 14);
    if let Some(Value::Float(q)) = view.get(Some(n), 15, GK::Num) {
        m.insert("jpegQuality".into(), Json::from(f32_json(*q)));
    }
    f.put_bool(&mut m, "premultiplyAlpha", 17);
    f.put_bool(&mut m, "bleed", 18);
    if let Some(Ok(Leaf::Doubles(s))) = f.node(19).map(|a| view.leaf(Some(a))) {
        m.insert("scale".into(), s.into_iter().map(|x| Json::from(f32_json(x as f32))).collect());
    }
    if let Some(s) = f.node(20).and_then(|a| strings(view, a)) {
        m.insert("scaleSuffix".into(), s.into_iter().map(Json::from).collect());
    }
    if let Some(a) = f.node(21) {
        let names: Vec<Json> = enums(view, a).into_iter().filter_map(|v| name_of(&RESAMPLING, v)).map(Json::from).collect();
        m.insert("scaleResampling".into(), names.into());
    }
    f.put_int(&mut m, "paddingX", 22);
    f.put_int(&mut m, "paddingY", 23);
    f.put_bool(&mut m, "edgePadding", 24);
    f.put_bool(&mut m, "duplicatePadding", 25);
    f.put_enum(&mut m, "filterMin", 26, &TEXTURE_FILTER);
    f.put_enum(&mut m, "filterMag", 27, &TEXTURE_FILTER);
    f.put_enum(&mut m, "wrapX", 28, &TEXTURE_WRAP);
    f.put_enum(&mut m, "wrapY", 29, &TEXTURE_WRAP);
    f.put_enum(&mut m, "format", 30, &PIXEL_FORMAT);
    f.put_str(&mut m, "atlasExtension", 31);
    f.put_bool(&mut m, "combineSubdirectories", 32);
    f.put_bool(&mut m, "flattenPaths", 33);
    f.put_bool(&mut m, "useIndexes", 34);
    f.put_bool(&mut m, "debug", 35);
    f.put_bool(&mut m, "fast", 36);
    f.put_bool(&mut m, "limitMemory", 37);
    f.put_bool(&mut m, "currentProject", 39);
    f.put_enum(&mut m, "packing", 40, &PACKING);
    f.put_bool(&mut m, "prettyPrint", 41);
    f.put_bool(&mut m, "legacyOutput", 42);
    Json::Object(m)
}

/// 태그 번호로 값을 꺼내 JSON 키에 넣는다. 없는 필드는 키를 넣지 않는다(읽는 쪽이 기본값을 쓴다).
struct Fields<'v, 'a> {
    view: &'v View<'a>,
    n: NodeId,
}

impl Fields<'_, '_> {
    fn int(&self, tag: i32) -> Option<i64> {
        match self.view.get(Some(self.n), tag, GK::Num) {
            Some(Value::Int(v)) => Some(*v),
            _ => None,
        }
    }

    fn node(&self, tag: i32) -> Option<NodeId> {
        self.view.get_node(Some(self.n), tag, GK::Node)
    }

    fn put_bool(&self, m: &mut Map<String, Json>, key: &str, tag: i32) {
        if let Some(v) = self.int(tag) {
            m.insert(key.into(), Json::Bool(v != 0));
        }
    }

    /// zigzag varint(Kryo `writeInt(v, false)`).
    fn put_int(&self, m: &mut Map<String, Json>, key: &str, tag: i32) {
        if let Some(v) = self.int(tag) {
            m.insert(key.into(), Json::from((v as u64 >> 1) as i64 ^ -(v & 1)));
        }
    }

    /// 문자열. 필드가 있는데 null 이면 null.
    fn put_str(&self, m: &mut Map<String, Json>, key: &str, tag: i32) {
        let tags = &self.view.doc.keys.tags;
        let field = self.view.doc.node(self.n).fields.iter().find(|f| tags[f.key as usize] == Some(tag));
        match field.map(|f| &f.value) {
            Some(Value::Str(s)) => {
                m.insert(key.into(), Json::from(&**s));
            }
            Some(Value::Null) => {
                m.insert(key.into(), Json::Null);
            }
            _ => {}
        }
    }

    fn put_enum(&self, m: &mut Map<String, Json>, key: &str, tag: i32, names: &[&'static str]) {
        if let Some(name) = self.node(tag).and_then(|e| enum_value(self.view, e)).and_then(|v| name_of(names, v)) {
            m.insert(key.into(), Json::from(name));
        }
    }
}

/// f32 를 사람이 적은 그대로의 10진수로(0.9 → 0.9, 0.8999999761… 이 아니라).
fn f32_json(x: f32) -> f64 {
    x.to_string().parse().unwrap_or(x as f64)
}

fn name_of(names: &[&'static str], v: i64) -> Option<&'static str> {
    usize::try_from(v - 1).ok().and_then(|i| names.get(i).copied())
}

fn enum_value(view: &View, n: NodeId) -> Option<i64> {
    match view.doc.node(n).field(KEY_V).map(|f| &f.value) {
        Some(Value::Int(v)) => Some(*v),
        _ => None,
    }
}

/// 열거형 배열(EnumArr): 원소는 슬롯 객체로 기록돼 있다.
fn enums(view: &View, n: NodeId) -> Vec<i64> {
    view.doc
        .node(n)
        .parts
        .iter()
        .filter_map(|p| match *p {
            Part::Slot { obj, .. } => enum_value(view, obj),
            Part::Span { .. } => None,
        })
        .collect()
}

/// 문자열 배열(StrArrK): varint(개수 + 1) 뒤에 Kryo 문자열들. 해석기는 값을 남기지 않으므로 바이트에서 읽는다.
fn strings(view: &View, n: NodeId) -> Option<Vec<String>> {
    let mut r = Bytes { b: view.body, p: view.body_start(n) };
    let count = r.varint()?.checked_sub(1)?;
    let mut out = Vec::new();
    for _ in 0..count.min(1024) {
        out.push(r.kstring()?.unwrap_or_default());
    }
    Some(out)
}

struct Bytes<'a> {
    b: &'a [u8],
    p: usize,
}

impl Bytes<'_> {
    fn byte(&mut self) -> Option<u8> {
        let x = *self.b.get(self.p)?;
        self.p += 1;
        Some(x)
    }

    fn varint(&mut self) -> Option<i64> {
        let mut value = 0i64;
        let mut shift = 0u32;
        loop {
            let x = self.byte()?;
            if shift < 63 {
                value |= ((x & 0x7F) as i64) << shift;
            }
            shift += 7;
            if x & 0x80 == 0 {
                return Some(value);
            }
        }
    }

    /// Kryo 문자열(document.rs `kstring` 과 같은 규칙). 길이 0 이면 None 안의 None(null).
    fn kstring(&mut self) -> Option<Option<String>> {
        let b0 = self.byte()?;
        if b0 & 0x80 == 0 {
            // ASCII: 마지막 글자에 0x80
            let mut s = String::new();
            s.push(b0 as char);
            loop {
                let x = self.byte()?;
                s.push((x & 0x7F) as char);
                if x & 0x80 != 0 {
                    return Some(Some(s));
                }
            }
        }
        let mut n = (b0 & 0x3F) as i64;
        if b0 & 0x40 != 0 {
            let mut sh = 6;
            loop {
                let x = self.byte()?;
                n |= ((x & 0x7F) as i64) << sh;
                sh += 7;
                if x & 0x80 == 0 || sh > 34 {
                    break;
                }
            }
        }
        if n == 0 {
            return Some(None);
        }
        let mut chars: Vec<u16> = Vec::new();
        for _ in 0..(n - 1) {
            let c = self.byte()? as u32;
            let c = if c >= 0xE0 {
                let b1 = self.byte()? as u32;
                let b2 = self.byte()? as u32;
                ((c & 0x0F) << 12) | ((b1 & 0x3F) << 6) | (b2 & 0x3F)
            } else if c >= 0xC0 {
                ((c & 0x1F) << 6) | (self.byte()? as u32 & 0x3F)
            } else {
                c
            };
            chars.push(c as u16);
        }
        Some(Some(String::from_utf16_lossy(&chars)))
    }
}
