//! 프로젝트 그래프 → 버전 중립 스켈레톤 모델(C# `SkeletonModelReader`, 파이썬 PoC model.py).
//! 필드 태그 번호는 4.0~4.3 프로젝트에서 같다(포맷마다 키 모양만 다르며 [`View`]가 감춘다).

use std::cmp::Ordering;
use std::collections::{HashMap, HashSet};

use spine_model::*;

use crate::document::{NodeId, Value, KEY_8, KEY_9, KEY_FLAG, KEY_KEYS, KEY_P0, KEY_P1, KEY_TIME, KEY_TYPE};
use crate::view::{broken, Leaf, View, GK};

const PROPS: [&str; 6] = ["rotate", "x", "y", "scaleX", "scaleY", "shearY"];
const UNSET: f64 = 2147483648.0; // 쓰지 않은 손잡이(SkeletonJsonWriter 와 같은 값)
const WHITE: [i32; 4] = [255, 255, 255, 255];

type R<T> = SpineResult<T>;

/// 타임라인 종류 → (모델 이름, 값 채널 수)
fn timeline_info(ty: i64) -> Option<(&'static str, i32)> {
    Some(match ty {
        0 => ("rotate", 1),
        1 => ("translate", 2),
        2 => ("scale", 2),
        3 => ("shear", 2),
        4 => ("color", 7),
        5 => ("attachment", 0),
        6 => ("deform", 0),
        7 => ("ik", 5),
        8 => ("transform", 6),
        9 => ("pathPosition", 1),
        10 => ("pathSpacing", 1),
        11 => ("pathMix", 3),
        12 => ("event", 0),
        13 => ("drawOrder", 0),
        14 => ("translatex", 1),
        15 => ("translatey", 1),
        22 => ("sequence", 3),
        27 => ("physicsWind", 1),
        36 => ("physicsWindAll", 1),
        40 => ("sliderMix", 1),
        _ => return None,
    })
}

/// 3.8 타임라인 종류 번호 → 4.x 종류 번호
fn types38(raw: i64) -> Option<i64> {
    Some(match raw {
        26 => 0,
        25 => 1,
        24 => 2,
        68 => 3,
        81 => 4,
        27 => 5,
        51 => 6,
        57 => 7,
        67 => 8,
        74 => 9,
        73 => 10,
        75 => 11,
        36 => 12,
        40 => 13,
        _ => return None,
    })
}

/// 곡선이 붙는 채널 수(나머지 값은 계단)
fn curve_channels(ty: i64) -> i32 {
    match ty {
        0 => 1,
        1 | 2 | 3 => 2,
        4 => 7,
        6 => 1,
        7 => 2,
        8 => 6,
        9 | 10 => 1,
        11 => 3,
        14 | 15 => 1,
        27 | 36 | 40 => 1,
        _ => 0,
    }
}

fn inherits(k: i64) -> Option<&'static str> {
    Some(match k {
        1 => "normal",
        2 => "onlyTranslation",
        3 => "noRotationOrReflection",
        4 => "noScale",
        5 => "noScaleOrReflection",
        _ => return None,
    })
}

fn blends(k: i64) -> Option<&'static str> {
    Some(match k {
        1 => "normal",
        2 => "additive",
        3 => "multiply",
        4 => "screen",
        _ => return None,
    })
}

fn spacing_modes(k: i64) -> Option<&'static str> {
    Some(match k {
        1 => "length",
        2 => "fixed",
        3 => "percent",
        4 => "proportional",
        _ => return None,
    })
}

fn rotate_modes(k: i64) -> Option<&'static str> {
    Some(match k {
        1 => "tangent",
        2 => "chain",
        3 => "chainScale",
        _ => return None,
    })
}

fn position_modes(k: i64) -> Option<&'static str> {
    Some(match k {
        1 => "fixed",
        2 => "percent",
        _ => return None,
    })
}

fn rgba(c: Option<[i32; 4]>) -> Option<String> {
    c.map(|c| format!("{:02x}{:02x}{:02x}{:02x}", c[0], c[1], c[2], c[3]))
}

fn rgb(c: Option<[i32; 4]>) -> Option<String> {
    c.map(|c| format!("{:02x}{:02x}{:02x}", c[0], c[1], c[2]))
}

fn zigzag_dec(v: i64) -> i64 {
    (((v as u64) >> 1) as i64) ^ -(v & 1)
}

fn null_node() -> spine_model::SpineError {
    broken("개체 참조가 개체의 인스턴스로 설정되지 않았습니다")
}

fn unsupported(msg: String) -> spine_model::SpineError {
    SpineError::Unsupported(msg)
}

/// C# string.CompareOrdinal(UTF-16 코드 단위 비교). null 은 가장 앞.
fn cmp_ordinal(a: Option<&str>, b: Option<&str>) -> Ordering {
    match (a, b) {
        (None, None) => Ordering::Equal,
        (None, Some(_)) => Ordering::Less,
        (Some(_), None) => Ordering::Greater,
        (Some(a), Some(b)) => {
            if a.is_ascii() && b.is_ascii() {
                a.cmp(b)
            } else {
                a.encode_utf16().cmp(b.encode_utf16())
            }
        }
    }
}

/// 자주 찾는 클래스 번호(스키마에 없으면 None — 그 클래스 객체는 없다).
struct Classes {
    list: GK,
    folder: GK,
    bone: GK,
    slot: GK,
    skeleton: GK,
    placeholder: GK,
    skin: GK,
    mesh_att: GK,
    animation: GK,
    sequence: GK,
    event_data: GK,
    timeline_type: GK,
}

pub struct Reader<'a> {
    p: &'a View<'a>,
    c: Classes,
    fps: f64,
    full: HashMap<NodeId, Option<String>>, // 폴더 안 슬롯·제약의 전체 이름(폴더/이름)
    bones: Vec<Option<NodeId>>,
    bone_index: HashMap<NodeId, usize>,
    slots: Vec<Option<NodeId>>,
    slot_index: HashMap<NodeId, usize>,
    skin_nodes: Vec<Option<NodeId>>,
    /// 메시마다 그것을 부모로 삼는 linked mesh 수(부모가 linked mesh 면 그 뿌리 메시에 센다).
    linked_children: HashMap<NodeId, usize>,
    /// Export 체크가 꺼진 첨부도 내보낸다(Spine «Export all»).
    export_all: bool,
    /// 가중치 메시의 삼각형을 Spine 4.2 이하 편집기처럼 정렬한다(`sort_triangles`).
    sort_triangles: bool,
}

/// 프로젝트의 스켈레톤 목록(이름, 노드). 없으면 오류.
pub fn skeletons(view: &View) -> R<Vec<(Option<String>, NodeId)>> {
    let list = view.doc.of("Skeleton");
    if list.is_empty() {
        return Err(SpineError::Format("스켈레톤이 없습니다".to_string()));
    }
    Ok(list.into_iter().map(|s| (view.get_string(Some(s), 1).map(str::to_string), s)).collect())
}

impl<'a> Reader<'a> {
    pub fn new(p: &'a View<'a>) -> Reader<'a> {
        let g = |n: &str| GK::Cls(p.cls(n));
        Reader {
            p,
            c: Classes {
                list: g("List"),
                folder: g("Folder"),
                bone: g("Bone"),
                slot: g("Slot"),
                skeleton: g("Skeleton"),
                placeholder: g("Placeholder"),
                skin: g("Skin"),
                mesh_att: g("MeshAtt"),
                animation: g("Animation"),
                sequence: g("Sequence"),
                event_data: g("EventData"),
                timeline_type: g("TimelineType"),
            },
            fps: 30.0,
            full: HashMap::new(),
            bones: Vec::new(),
            bone_index: HashMap::new(),
            slots: Vec::new(),
            slot_index: HashMap::new(),
            skin_nodes: Vec::new(),
            linked_children: linked_children(p, g("MeshAtt")),
            export_all: false,
            sort_triangles: !p.format.starts_with("4.3"),
        }
    }

    /// 내보낼 Spine 버전을 알려 준다. 4.3 이상으로 내보내는 것은 4.3 편집기뿐이고, 4.3 편집기는 옛 프로젝트를 열어도
    /// 가중치 메시의 삼각형을 정렬하지 않는다(4.0 형식 그대로인 공식 4.3 예제의 내보내기와 대조).
    pub fn target_version(mut self, version: &str) -> Self {
        let mut parts = version.split('.').map(|p| p.parse::<u32>().unwrap_or(0));
        let target = (parts.next().unwrap_or(0), parts.next().unwrap_or(0));
        if target >= (4, 3) {
            self.sort_triangles = false;
        }
        self
    }

    /// Export 체크가 꺼진 첨부도 내보낼지(Spine «Export all»).
    pub fn export_all(mut self, on: bool) -> Self {
        self.export_all = on;
        self
    }

    // ------------------------------------------------------------ 작은 도우미

    fn cls(&self, n: NodeId) -> &'a str {
        self.p.class_name(n)
    }

    fn name(&self, n: Option<NodeId>) -> Option<String> {
        let n = n?;
        if let Some(Some(full)) = self.full.get(&n) {
            return Some(full.clone());
        }
        self.p.get_string(Some(n), 1).map(str::to_string)
    }

    fn name_or_empty(&self, n: Option<NodeId>) -> String {
        self.name(n).unwrap_or_default()
    }

    fn node_of(&self, n: Option<NodeId>, tag: i32, kind: GK) -> Option<NodeId> {
        self.p.get_node(n, tag, kind)
    }

    /// 폴더 안 항목의 이름: 폴더 경로(Folder 1 이름, 3 부모)/이름. 뿌리 폴더는 이름이 없다.
    fn full_name(&self, n: Option<NodeId>, folder_tag: i32) -> R<Option<String>> {
        let mut name = self.p.get_string(n, 1).map(str::to_string);
        let mut folder = self.p.get_node(n, folder_tag, self.c.folder);
        let mut guard = 0;
        while let Some(f) = folder {
            if let Some(part) = self.p.get_string(Some(f), 1) {
                if !part.is_empty() {
                    name = Some(format!("{}/{}", part, name.as_deref().unwrap_or("")));
                }
            }
            folder = self.p.get_node(Some(f), 3, self.c.folder);
            guard += 1;
            if guard > 100_000 {
                return Err(broken("폴더가 순환합니다"));
            }
        }
        Ok(name)
    }

    fn skin_name(&self, s: Option<NodeId>) -> R<Option<String>> {
        self.full_name(s, 5)
    }

    fn lst(&self, n: Option<NodeId>, tag: i32) -> Vec<Option<NodeId>> {
        self.p.items(self.p.get_node(n, tag, self.c.list))
    }

    fn f(&self, n: Option<NodeId>, tag: i32, fallback: f64) -> f64 {
        match self.p.get(n, tag, GK::Num) {
            Some(Value::Float(f)) => *f as f64,
            Some(Value::Int(i)) => *i as f64,
            _ => fallback,
        }
    }

    fn i(&self, n: Option<NodeId>, tag: i32, fallback: i64) -> i64 {
        match self.p.get(n, tag, GK::Num) {
            Some(Value::Float(f)) => *f as i64,
            Some(Value::Int(i)) => *i,
            _ => fallback,
        }
    }

    fn leaf_of(&self, n: Option<NodeId>, tag: i32) -> R<Leaf> {
        match self.p.get_node(n, tag, GK::Node) {
            Some(v) => self.p.leaf(Some(v)),
            None => Ok(Leaf::Null),
        }
    }

    fn leaf_long(&self, n: Option<NodeId>, tag: i32, fallback: i64) -> R<i64> {
        Ok(match self.leaf_of(n, tag)? {
            Leaf::Long(v) => v,
            _ => fallback,
        })
    }

    fn leaf_color(&self, n: Option<NodeId>, tag: i32) -> R<Option<[i32; 4]>> {
        Ok(match self.leaf_of(n, tag)? {
            Leaf::Color(c) => Some(c),
            _ => None,
        })
    }

    fn leaf_floats(&self, n: Option<NodeId>, tag: i32) -> R<Option<Vec<f64>>> {
        Ok(as_floats(self.leaf_of(n, tag)?))
    }

    fn leaf_ints(&self, n: Option<NodeId>, tag: i32) -> R<Option<Vec<i64>>> {
        Ok(match self.leaf_of(n, tag)? {
            Leaf::Longs(v) => Some(v),
            Leaf::Doubles(v) => Some(v.into_iter().map(|x| x as i64).collect()),
            _ => None,
        })
    }

    // ------------------------------------------------------------ 전체

    pub fn read(mut self, sk: NodeId) -> R<SkeletonModel> {
        let p = self.p;
        let sk = Some(sk);
        let proj = p.root();
        self.fps = self.f(proj, 2, 30.0);
        if self.fps == 0.0 {
            self.fps = 30.0;
        }

        let mut m = SkeletonModel {
            name: p.get_string(sk, 1).map(str::to_string),
            format: Some(p.format.to_string()),
            fps: self.fps,
            images: Some(p.get_string(sk, 3).unwrap_or("").to_string()),
            audio: Some(p.get_string(sk, 34).unwrap_or("").to_string()),
            reference_scale: self.f(sk, 46, 100.0),
            bounds: self.leaf_floats(sk, 24)?,
            ..Default::default()
        };

        self.bones = self.lst(sk, 4);
        self.bone_index = index_of(&self.bones);
        self.slots = self.lst(sk, 13);
        self.slot_index = index_of(&self.slots);
        for i in 0..self.slots.len() {
            let s0 = self.slots[i];
            let full = self.full_name(s0, 16)?;
            let s0 = s0.ok_or_else(|| broken("값은 null 일 수 없습니다"))?;
            self.full.insert(s0, full);
        }

        let mut cs = self.lst(sk, 16);
        cs.extend(self.lst(sk, 22));
        cs.extend(self.lst(sk, 23));
        cs.extend(self.constraint_order(sk)?);
        for c0 in cs.into_iter().flatten() {
            let full = self.full_name(Some(c0), 32)?;
            self.full.insert(c0, full);
        }

        let mut bones = Vec::with_capacity(self.bones.len());
        for &b in &self.bones {
            bones.push(self.bone(b)?);
        }
        m.bones = bones;
        let mut slots = Vec::with_capacity(self.slots.len());
        for &s in &self.slots {
            slots.push(self.slot(s)?);
        }
        m.slots = slots;
        m.constraints = self.constraints(sk)?;
        self.skin_nodes = self.lst(sk, 8);
        let mut in_skin = HashSet::new();
        for &s in &self.skin_nodes {
            for b in self.lst(s, 6).into_iter().flatten() {
                in_skin.insert(b);
            }
        }
        for (i, b) in self.bones.iter().enumerate() {
            m.bones[i].skin = b.map_or(false, |b| in_skin.contains(&b));
        }

        m.skins = self.skins(sk)?;
        m.events = self.lst(sk, 10).into_iter().map(|e| self.event_data(e)).collect();
        let mut anims = Vec::new();
        for a in self.lst(sk, 7) {
            if self.i(a, 5, 1) != 0 {
                // 5: 내보내기 포함 여부
                anims.push(self.animation(a)?);
            }
        }
        m.animations = anims;
        m.strings = self.string_table(sk)?;
        Ok(m)
    }

    /// `.skel` 문자열 표(파이썬 model.py string_table): 첨부 목록(이름순, 자리표시자 포함)을 돌며 이름과 경로.
    fn string_table(&self, sk: Option<NodeId>) -> R<Vec<String>> {
        let mut out = Vec::new();
        for a in self.lst(sk, 6) {
            let a = a.ok_or_else(null_node)?;
            out.push(self.att_name(a).unwrap_or_default());
            let tag = match self.cls(a) {
                "RegionAtt" => 13,
                "MeshAtt" => 14,
                _ => -1,
            };
            if let Some(path) = self.p.get_string(Some(a), tag) {
                if !path.is_empty() {
                    out.push(path.to_string());
                }
            }
        }
        Ok(out)
    }

    // ------------------------------------------------------------ 뼈·슬롯

    fn bone(&self, b: Option<NodeId>) -> R<ModelBone> {
        let parent = self.node_of(b, 3, self.c.bone);
        Ok(ModelBone {
            name: self.name(b),
            parent: if parent.is_some() { self.name(parent) } else { None },
            length: self.f(b, 9, 0.0),
            x: self.f(b, 10, 0.0),
            y: self.f(b, 11, 0.0),
            rotation: self.f(b, 12, 0.0),
            scale_x: self.f(b, 13, 1.0),
            scale_y: self.f(b, 14, 1.0),
            shear_x: self.f(b, 23, 0.0),
            shear_y: self.f(b, 25, 0.0),
            inherit: Some(inherits(self.leaf_long(b, 27, 1)?).unwrap_or("normal").to_string()),
            color: rgba(self.leaf_color(b, 17)?),
            icon: Some(match self.p.get_string(b, 28) {
                Some(s) if !s.is_empty() => s.to_string(),
                _ => "bone".to_string(),
            }),
            skin: false,
            visible: self.i(b, 2, 1) != 0,
            editor33: self.f(b, 33, 1.0),
            editor34: self.f(b, 34, 0.0),
        })
    }

    fn slot(&self, s: Option<NodeId>) -> R<ModelSlot> {
        let mut att = self.node_of(s, 9, GK::Node);
        if let Some(a) = att {
            if !self.exported(Some(a))? {
                att = None;
            }
        }
        let dark = self.i(s, 12, 0) != 0;
        Ok(ModelSlot {
            name: self.name(s),
            bone: self.name(self.node_of(s, 2, self.c.bone)),
            color: rgba(Some(self.leaf_color(s, 5)?.unwrap_or(WHITE))),
            dark: if dark { rgb(self.leaf_color(s, 11)?) } else { None },
            attachment: match att {
                Some(a) => self.att_key(Some(a))?,
                None => None,
            },
            blend: Some(blends(self.leaf_long(s, 7, 1)?).unwrap_or("normal").to_string()),
            visible: self.i(s, 3, 1) != 0,
        })
    }

    // ------------------------------------------------------------ 제약

    fn constraint_order(&self, sk: Option<NodeId>) -> R<Vec<Option<NodeId>>> {
        let old = self.p.get_node(sk, 25, self.c.list);
        if old.is_some() {
            return Ok(self.p.items(old));
        }

        let mut folder = self.p.get_node(sk, 44, self.c.folder);
        if folder.is_none() {
            let mut cands = self.p.doc.of("IkConstraint");
            cands.extend(self.p.doc.of("TransformConstraint"));
            for c in cands {
                if self.p.get_node(Some(c), 0, self.c.skeleton) != sk {
                    continue; // 다른 스켈레톤의 제약 폴더를 쓰지 않는다
                }
                folder = self.p.get_node(Some(c), 32, self.c.folder);
                if folder.is_some() {
                    break;
                }
            }
        }

        let mut output = Vec::new();
        if let Some(f) = folder {
            self.walk(f, &mut output, 0)?;
        }
        output.reverse();
        Ok(output)
    }

    fn walk(&self, folder: NodeId, output: &mut Vec<Option<NodeId>>, depth: u32) -> R<()> {
        if depth > 10_000 {
            return Err(broken("폴더가 순환합니다"));
        }
        let list = self
            .p
            .get_node(Some(folder), 5, self.c.list)
            .or_else(|| self.p.get_node(Some(folder), 4, self.c.list));
        for x in self.p.items(list).into_iter().flatten() {
            if self.cls(x) == "Folder" {
                self.walk(x, output, depth + 1)?;
            } else {
                output.push(Some(x));
            }
        }
        Ok(())
    }

    fn constraints(&self, sk: Option<NodeId>) -> R<Vec<ModelConstraint>> {
        let mut order = self.constraint_order(sk)?;
        let mut nodes = self.lst(sk, 16);
        nodes.extend(self.lst(sk, 22));
        nodes.extend(self.lst(sk, 23));
        let seen: HashSet<NodeId> = order.iter().flatten().copied().collect();
        order.extend(nodes.into_iter().filter(|n| n.map_or(true, |n| !seen.contains(&n))));
        let keys38 = self.p.keys38();
        let mut output = Vec::with_capacity(order.len());
        for (i, &c) in order.iter().enumerate() {
            let skin = !self.lst(c, 22).is_empty();
            let mut o = ModelConstraint { name: self.name(c), order: i as i32, skin, ..Default::default() };
            let cn = c.ok_or_else(null_node)?;
            match self.cls(cn) {
                "IkConstraint" => {
                    o.r#type = Some("ik".into());
                    o.bones = self.lst(c, 2).into_iter().map(|b| self.name_or_empty(b)).collect();
                    o.target = self.name(self.node_of(c, 3, self.c.bone));
                    o.mix = self.f(c, 5, 1.0);
                    o.softness = self.f(c, 14, 0.0);
                    // 3.8: 필드 4 가 굽힘 방향(1 음, 2 양)
                    o.bend_positive = if keys38 { self.i(c, 4, 2) != 1 } else { self.i(c, 16, 1) != 0 };
                    o.compress = self.i(c, 9, 0) != 0;
                    o.stretch = self.i(c, 11, 0) != 0;
                    o.uniform = self.i(c, 13, 0) != 0;
                }
                "TransformConstraint" => {
                    o.r#type = Some("transform".into());
                    o.bones = self.lst(c, 19).into_iter().map(|b| self.name_or_empty(b)).collect();
                    o.target = self.name(self.node_of(c, 3, self.c.bone));
                    o.rotation = self.f(c, 14, 0.0);
                    o.x = self.f(c, 6, 0.0);
                    o.y = self.f(c, 7, 0.0);
                    o.scale_x = self.f(c, 16, 0.0);
                    o.scale_y = self.f(c, 17, 0.0);
                    o.shear_y = self.f(c, 18, 0.0);
                    o.local = self.i(c, 20, 0) != 0;
                    o.relative = self.i(c, 21, 0) != 0;
                    o.mixes[ModelConstraint::MIX_ROTATE] = self.f(c, 8, 1.0);
                    o.mixes[ModelConstraint::MIX_X] = self.f(c, 4, 1.0);
                    // 3.8 은 이동·크기 mix 가 축마다 따로가 아니다(필드 23·25 없음) → x 값을 y 에도 쓴다
                    o.mixes[ModelConstraint::MIX_Y] = self.f(c, 23, if keys38 { self.f(c, 4, 1.0) } else { 1.0 });
                    o.mixes[ModelConstraint::MIX_SCALE_X] = self.f(c, 10, 1.0);
                    o.mixes[ModelConstraint::MIX_SCALE_Y] = self.f(c, 25, if keys38 { self.f(c, 10, 1.0) } else { 1.0 });
                    o.mixes[ModelConstraint::MIX_SHEAR_Y] = self.f(c, 12, 1.0);
                    o.properties = self.transform_props(c)?;
                }
                "PathConstraint" => {
                    o.r#type = Some("path".into());
                    o.bones = self.lst(c, 2).into_iter().map(|b| self.name_or_empty(b)).collect();
                    o.target = self.name(self.node_of(c, 3, self.c.slot));
                    o.spacing_mode = Some(spacing_modes(self.leaf_long(c, 14, 1)?).unwrap_or("length").into());
                    o.rotate_mode = Some(rotate_modes(self.leaf_long(c, 15, 1)?).unwrap_or("tangent").into());
                    o.position_mode = Some(position_modes(self.leaf_long(c, 16, 2)?).unwrap_or("percent").into());
                    o.position = self.f(c, 4, 0.0);
                    o.spacing = self.f(c, 12, 0.0);
                    o.rotation = self.f(c, 10, 0.0);
                    o.mixes[ModelConstraint::MIX_ROTATE] = self.f(c, 8, 1.0);
                    o.mixes[ModelConstraint::MIX_X] = self.f(c, 6, 1.0);
                    o.mixes[ModelConstraint::MIX_Y] = self.f(c, 23, if keys38 { self.f(c, 6, 1.0) } else { 1.0 });
                }
                "PhysicsConstraint" => {
                    // 저장값 a(태그 7)·b(태그 9) → rotate = a·t, shearX = b·t, t = a/(a+b) (a 가 0이면 t = 1).
                    let a0 = self.f(c, 7, 0.0);
                    let b0 = self.f(c, 9, 0.0);
                    let t0 = if a0 != 0.0 { a0 / (a0 + b0) } else { 1.0 };
                    o.r#type = Some("physics".into());
                    o.bone = self.name(self.node_of(c, 3, self.c.bone));
                    o.x = self.f(c, 5, 0.0);
                    o.y = self.f(c, 6, 0.0);
                    o.rotate = a0 * t0;
                    o.scale_x = self.f(c, 8, 0.0);
                    o.shear_x = b0 * t0;
                    o.limit = self.f(c, 33, 5000.0);
                    o.fps = 1.0 / self.f(c, 4, 1.0 / 60.0);
                    o.inertia = self.f(c, 10, 1.0);
                    o.strength = self.f(c, 12, 100.0);
                    o.damping = self.f(c, 14, 1.0);
                    o.mass = self.f(c, 16, 1.0);
                    o.wind = self.f(c, 18, 0.0);
                    o.gravity = self.f(c, 20, 0.0);
                    o.mix = self.f(c, 23, 1.0);
                    o.inertia_global = self.i(c, 25, 0) != 0;
                    o.strength_global = self.i(c, 26, 0) != 0;
                    o.damping_global = self.i(c, 27, 0) != 0;
                    o.mass_global = self.i(c, 28, 0) != 0;
                    o.wind_global = self.i(c, 29, 0) != 0;
                    o.gravity_global = self.i(c, 30, 0) != 0;
                    o.mix_global = self.i(c, 31, 0) != 0;
                }
                "SliderConstraint" => {
                    let prop = self.node_of(c, 11, GK::Node);
                    let animation = self.node_of(c, 2, self.c.animation);
                    let bone = self.node_of(c, 10, self.c.bone);
                    o.r#type = Some("slider".into());
                    o.animation = match animation {
                        Some(_) => self.full_name(animation, 7)?,
                        None => None,
                    };
                    o.bone = if bone.is_some() { self.name(bone) } else { None };
                    o.property = match prop {
                        Some(pn) => Some(prop_name(self.cls(pn), 6, 134)?.to_string()),
                        None => None,
                    };
                    o.r#loop = self.i(c, 9, 0) != 0;
                    o.local = self.i(c, 13, 0) != 0;
                    o.max = self.f(c, 4, 0.0);
                    o.scale = self.f(c, 12, 1.0);
                    o.mix = self.f(c, 6, 1.0);
                    o.offset = self.f(c, 15, 0.0);
                    o.property_offset = match prop {
                        Some(pn) => self.number_field(pn, KEY_P0)?,
                        None => 0.0,
                    };
                }
                other => return Err(unsupported(format!("아직 지원하지 않는 제약: {other}"))),
            }
            output.push(o);
        }
        Ok(output)
    }

    /// x.F[key] 의 숫자 값(C# Number: float 또는 long).
    fn number_field(&self, n: NodeId, key: u32) -> R<f64> {
        match self.p.doc.node(n).field(key) {
            None => Err(broken("주어진 키가 사전에 없습니다")),
            Some(f) => match f.value {
                Value::Float(v) => Ok(v as f64),
                Value::Int(v) => Ok(v as f64),
                Value::Null => Err(null_node()),
                _ => Err(broken("지정된 캐스트가 잘못되었습니다")),
            },
        }
    }

    /// 4.3: 속성 목록(29) — XfFrom134~139(rotate…shearY) {p0 오프셋, p1 [XfTo140~145 (오프셋, 최대, 배율)]}.
    /// 4.2까지는 없다(None) — 출력기가 여섯 속성 모두로 본다.
    fn transform_props(&self, c: Option<NodeId>) -> R<Option<Vec<ModelFromProperty>>> {
        let list = self.p.get_node(c, 29, self.c.list);
        if list.is_none() {
            return Ok(None);
        }
        let mut output = Vec::new();
        for x in self.p.items(list) {
            let x = x.ok_or_else(null_node)?;
            let cls = self.cls(x);
            if !cls.starts_with("XfFrom") {
                continue;
            }
            let mut source = ModelFromProperty {
                name: Some(prop_name(cls, 6, 134)?.to_string()),
                offset: self.number_field(x, KEY_P0)?,
                to: Vec::new(),
            };
            let p1 = self.p.doc.node(x).field(KEY_P1).ok_or_else(|| broken("주어진 키가 사전에 없습니다"))?;
            if let Value::List(tos) = &p1.value {
                for raw in tos.iter() {
                    let t = match raw {
                        Value::Node(t) => *t,
                        Value::Null => return Err(null_node()),
                        _ => return Err(broken("지정된 캐스트가 잘못되었습니다")),
                    };
                    let values = self.p.floats(self.p.body_start(t), 3)?;
                    source.to.push(ModelToProperty {
                        name: Some(prop_name(self.cls(t), 4, 140)?.to_string()),
                        offset: values[0],
                        max: values[1],
                        scale: values[2],
                    });
                }
            }
            output.push(source);
        }
        Ok(Some(output))
    }

    // ------------------------------------------------------------ 스킨·첨부

    fn att_key(&self, a: Option<NodeId>) -> R<Option<String>> {
        let placeholder = self.p.get_node(a, 4, self.c.placeholder);
        if placeholder.is_some() {
            return Ok(self.p.get_string(placeholder, 1).map(str::to_string));
        }
        let a = a.ok_or_else(null_node)?;
        Ok(self.att_name(a))
    }

    fn att_name(&self, a: NodeId) -> Option<String> {
        if let Some(n) = self.p.get_string(Some(a), 1) {
            return Some(n.to_string());
        }
        // 정점 첨부: 이름이 다른 단계에 있다(d1 등)
        let doc = self.p.doc;
        for f in &doc.node(a).fields {
            let key = doc.keys.name(f.key).trim_start_matches(['t', 'd']);
            if key.rsplit(':').next() == Some("1") {
                if let Value::Str(s) = &f.value {
                    return Some(s.to_string());
                }
            }
        }
        None
    }

    /// 첨부의 Export 체크(태그 5). 꺼져 있으면 첨부와 그 키가 내보내기에서 빠진다.
    fn exported(&self, a: Option<NodeId>) -> R<bool> {
        let a = a.ok_or_else(null_node)?;
        Ok(self.export_all || self.cls(a) == "Placeholder" || self.i(Some(a), 5, 1) != 0)
    }

    fn att_slot(&self, a: Option<NodeId>) -> Option<NodeId> {
        self.p.get_node(a, 0, self.c.slot)
    }

    /// 폴더 밖 항목이 먼저, 그다음 폴더별(폴더 안에서 다시 같은 규칙).
    fn skin_order_key(&self, n: Option<NodeId>) -> R<Vec<(i32, String)>> {
        let name = self.skin_name(n)?.ok_or_else(null_node)?;
        let parts: Vec<&str> = name.split('/').collect();
        let last = parts.len() - 1;
        Ok(parts.iter().enumerate().map(|(i, p)| (if i == last { 0 } else { 1 }, p.to_lowercase())).collect())
    }

    fn in_default(&self, a: Option<NodeId>) -> R<bool> {
        // 6번 목록에는 모든 스킨의 첨부와 자리표시자가 함께 있다
        let an = a.ok_or_else(null_node)?;
        Ok(self.cls(an) != "Placeholder" && self.p.get_node(a, 4, self.c.placeholder).is_none())
    }

    fn skins(&self, sk: Option<NodeId>) -> R<Vec<ModelSkin>> {
        let mut defaults = Vec::new();
        for a in self.lst(sk, 6) {
            if self.in_default(a)? {
                defaults.push(a);
            }
        }
        let mut output = vec![ModelSkin {
            name: Some("default".into()),
            attachments: self.skin_atts(&defaults)?,
            ..Default::default()
        }];

        let mut keyed = Vec::with_capacity(self.skin_nodes.len());
        for &s in &self.skin_nodes {
            keyed.push((self.skin_order_key(s)?, s));
        }
        keyed.sort_by(|a, b| compare_skin_keys(&a.0, &b.0));
        for (_, s) in keyed {
            output.push(ModelSkin {
                name: self.skin_name(s)?,
                color: rgba(self.leaf_color(s, 8)?),
                bones: self.lst(s, 6).into_iter().map(|b| self.name_or_empty(b)).collect(),
                constraints: self.lst(s, 7).into_iter().map(|b| self.name_or_empty(b)).collect(),
                attachments: self.skin_atts(&self.lst(s, 2))?,
            });
        }
        Ok(output)
    }

    fn skin_atts(&self, atts: &[Option<NodeId>]) -> R<Vec<ModelSkinEntry>> {
        struct Row {
            slot_index: usize,
            key: Option<String>,
            attachment: ModelAttachment,
        }
        let mut rows = Vec::new();
        for &a in atts {
            let an = a.ok_or_else(null_node)?;
            if self.cls(an) == "Placeholder" || !self.exported(a)? {
                continue; // 자리표시자, Export 를 끈 첨부
            }
            let slot = self.att_slot(a);
            let slot_index = slot.and_then(|s| self.slot_index.get(&s).copied()).unwrap_or(1 << 30);
            let key = self.att_key(a)?;
            let attachment = self.attachment(an)?;
            rows.push(Row { slot_index, key, attachment });
        }
        rows.sort_by(|a, b| a.slot_index.cmp(&b.slot_index).then_with(|| cmp_ordinal(a.key.as_deref(), b.key.as_deref())));
        let mut out = Vec::with_capacity(rows.len());
        for r in rows {
            let slot = *self.slots.get(r.slot_index).ok_or_else(|| broken("인덱스가 범위를 벗어났습니다"))?;
            out.push(ModelSkinEntry { slot: self.name(slot), key: r.key, attachment: Some(r.attachment) });
        }
        Ok(out)
    }

    fn vertex_data(&self, a: NodeId, o: &mut ModelAttachment) -> R<()> {
        let a = Some(a);
        let bones = self.lst(a, 1);
        let vws = self.lst(a, 2);
        let weighted = self.i(a, 30, 0) != 0;
        let mut verts = Vec::new();
        let mut dense_map: Vec<i32> = Vec::new();
        let mut setup = Vec::new();
        let null_arg = || broken("값은 null 일 수 없습니다");
        if !weighted {
            for (vi, &vw) in vws.iter().enumerate() {
                let pos = self.leaf_floats(vw, 1)?.ok_or_else(null_arg)?;
                verts.extend(pos.iter().take(2));
                setup.extend(pos.iter().take(2));
                let vi = vi as i32;
                dense_map.push(vi * 2);
                dense_map.push(vi * 2 + 1);
            }
        } else {
            let nb = bones.len();
            let oob = || broken("인덱스가 범위를 벗어났습니다");
            for (vi, &vw) in vws.iter().enumerate() {
                let weights = self.leaf_floats(vw, 0)?.ok_or_else(null_node)?;
                let pos = self.leaf_floats(vw, 1)?;
                let mut used = Vec::new();
                for bi in 0..nb {
                    if *weights.get(bi).ok_or_else(oob)? != 0.0 {
                        used.push(bi);
                    }
                }
                verts.push(used.len() as f64);
                let pos = pos.ok_or_else(null_node)?;
                for bi in used {
                    let bone = bones[bi].ok_or_else(|| broken("값은 null 일 수 없습니다"))?;
                    let index = *self.bone_index.get(&bone).ok_or_else(|| broken("주어진 키가 사전에 없습니다"))?;
                    let px = *pos.get(bi * 2).ok_or_else(oob)?;
                    let py = *pos.get(bi * 2 + 1).ok_or_else(oob)?;
                    verts.push(index as f64);
                    verts.push(px);
                    verts.push(py);
                    verts.push(weights[bi]);
                    setup.push(px);
                    setup.push(py);
                    let d = ((vi * nb + bi) * 2) as i32;
                    dense_map.push(d);
                    dense_map.push(d + 1);
                }
            }
        }
        o.vertices = Some(verts);
        o.weighted = weighted;
        o.dense_map = Some(dense_map);
        o.setup = Some(setup);
        Ok(())
    }

    /// 가중치 메시의 삼각형을 Spine 이 내보내는 순서로 만든다.
    ///
    /// Spine 4.2 이하는 프로젝트를 열 때 가중치 메시마다 삼각형을 한 번 정렬하는데(`sort_triangles_once`), linked mesh 는
    /// 부모와 삼각형 배열을 함께 써서 linked mesh 하나마다 같은 배열이 한 번 더 정렬된다. 정렬은 같은 뼈의 삼각형
    /// 순서를 뒤집으므로, 결과는 "1 + linked mesh 수"가 홀수인지 짝수인지로 갈린다. 뼈가 하나뿐이면 정렬하지 않는다.
    /// (공식 3.8 · 4.0 · 4.1 · 4.2 예제와 linked mesh 가 많은 4.2 프로젝트를 Spine 내보내기와 대조해 확인.)
    fn sort_triangles(&self, an: NodeId, tri: &mut [i64]) -> R<()> {
        let a = Some(an);
        let bones = self.lst(a, 1).len();
        // Spine 4.3 편집기는 저장된 순서 그대로 내보낸다(공식 4.3 예제와 대조).
        if bones < 2 || !self.sort_triangles {
            return Ok(());
        }
        let mut weights: Vec<Vec<f32>> = Vec::new();
        for vw in self.lst(a, 2) {
            let w = self.leaf_floats(vw, 0)?.ok_or_else(null_node)?;
            weights.push((0..bones).map(|b| w.get(b).copied().unwrap_or(0.0) as f32).collect());
        }
        sort_triangles_once(tri, &weights);
        if self.linked_children.get(&an).copied().unwrap_or(0) % 2 == 1 {
            sort_triangles_once(tri, &weights);
        }
        Ok(())
    }

    fn sequence(&self, q: Option<NodeId>) -> Option<ModelSequence> {
        q?;
        Some(ModelSequence {
            count: zigzag_dec(self.i(q, 2, 0)),
            start: zigzag_dec(self.i(q, 1, 0)),
            digits: zigzag_dec(self.i(q, 3, 0)),
            setup: zigzag_dec(self.i(q, 4, 0)),
        })
    }

    fn attachment(&self, an: NodeId) -> R<ModelAttachment> {
        let a = Some(an);
        let mut o = ModelAttachment { name: self.att_name(an), key: self.att_key(a)?, ..Default::default() };
        let cls = self.cls(an);
        let get_str = |tag| Some(self.p.get_string(a, tag).unwrap_or("").to_string());
        if cls == "RegionAtt" {
            o.r#type = Some("region".into());
            o.path = get_str(13);
            o.x = self.f(a, 6, 0.0);
            o.y = self.f(a, 7, 0.0);
            o.scale_x = self.f(a, 8, 1.0);
            o.scale_y = self.f(a, 9, 1.0);
            o.rotation = self.f(a, 10, 0.0);
            o.width = self.f(a, 11, 0.0);
            o.height = self.f(a, 12, 0.0);
            o.color = rgba(Some(self.leaf_color(a, 14)?.unwrap_or(WHITE)));
            o.sequence = self.sequence(self.p.get_node(a, 15, self.c.sequence));
            return Ok(o);
        }

        if matches!(cls, "MeshAtt" | "BBoxAtt" | "ClippingAtt" | "PathAtt") {
            self.vertex_data(an, &mut o)?;
            o.vertex_count = self.lst(a, 2).len() as i32;
            o.color = rgba(Some(self.leaf_color(a, 31)?.unwrap_or(WHITE)));
            if cls == "BBoxAtt" {
                o.r#type = Some("boundingbox".into());
            } else if cls == "PathAtt" {
                o.r#type = Some("path".into());
                o.closed = self.i(a, 12, 0) != 0;
                o.constant_speed = self.i(a, 13, 1) != 0;
                o.lengths = Some(self.leaf_floats(a, 15)?.unwrap_or_default());
            } else if cls == "ClippingAtt" {
                o.r#type = Some("clipping".into());
                o.end = self.name(self.p.get_node(a, 12, self.c.slot));
                o.convex = self.p.format.starts_with("4.3") && self.i(a, 13, 0) != 0; // 4.3 새 필드
            } else {
                let parent = self.p.get_node(a, 26, self.c.mesh_att);
                if parent.is_some() {
                    let owner = self.p.get_node(parent, 32, self.c.skin);
                    o.r#type = Some("linkedmesh".into());
                    o.path = get_str(14);
                    o.parent = self.att_key(parent)?;
                    o.parent_slot = self.name(self.att_slot(parent));
                    o.skin = if owner.is_some() { self.skin_name(owner)? } else { None };
                    o.timelines = self.i(a, 27, 1) != 0;
                    o.width = self.f(a, 16, 0.0);
                    o.height = self.f(a, 17, 0.0);
                    return Ok(o);
                }

                let mut tri = self.leaf_ints(a, 13)?.unwrap_or_default();
                if o.weighted {
                    self.sort_triangles(an, &mut tri)?;
                }
                o.sequence = self.sequence(self.p.get_node(a, 36, self.c.sequence));
                o.r#type = Some("mesh".into());
                o.path = get_str(14);
                o.uvs = Some(self.leaf_floats(a, 12)?.unwrap_or_default());
                // 7번 필드에 hull 값 수(정점 수 × 2)가 들어 있다. 삼각형 가장자리로 세면 hull 밖에 안쪽 정점이 있는
                // 메시(Spine 이 "internal vertex outside hull" 로 경고하는 것)에서 어긋난다. 필드가 없을 때만 센다.
                let stored_hull = self.i(a, 7, -1);
                o.hull = if stored_hull >= 0 { (zigzag_dec(stored_hull) / 2) as i32 } else { hull_count(&tri) };
                o.triangles = Some(tri);
                o.edges = Some(self.leaf_ints(a, 8)?.unwrap_or_default());
                o.width = self.f(a, 16, 0.0);
                o.height = self.f(a, 17, 0.0);
            }
            return Ok(o);
        }

        if cls == "PointAtt" {
            o.r#type = Some("point".into());
            o.x = self.f(a, 6, 0.0);
            o.y = self.f(a, 7, 0.0);
            o.rotation = self.f(a, 8, 0.0);
            o.color = rgba(Some(self.leaf_color(a, 9)?.unwrap_or([241, 241, 0, 255])));
            return Ok(o);
        }

        Err(unsupported(format!("아직 지원하지 않는 첨부: {cls}")))
    }

    // ------------------------------------------------------------ 이벤트

    fn event_data(&self, e: Option<NodeId>) -> ModelEvent {
        ModelEvent {
            name: self.p.get_string(e, 2).map(str::to_string),
            int: zigzag_dec(self.i(e, 3, 0)),
            float: self.f(e, 4, 0.0),
            string: Some(self.p.get_string(e, 5).unwrap_or("").to_string()),
            audio: Some(self.p.get_string(e, 6).unwrap_or("").to_string()),
            volume: self.f(e, 8, 1.0),
            balance: self.f(e, 9, 0.0),
        }
    }

    // ------------------------------------------------------------ 애니메이션

    fn animation(&self, an: Option<NodeId>) -> R<ModelAnimation> {
        let mut output = ModelAnimation {
            name: self.full_name(an, 7)?,
            color: rgba(Some(self.leaf_color(an, 10)?.unwrap_or(WHITE))),
            no_clean: self.i(an, 9, 0) != 0,
            timelines: Vec::new(),
        };
        for g in self.lst(an, 2) {
            let target = self.node_of(g, 1, GK::Node);
            for tl in self.lst(g, 2) {
                let tl = tl.ok_or_else(null_node)?;
                if let Some(t) = self.timeline(target, tl)? {
                    output.timelines.push(t);
                }
            }
        }
        Ok(output)
    }

    fn timeline_type(&self, tl: NodeId) -> R<i64> {
        if let Some(f) = self.p.doc.node(tl).field(KEY_TYPE) {
            return match f.value {
                Value::Int(v) => Ok(v),
                _ => Err(broken("지정된 캐스트가 잘못되었습니다")),
            };
        }

        let leaf_long = |n: Option<NodeId>| -> R<i64> {
            match self.p.leaf(n)? {
                Leaf::Long(v) => Ok(v),
                Leaf::Null => Err(null_node()),
                _ => Err(broken("지정된 캐스트가 잘못되었습니다")),
            }
        };
        if self.p.keys38() {
            let raw = leaf_long(self.p.get_node(Some(tl), 1, self.c.timeline_type))?;
            return types38(raw).ok_or_else(|| unsupported(format!("아직 지원하지 않는 타임라인 종류 3.8:{raw}")));
        }
        leaf_long(self.p.get_node(Some(tl), 9, GK::Node))
    }

    fn timeline_keys(&self, tl: NodeId) -> Vec<Option<NodeId>> {
        if let Some(f) = self.p.doc.node(tl).field(KEY_KEYS) {
            return match f.value {
                Value::Node(n) => self.p.items(Some(n)),
                _ => Vec::new(),
            };
        }
        self.lst(Some(tl), 2)
    }

    /// 새 방식: extra&lt;종류&gt;, 옛 방식: 3 슬롯, 4 이벤트 정의, 5 첨부, 6 IK, 7 변형.
    fn timeline_extra(&self, tl: NodeId, ty: i64) -> Option<NodeId> {
        let node = self.p.doc.node(tl);
        if node.has(KEY_TYPE) {
            let key = self.p.doc.keys.find(&format!("extra{ty}"))?;
            return match node.field(key)?.value {
                Value::Node(n) => Some(n),
                _ => None,
            };
        }
        let tag = match ty {
            4 | 5 => 3,
            6 => 5,
            7 => 6,
            8 => 7,
            9 | 10 | 11 => 8,
            12 => 4,
            22 => 3,
            27 | 40 => 11,
            _ => return None,
        };
        self.p.get_node(Some(tl), tag, GK::Node)
    }

    fn timeline(&self, target: Option<NodeId>, tl: NodeId) -> R<Option<ModelTimeline>> {
        let ty = self.timeline_type(tl)?;
        let Some((kind, value_count)) = timeline_info(ty) else {
            return Err(unsupported(format!("아직 지원하지 않는 타임라인 종류 {ty}")));
        };
        let extra = self.timeline_extra(tl, ty);
        let new_style = self.p.doc.node(tl).has(KEY_TYPE);
        let mut t = ModelTimeline { r#type: Some(kind.to_string()), code: ty as i32, ..Default::default() };
        match kind {
            "rotate" | "translate" | "scale" | "shear" | "translatex" | "translatey" => t.bone = self.name(target),
            "color" | "attachment" => t.slot = self.name(extra),
            "deform" => {
                let att = extra;
                if !self.exported(att)? {
                    return Ok(None);
                }
                t.slot = if new_style { self.name(self.att_slot(att)) } else { self.name(self.p.get_node(Some(tl), 3, self.c.slot)) };
                t.attachment = self.att_key(att)?;
                let skin = self.p.get_node(att, 32, self.c.skin);
                t.skin = if skin.is_some() { self.skin_name(skin)? } else { Some("default".into()) };
            }
            "ik" | "transform" | "pathPosition" | "pathSpacing" | "pathMix" | "physicsWind" | "sliderMix" => {
                t.constraint = self.name(extra)
            }
            "sequence" => {
                let att = if new_style {
                    let doc = self.p.doc;
                    let f = doc
                        .keys
                        .find("extraB22")
                        .and_then(|k| doc.node(tl).field(k))
                        .ok_or_else(|| broken("주어진 키가 사전에 없습니다"))?;
                    match f.value {
                        Value::Node(n) => Some(n),
                        Value::Null => None,
                        _ => return Err(broken("지정된 캐스트가 잘못되었습니다")),
                    }
                } else {
                    self.p.get_node(Some(tl), 10, GK::Node)
                };
                if !self.exported(att)? {
                    return Ok(None);
                }
                t.slot = self.name(extra);
                t.attachment = self.att_key(att)?;
                let skin = self.p.get_node(att, 32, self.c.skin);
                t.skin = if skin.is_some() { self.skin_name(skin)? } else { Some("default".into()) };
            }
            _ => {}
        }

        let nc = curve_channels(ty);
        let key_nodes = self.timeline_keys(tl);
        let mut keys = Vec::with_capacity(key_nodes.len());
        for &k in &key_nodes {
            let k = k.ok_or_else(null_node)?;
            keys.push(self.key(k, ty, value_count, nc)?);
        }

        if self.p.keys38() {
            if kind == "rotate" {
                for k in keys.iter_mut() {
                    k.angle38 = k.values.as_ref().and_then(|v| v.first().copied());
                }
                unwrap38(&mut keys)?;
            }
            self.handles38(&mut keys, &key_nodes, nc)?;
        }
        t.keys = keys;
        Ok(Some(t))
    }

    /// 3.8 곡선 → 4.x 손잡이. 3.8 은 키마다 정규화된 곡선 하나(c1 c2 c3 c4, 시간·값 모두 0~1)를 모든 채널에 쓴다.
    /// 4.x 는 채널마다 (시간, 값) 손잡이 둘: 이 키에서 나가는 것과 다음 키로 들어가는 것.
    fn handles38(&self, keys: &mut [ModelKey], nodes: &[Option<NodeId>], channels: i32) -> R<()> {
        let channels = channels.max(0) as usize;
        for k in keys.iter_mut() {
            k.handles = Some(vec![[UNSET; 4]; channels]);
        }
        for i in 0..keys.len().saturating_sub(1) {
            let (head, tail) = keys.split_at_mut(i + 1);
            let a = &mut head[i];
            let b = &mut tail[0];
            if a.curve.as_deref() != Some("bezier") {
                continue;
            }
            if let (Some(ad), Some(bd)) = (&a.dense, &b.dense) {
                if same_dense(ad, bd) {
                    a.curve = Some("linear".into()); // 모양이 같은 변형 키 사이는 곡선이 뜻이 없다(Spine 도 계단으로 내보낸다)
                    continue;
                }
            }
            let c1 = self.f(nodes[i], 4, 0.25);
            let c2 = self.f(nodes[i], 5, 0.0);
            let c3 = self.f(nodes[i], 6, 0.75);
            let c4 = self.f(nodes[i], 7, 1.0);
            a.curve38 = Some([c1, c2, c3, c4]);
            let dt = b.frame - a.frame;
            let av = a.values.as_deref().unwrap_or(&[]);
            let bv = b.values.as_deref().unwrap_or(&[]);
            let mut pairs = Vec::with_capacity(channels);
            for c in 0..channels {
                let (mut v0, mut v1) = (0.0, 1.0); // 값 없는 타임라인(변형): 곡선은 0→1 비율
                if av.len() > c && bv.len() > c {
                    v0 = av[c];
                    v1 = bv[c];
                }
                pairs.push((v0, v1));
            }
            let af = a.frame;
            let ah = a.handles.as_mut().unwrap();
            for (c, &(v0, v1)) in pairs.iter().enumerate() {
                ah[c][2] = af + dt * c1;
                ah[c][3] = v0 + (v1 - v0) * c2;
            }
            let bh = b.handles.as_mut().unwrap();
            for (c, &(v0, v1)) in pairs.iter().enumerate() {
                bh[c][0] = af + dt * c3;
                bh[c][1] = v0 + (v1 - v0) * c4;
            }
        }
        Ok(())
    }

    fn bits38(&self, k: NodeId, key: u32) -> R<i64> {
        let f = self.p.doc.node(k).field(key).ok_or_else(|| broken("주어진 키가 사전에 없습니다"))?;
        let at = f.at as usize;
        let b = self.p.body.get(at..at + 4).ok_or_else(|| broken("인덱스가 배열 범위를 벗어났습니다"))?;
        Ok(((b[0] as i64) << 24) | ((b[1] as i64) << 16) | ((b[2] as i64) << 8) | b[3] as i64)
    }

    /// 3.8 키의 값(필드 8~11, 17)을 4.x 모델의 채널 순서로 옮긴다.
    fn values38(&self, k: NodeId, ty: i64) -> R<Vec<f64>> {
        let kn = Some(k);
        let v8 = self.f(kn, 8, 0.0);
        let v9 = self.f(kn, 9, 0.0);
        let v10 = self.f(kn, 10, 0.0);
        let v11 = self.f(kn, 11, 0.0);
        Ok(match ty {
            0 | 9 | 10 => vec![v8],
            1 | 2 | 3 => vec![v8, v9],
            4 => {
                // 색은 float 비트에 ABGR 로 들어 있다(알파는 아래 1비트가 깎여 있어 필드 10 의 값을 쓴다). 9 = 어두운 색.
                let light = self.bits38(k, KEY_8)?;
                let dark = self.bits38(k, KEY_9)?;
                vec![
                    byte01(light, 0),
                    byte01(light, 8),
                    byte01(light, 16),
                    (v10 * 255.0 + 1e-6).floor() / 255.0, // 알파는 8비트로 내림(Spine 과 같게)
                    byte01(dark, 0),
                    byte01(dark, 8),
                    byte01(dark, 16),
                ]
            }
            7 => vec![v8, self.f(kn, 17, 0.0), v11, v10, if v9 > 0.0 { 1.0 } else { 0.0 }],
            8 => vec![v8, v9, v9, v10, v10, v11], // 3.8 은 이동·크기 mix 가 축마다 따로가 아니다
            11 => vec![v8, v9, v9],
            _ => Vec::new(),
        })
    }

    fn key(&self, k: NodeId, ty: i64, n: i32, nc: i32) -> R<ModelKey> {
        let p = self.p;
        let kn = Some(k);
        let node = p.doc.node(k);
        let frame;
        let curve;
        let vals;
        let handles;
        let mut payload: Option<NodeId> = None;
        let curve_of = |c: i64, bezier: i64, stepped: i64| {
            if c == bezier {
                "bezier"
            } else if c == stepped {
                "stepped"
            } else {
                "linear"
            }
        };
        if let Some(time) = node.field(KEY_TIME) {
            // 4.2.22~: 고정 배치
            frame = match time.value {
                Value::Float(f) => f as f64,
                _ => return Err(broken("지정된 캐스트가 잘못되었습니다")),
            };
            let flag_f = node.field(KEY_FLAG).ok_or_else(|| broken("주어진 키가 사전에 없습니다"))?;
            let flag = match flag_f.value {
                Value::Int(v) => v,
                _ => return Err(broken("지정된 캐스트가 잘못되었습니다")),
            };
            curve = curve_of(flag, 1, 2);
            let vals_at = time.end.ok_or_else(|| broken("값이 없습니다"))? as usize;
            vals = p.floats(vals_at, n as i64)?;
            if p.curve_conditional() {
                // 4.3.04~: 손잡이 블록은 플래그 뒤에, 이 키(나 앞 키)가 베지어일 때만 있다
                let after = flag_f.end.ok_or_else(|| broken("값이 없습니다"))? as i64;
                let mut payload_start = node.end as i64;
                let mut any = false;
                for f in &node.fields {
                    if p.doc.keys.payload[f.key as usize] {
                        payload_start = if any { payload_start.min(f.at as i64) } else { f.at as i64 };
                        any = true;
                    }
                }
                handles = if nc != 0 && payload_start - after >= 20 * nc as i64 {
                    p.floats(after as usize, (nc * 5) as i64)?
                } else {
                    Vec::new()
                };
            } else {
                handles = if nc != 0 { p.floats(vals_at + 4 * n as usize, (nc * 5) as i64)? } else { Vec::new() };
            }
            for f in &node.fields {
                if p.doc.keys.payload[f.key as usize] {
                    payload = match f.value {
                        Value::Node(n) => Some(n),
                        _ => None,
                    };
                }
            }
        } else if self.cls(k) == "Key38" {
            // 3.8: 태그 필드, 값은 8~11·17, 곡선은 정규화된 하나(handles38 이 손잡이로 바꾼다)
            frame = self.f(kn, 2, 0.0);
            curve = curve_of(self.leaf_long(kn, 3, 1)?, 2, 3);
            vals = self.values38(k, ty)?;
            handles = Vec::new();
            let tag = match ty {
                5 => 13,
                6 => 16,
                12 => 14,
                13 => 15,
                _ => -1,
            };
            payload = if tag < 0 { None } else { p.get_node(kn, tag, GK::Node) };
        } else {
            // ~4.2.20: 태그 필드
            frame = self.f(kn, 2, 0.0);
            curve = curve_of(self.leaf_long(kn, 5, 1)?, 2, 3);
            vals = self.leaf_floats(kn, 3)?.unwrap_or_default();
            handles = self.leaf_floats(kn, 4)?.unwrap_or_default();
            let tag = match ty {
                5 => 6,
                6 => 9,
                12 => 7,
                13 => 8,
                _ => -1,
            };
            payload = p.get_node(kn, tag, GK::Node);
        }

        let mut key = ModelKey {
            frame,
            curve: Some(curve.to_string()),
            values: Some(vals),
            handles: Some(handles.chunks_exact(5).map(|h| [h[0], h[1], h[2], h[3]]).collect()),
            ..Default::default()
        };

        match ty {
            5 => {
                key.name = match payload {
                    Some(pl) if self.exported(Some(pl))? => self.att_key(Some(pl))?,
                    _ => None,
                };
            }
            6 => {
                key.dense = match payload {
                    Some(pl) => as_floats(p.leaf(Some(pl))?),
                    None => Some(Vec::new()),
                };
            }
            12 => {
                let data = p.get_node(payload, 1, self.c.event_data);
                key.event = Some(ModelKeyEvent {
                    name: p.get_string(data, 2).map(str::to_string),
                    int: zigzag_dec(self.i(payload, 2, 0)),
                    float: self.f(payload, 3, 0.0),
                    string: p.get_string(payload, 4).map(str::to_string),
                    volume: self.f(payload, 5, 1.0),
                    balance: self.f(payload, 6, 0.0),
                });
            }
            13 => {
                key.offsets = Some(
                    p.items(payload)
                        .into_iter()
                        .map(|e| (self.name_or_empty(p.get_node(e, 0, self.c.slot)), zigzag_dec(self.i(e, 1, 0))))
                        .collect(),
                );
            }
            _ => {}
        }
        Ok(key)
    }
}

/// 메시마다 linked mesh 가 몇 개 달려 있는지 센다. linked mesh 의 linked mesh 는 뿌리 메시에 센다.
fn linked_children(p: &View, mesh_att: GK) -> HashMap<NodeId, usize> {
    let mut out: HashMap<NodeId, usize> = HashMap::new();
    for n in p.doc.of("MeshAtt") {
        let mut root = p.get_node(Some(n), 26, mesh_att);
        let mut steps = 0;
        while let Some(up) = p.get_node(root, 26, mesh_att) {
            root = Some(up);
            steps += 1;
            if steps > 64 {
                break; // 고리: 더 올라가지 않는다
            }
        }
        if let Some(root) = root {
            *out.entry(root).or_insert(0) += 1;
        }
    }
    out
}

/// 가중치 메시의 삼각형을 Spine 이 프로젝트를 열 때 하는 대로 한 번 정렬한다.
///
/// Spine 은 삼각형마다 "세 정점의 가중치 합이 가장 큰 뼈"를 구해, 가중치 목록에서 아래에 있는 뼈의 삼각형부터
/// 그리도록 늘어놓는다(목록 위쪽 뼈의 삼각형이 위에 그려진다). 같은 뼈의 삼각형끼리는 들어온 순서의 반대가 된다.
/// `weights` 는 정점별로 가중치 목록 순서의 가중치다.
fn sort_triangles_once(tri: &mut [i64], weights: &[Vec<f32>]) {
    let count = tri.len() / 3;
    let mut keyed: Vec<(usize, usize)> = Vec::with_capacity(count); // (가장 센 뼈의 목록 위치, 삼각형 번호)
    for (t, corners) in tri.chunks_exact(3).enumerate() {
        let mut best = (0usize, f32::NEG_INFINITY);
        let bones = weights.first().map_or(0, Vec::len);
        for b in 0..bones {
            let mut sum = 0f32;
            for &v in corners {
                match usize::try_from(v).ok().and_then(|v| weights.get(v)).and_then(|w| w.get(b)) {
                    Some(w) => sum += w,
                    None => return, // 잘린 자료: 건드리지 않는다
                }
            }
            if sum > best.1 {
                best = (b, sum);
            }
        }
        keyed.push((best.0, t));
    }
    // 목록 위치가 큰 뼈부터, 같은 뼈 안에서는 뒤에 있던 삼각형부터
    keyed.sort_by(|a, b| b.0.cmp(&a.0).then(b.1.cmp(&a.1)));
    let old = tri.to_vec();
    for (i, &(_, t)) in keyed.iter().enumerate() {
        tri[i * 3..i * 3 + 3].copy_from_slice(&old[t * 3..t * 3 + 3]);
    }
}

/// 3.8 회전 키는 가까운 쪽으로 돌도록 보간한다(차이를 -180~180 으로 접는다). 4.x 는 값을 그대로 보간하므로
/// 앞 키와의 차이를 접어 이어 붙인 값으로 바꾼다(Spine 4.x 가 3.8 프로젝트를 열 때와 같다).
fn unwrap38(keys: &mut [ModelKey]) -> R<()> {
    let oob = || broken("인덱스가 범위를 벗어났습니다");
    for i in 1..keys.len() {
        let prev = *keys[i - 1].values.as_ref().and_then(|v| v.first()).ok_or_else(oob)?;
        let cur = keys[i].values.as_mut().and_then(|v| v.first_mut()).ok_or_else(oob)?;
        let mut delta = *cur - prev;
        delta -= 360.0 * ((delta + 180.0) / 360.0).floor();
        *cur = prev + delta;
    }
    Ok(())
}

fn same_dense(a: &[f64], b: &[f64]) -> bool {
    a.len() == b.len() && a.iter().zip(b).all(|(x, y)| !((x - y).abs() > 0.0001))
}

fn byte01(bits: i64, shift: u32) -> f64 {
    ((bits >> shift) & 0xFF) as f64 / 255.0
}

fn as_floats(leaf: Leaf) -> Option<Vec<f64>> {
    match leaf {
        Leaf::Doubles(v) => Some(v),
        Leaf::Longs(v) => Some(v.into_iter().map(|x| x as f64).collect()),
        Leaf::Boxed(v) => Some(v.into_iter().map(|x| x.unwrap_or(0.0)).collect()),
        _ => None,
    }
}

/// 클래스 이름 뒤 번호로 속성 이름(XfFrom134 → rotate, XfTo140 → rotate).
fn prop_name(cls: &str, prefix: usize, base: i32) -> R<&'static str> {
    let n: i32 = cls
        .get(prefix..)
        .and_then(|s| s.parse().ok())
        .ok_or_else(|| broken(format!("입력 문자열의 형식이 잘못되었습니다: {cls}")))?;
    let i = n - base;
    if !(0..6).contains(&i) {
        return Err(broken("인덱스가 배열 범위를 벗어났습니다"));
    }
    Ok(PROPS[i as usize])
}

fn index_of(nodes: &[Option<NodeId>]) -> HashMap<NodeId, usize> {
    let mut index = HashMap::with_capacity(nodes.len());
    for (i, n) in nodes.iter().enumerate() {
        if let Some(n) = n {
            index.insert(*n, i);
        }
    }
    index
}

fn compare_skin_keys(a: &[(i32, String)], b: &[(i32, String)]) -> Ordering {
    for i in 0..a.len().min(b.len()) {
        if a[i].0 != b[i].0 {
            return a[i].0.cmp(&b[i].0);
        }
        let o = cmp_ordinal(Some(&a[i].1), Some(&b[i].1));
        if o != Ordering::Equal {
            return o;
        }
    }
    a.len().cmp(&b.len())
}

/// 경계 변(삼각형 하나에만 속한 변)의 수 = 테두리 정점 수.
fn hull_count(tri: &[i64]) -> i32 {
    let mut count: HashMap<(i64, i64), i32> = HashMap::new();
    let mut add = |a: i64, b: i64| *count.entry((a.min(b), a.max(b))).or_insert(0) += 1;
    let mut i = 0;
    while (i as i64) < tri.len() as i64 - 2 {
        add(tri[i], tri[i + 1]);
        add(tri[i + 1], tri[i + 2]);
        add(tri[i + 2], tri[i]);
        i += 3;
    }
    count.values().filter(|&&v| v == 1).count() as i32
}

#[cfg(test)]
mod tests {
    use super::sort_triangles_once;

    #[test]
    fn weighted_triangles_follow_the_weights_list() {
        // 정점 0~2 는 목록 첫째 뼈, 3~5 는 둘째 뼈, 6 은 반반
        let mut weights = vec![vec![1.0f32, 0.0]; 3];
        weights.extend(vec![vec![0.0f32, 1.0]; 3]);
        weights.push(vec![0.5, 0.5]);
        let mut tri: Vec<i64> = vec![0, 1, 2, 3, 4, 5, 2, 1, 0, 5, 4, 3];
        sort_triangles_once(&mut tri, &weights);
        // 목록 아래쪽(둘째) 뼈의 삼각형이 먼저, 같은 뼈 안에서는 들어온 순서의 반대
        assert_eq!(tri, [5, 4, 3, 3, 4, 5, 2, 1, 0, 0, 1, 2]);
        // 한 번 더 정렬하면(= linked mesh 하나) 묶음 순서는 그대로, 묶음 안만 다시 뒤집힌다
        sort_triangles_once(&mut tri, &weights);
        assert_eq!(tri, [3, 4, 5, 5, 4, 3, 0, 1, 2, 2, 1, 0]);

        // 합이 같으면 목록에서 앞선 뼈: (6, 6, 6) 은 첫째 뼈의 묶음에 든다
        let mut tri: Vec<i64> = vec![6, 6, 6, 3, 4, 5, 0, 1, 2];
        sort_triangles_once(&mut tri, &weights);
        assert_eq!(tri, [3, 4, 5, 0, 1, 2, 6, 6, 6]);

        // 정점 번호가 범위를 벗어나면 건드리지 않는다
        let mut tri: Vec<i64> = vec![0, 1, 9, 3, 4, 5];
        sort_triangles_once(&mut tri, &weights);
        assert_eq!(tri, [0, 1, 9, 3, 4, 5]);
    }
}
