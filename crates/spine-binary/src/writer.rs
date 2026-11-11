//! 모델 → Spine 바이너리(.skel). 파이썬 `binary_writer.Writer` 포팅.
//!
//! 런타임(spine-runtimes)의 SkeletonBinary 가 읽는 순서 그대로 쓴다. 버전마다 다른 부분은 `major` 로 가른다.
//! 3.8 은 설정 자세 부분이 4.0 과 거의 같고(해시가 문자열, 변형·경로 제약의 mix 개수가 다르다),
//! 애니메이션은 형식이 달라 따로 쓴다([`Writer::animation38`]).

use std::collections::{HashMap, HashSet};

use spine_model::{
    ModelAttachment, ModelConstraint, ModelEvent, ModelKey, ModelSkin, ModelTimeline,
    SkeletonModel, SpineError, SpineResult,
};

use crate::out::Out;
use crate::util::{
    byte255, curve_str, f32r, hds, legacy_properties, nat_cmp, round_half_up, same_key_stepped, vals, UNSET,
};

const INHERIT: [&str; 5] = [
    "normal",
    "onlyTranslation",
    "noRotationOrReflection",
    "noScale",
    "noScaleOrReflection",
];
const BLEND: [&str; 4] = ["normal", "additive", "multiply", "screen"];
const POSITION_MODE: [&str; 2] = ["fixed", "percent"];
const SPACING_MODE: [&str; 4] = ["length", "fixed", "percent", "proportional"];
const ROTATE_MODE: [&str; 3] = ["tangent", "chain", "chainScale"];
const PROPS: [&str; 6] = ["rotate", "x", "y", "scaleX", "scaleY", "shearY"];

const CURVE_LINEAR: i64 = 0;
const CURVE_STEPPED: i64 = 1;
const CURVE_BEZIER: i64 = 2;

fn att_type_code(t: &str) -> Option<i64> {
    Some(match t {
        "region" => 0,
        "boundingbox" => 1,
        "mesh" => 2,
        "linkedmesh" => 3,
        "path" => 4,
        "point" => 5,
        "clipping" => 6,
        _ => return None,
    })
}

fn err(msg: String) -> SpineError {
    SpineError::Format(msg)
}

/// 20바이트를 base64 로 적은 27자(채움 글자 없음) — 3.8 의 해시 문자열 모양.
fn base64_27(bytes: &[u8; 20]) -> String {
    const T: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(28);
    for chunk in bytes.chunks(3) {
        let n = (chunk[0] as u32) << 16 | (*chunk.get(1).unwrap_or(&0) as u32) << 8 | *chunk.get(2).unwrap_or(&0) as u32;
        for i in 0..=chunk.len() {
            out.push(T[(n >> (18 - 6 * i)) as usize & 63] as char);
        }
    }
    out
}

fn s(x: &Option<String>) -> &str {
    x.as_deref().unwrap_or("")
}

/// 이름 → 번호 표. 파이썬 dict 처럼 같은 이름이면 뒤의 것이 이긴다.
fn index_of<'a, I: IntoIterator<Item = &'a str>>(names: I) -> HashMap<String, usize> {
    let mut m = HashMap::new();
    for (i, n) in names.into_iter().enumerate() {
        m.insert(n.to_string(), i);
    }
    m
}

fn lookup(map: &HashMap<String, usize>, name: &str, what: &str) -> SpineResult<usize> {
    map.get(name)
        .copied()
        .ok_or_else(|| err(format!("{what}을(를) 찾을 수 없습니다: {name}")))
}

fn pos_in(list: &[&str], name: &str, what: &str) -> SpineResult<i64> {
    list.iter()
        .position(|x| *x == name)
        .map(|i| i as i64)
        .ok_or_else(|| err(format!("알 수 없는 {what}: {name}")))
}

/// `"ffffffff"` 같은 16진 색 → 정수. 비었으면 흰색.
fn color_int(hex: &Option<String>) -> SpineResult<i64> {
    match hex.as_deref() {
        None | Some("") => Ok(0xFFFF_FFFF),
        Some(h) => i64::from_str_radix(h, 16).map_err(|_| err(format!("잘못된 색: {h}"))),
    }
}

/// 파이썬 dict(삽입 순서 유지)를 흉내 낸 작은 순서 있는 표.
struct Ordered<V> {
    items: Vec<(String, V)>,
}

impl<V> Ordered<V> {
    fn new() -> Self {
        Ordered { items: Vec::new() }
    }

    fn entry(&mut self, key: &str, make: impl FnOnce() -> V) -> &mut V {
        let i = match self.items.iter().position(|(k, _)| k == key) {
            Some(i) => i,
            None => {
                self.items.push((key.to_string(), make()));
                self.items.len() - 1
            }
        };
        &mut self.items[i].1
    }

    fn set(&mut self, key: &str, v: V) {
        match self.items.iter().position(|(k, _)| k == key) {
            Some(i) => self.items[i].1 = v,
            None => self.items.push((key.to_string(), v)),
        }
    }

    fn len(&self) -> usize {
        self.items.len()
    }
}

type Curve = (i64, Vec<[f64; 4]>);

pub struct Writer<'a> {
    m: &'a SkeletonModel,
    version: &'a str,
    major: (i32, i32),
    vtuple: Vec<i32>,
    nonessential: bool,
    fps: f64,
    o: Out,
    bone_index: HashMap<String, usize>,
    slot_index: HashMap<String, usize>,
    cons: Vec<&'a ModelConstraint>,
    iks: Vec<&'a ModelConstraint>,
    trs: Vec<&'a ModelConstraint>,
    paths: Vec<&'a ModelConstraint>,
    phys: Vec<&'a ModelConstraint>,
    ik_index: HashMap<String, usize>,
    tr_index: HashMap<String, usize>,
    path_index: HashMap<String, usize>,
    phys_index: HashMap<String, usize>,
    con_index: HashMap<String, usize>,
    event_index: HashMap<String, usize>,
    skins: Vec<&'a ModelSkin>,
    strings: Vec<String>,
    /// 문자열 → 마지막 자리 + 1 (ref 가 쓰는 값).
    string_ref: HashMap<String, usize>,
}

/// "4.2.43" → (주 버전, 전체 튜플).
pub fn parse_version(version: &str) -> SpineResult<((i32, i32), Vec<i32>)> {
    let parts: Vec<i32> = version
        .split('.')
        .take(3)
        .map(|p| p.trim().parse::<i32>())
        .collect::<Result<_, _>>()
        .map_err(|_| SpineError::Usage(format!("잘못된 버전: {version}")))?;
    if parts.len() < 2 {
        return Err(SpineError::Usage(format!("잘못된 버전: {version}")));
    }
    Ok(((parts[0], parts[1]), parts))
}

fn sorted_natural<'a>(cons: &[&'a ModelConstraint], ty: &str) -> Vec<&'a ModelConstraint> {
    let mut v: Vec<&ModelConstraint> = cons
        .iter()
        .copied()
        .filter(|c| c.type_str() == ty)
        .collect();
    v.sort_by(|a, b| nat_cmp(s(&a.name), s(&b.name)));
    v
}

/// 모델에 문자열 표가 없을 때(옛 모델) 첨부 이름·키·경로로 만든 대체 표. 런타임은 읽을 수 있지만 편집기와 바이트가 같지는 않다.
fn fallback_strings(m: &SkeletonModel) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let mut seen = HashSet::new();
    let mut push = |x: &str, out: &mut Vec<String>| {
        if seen.insert(x.to_string()) {
            out.push(x.to_string());
        }
    };
    for sk in &m.skins {
        for e in &sk.attachments {
            if let Some(k) = &e.key {
                push(k, &mut out);
            }
            if let Some(a) = &e.attachment {
                if let Some(n) = &a.name {
                    push(n, &mut out);
                }
                if let Some(p) = &a.path {
                    if !p.is_empty() {
                        push(p, &mut out);
                    }
                }
                if let Some(p) = &a.parent {
                    push(p, &mut out);
                }
            }
        }
    }
    for sl in &m.slots {
        if let Some(a) = &sl.attachment {
            push(a, &mut out);
        }
    }
    for an in &m.animations {
        for t in &an.timelines {
            if let Some(a) = &t.attachment {
                push(a, &mut out);
            }
            for k in &t.keys {
                if let Some(n) = &k.name {
                    push(n, &mut out);
                }
            }
        }
    }
    out
}

impl<'a> Writer<'a> {
    pub fn new(m: &'a SkeletonModel, version: &'a str, nonessential: bool) -> SpineResult<Self> {
        let (major, vtuple) = parse_version(version)?;
        let cons: Vec<&ModelConstraint> = m.constraints.iter().collect();
        let iks = sorted_natural(&cons, "ik");
        let trs = sorted_natural(&cons, "transform");
        let paths = sorted_natural(&cons, "path");
        let phys = sorted_natural(&cons, "physics");
        let skins: Vec<&ModelSkin> = m
            .skins
            .iter()
            .filter(|x| !x.attachments.is_empty() || s(&x.name) != "default")
            .collect();
        let mut strings = if m.strings.is_empty() {
            fallback_strings(m)
        } else {
            m.strings.clone()
        };
        if major < (4, 2) {
            // 4.0·4.1: 스킨·이벤트 이름도 문자열 표 끝에(참조로 쓴다)
            strings.extend(
                skins
                    .iter()
                    .filter(|x| s(&x.name) != "default")
                    .map(|x| s(&x.name).to_string()),
            );
            strings.extend(m.events.iter().map(|e| s(&e.name).to_string()));
        }
        let mut string_ref = HashMap::new();
        for (i, x) in strings.iter().enumerate() {
            string_ref.insert(x.clone(), i + 1);
        }
        Ok(Writer {
            m,
            version,
            major,
            vtuple,
            nonessential,
            fps: m.fps,
            o: Out::default(),
            bone_index: index_of(m.bones.iter().map(|b| s(&b.name))),
            slot_index: index_of(m.slots.iter().map(|x| s(&x.name))),
            ik_index: index_of(iks.iter().map(|c| s(&c.name))),
            tr_index: index_of(trs.iter().map(|c| s(&c.name))),
            path_index: index_of(paths.iter().map(|c| s(&c.name))),
            phys_index: index_of(phys.iter().map(|c| s(&c.name))),
            con_index: HashMap::new(),
            event_index: index_of(m.events.iter().map(|e| s(&e.name))),
            cons,
            iks,
            trs,
            paths,
            phys,
            skins,
            strings,
            string_ref,
        })
    }

    fn v42(&self) -> bool {
        self.major >= (4, 2)
    }

    fn v38(&self) -> bool {
        self.major < (4, 0)
    }

    fn v43(&self) -> bool {
        self.major >= (4, 3)
    }

    // ------------------------------------------------------------ 시간·문자열

    fn sec(&self, frame: f64) -> f64 {
        if self.v38() {
            // 3.8: 프레임 ÷ fps 를 배정밀도로 계산해 float 로
            f32r(frame / self.fps)
        } else if self.v43() {
            f32r(f32r(frame) / f32r(self.fps))
        } else {
            f32r(f32r(frame) * f32r(1.0 / self.fps))
        }
    }

    /// 문자열 표 참조. 같은 문자열이 여러 번 있으면 편집기는 마지막 것을 가리킨다.
    fn rf(&mut self, x: Option<&str>) -> SpineResult<()> {
        match x {
            None => self.o.varint(0),
            Some(x) => {
                let i = *self
                    .string_ref
                    .get(x)
                    .ok_or_else(|| err(format!("문자열 표에 없는 이름: {x}")))?;
                self.o.varint(i as i64);
            }
        }
        Ok(())
    }

    fn bone(&self, name: &str) -> SpineResult<i64> {
        lookup(&self.bone_index, name, "뼈").map(|i| i as i64)
    }

    fn slot(&self, name: &str) -> SpineResult<i64> {
        lookup(&self.slot_index, name, "슬롯").map(|i| i as i64)
    }

    fn con(&self, name: &str) -> SpineResult<i64> {
        lookup(&self.con_index, name, "제약").map(|i| i as i64)
    }

    fn bone_list(&mut self, bones: &[String]) -> SpineResult<()> {
        self.o.varint(bones.len() as i64);
        for b in bones {
            let i = self.bone(b)?;
            self.o.varint(i);
        }
        Ok(())
    }

    // ------------------------------------------------------------ 전체

    pub fn build(mut self) -> SpineResult<Vec<u8>> {
        let m = self.m;
        if !self.v38() {
            self.o.b.extend_from_slice(&[0u8; 8]); // 해시: 본문을 다 쓴 뒤 채운다
        }
        self.o.string(Some(self.version));
        let bounds = m.bounds.as_deref().unwrap_or(&[]);
        for i in 0..4 {
            self.o.float(bounds.get(i).copied().unwrap_or(0.0));
        }
        if self.v42() {
            self.o.float(m.reference_scale);
        }
        self.o.boolean(self.nonessential);
        if self.nonessential {
            self.o.float(m.fps);
            self.o.string(Some(s(&m.images)));
            self.o.string(Some(s(&m.audio)));
        }
        self.o.varint(self.strings.len() as i64);
        for i in 0..self.strings.len() {
            let x = std::mem::take(&mut self.strings[i]);
            self.o.string(Some(&x));
            self.strings[i] = x;
        }
        self.bones()?;
        self.slots()?;
        if self.v43() {
            self.constraints43()?;
        } else {
            self.ik()?;
            self.transform()?;
            self.path_constraints()?;
            if self.v42() {
                self.physics_constraints()?;
            }
        }
        self.default_skin()?;
        let skins = self.skins.clone();
        let first_default = skins
            .first()
            .map(|x| s(&x.name) == "default")
            .unwrap_or(false);
        self.o
            .varint(skins.len() as i64 - if first_default { 1 } else { 0 });
        for sk in &skins {
            if s(&sk.name) != "default" {
                self.skin(sk)?;
            }
        }
        self.o.varint(m.events.len() as i64);
        for e in &m.events {
            self.event_data(e)?;
        }
        self.o.varint(m.animations.len() as i64);
        for an in &m.animations {
            self.o.string(an.name.as_deref());
            if self.v38() {
                self.animation38(&an.timelines)?;
                continue;
            }
            self.animation(&an.timelines)?;
            if self.nonessential && self.v43() {
                let c = color_int(&an.color)?;
                self.o.int(c);
            }
        }
        if self.v43() {
            // 슬라이더가 쓰는 애니메이션 번호는 애니메이션을 다 읽은 뒤에 온다
            let mut cons = self.cons.clone();
            cons.sort_by_key(|c| c.order);
            for c in cons {
                if c.type_str() == "slider" {
                    let i = m
                        .animations
                        .iter()
                        .position(|a| a.name == c.animation)
                        .ok_or_else(|| {
                            err(format!(
                                "슬라이더의 애니메이션을 찾을 수 없습니다: {}",
                                s(&c.animation)
                            ))
                        })?;
                    self.o.varint(i as i64);
                }
            }
        }
        if self.v38() {
            // 3.8: 해시는 맨 앞의 문자열(base64 27자). 편집기의 계산법은 알 수 없어 본문의 SHA-1 을 쓴다(런타임은 비교에만 쓴다).
            let digest = sha1_smol::Sha1::from(&self.o.b).digest().bytes();
            let mut head = Out::default();
            head.string(Some(&base64_27(&digest)));
            head.b.extend_from_slice(&self.o.b);
            return Ok(head.b);
        }
        let digest = sha1_smol::Sha1::from(&self.o.b[8..]).digest().bytes();
        self.o.b[..8].copy_from_slice(&digest[..8]);
        Ok(self.o.b)
    }

    // ------------------------------------------------------------ 설정 자세

    fn bones(&mut self) -> SpineResult<()> {
        let m = self.m;
        self.o.varint(m.bones.len() as i64);
        for (i, b) in m.bones.iter().enumerate() {
            self.o.string(b.name.as_deref());
            if i > 0 {
                let p = self.bone(s(&b.parent))?;
                self.o.varint(p);
            }
            let inherit = pos_in(
                &INHERIT,
                b.inherit.as_deref().unwrap_or("normal"),
                "상속 방식",
            )?;
            if self.v43() {
                // 4.3: 길이가 상속 방식 뒤로
                for v in [
                    b.rotation, b.x, b.y, b.scale_x, b.scale_y, b.shear_x, b.shear_y,
                ] {
                    self.o.float(v);
                }
                self.o.byte(inherit);
                self.o.float(b.length);
            } else {
                for v in [
                    b.rotation, b.x, b.y, b.scale_x, b.scale_y, b.shear_x, b.shear_y, b.length,
                ] {
                    self.o.float(v);
                }
                self.o.byte(inherit);
            }
            self.o.boolean(b.skin);
            if self.nonessential {
                let c = color_int(&b.color)?;
                self.o.int(c);
                if self.v42() {
                    let icon = b.icon.as_deref().filter(|x| *x != "bone");
                    self.o.string(icon);
                    if self.v43() {
                        self.o.float(b.editor33);
                        self.o.float(b.editor34);
                    }
                    self.o.boolean(b.visible);
                }
            }
        }
        Ok(())
    }

    fn slots(&mut self) -> SpineResult<()> {
        let m = self.m;
        self.o.varint(m.slots.len() as i64);
        for sl in &m.slots {
            self.o.string(sl.name.as_deref());
            let b = self.bone(s(&sl.bone))?;
            self.o.varint(b);
            let c = color_int(&sl.color)?;
            self.o.int(c);
            let d = color_int(&sl.dark)?;
            self.o.int(d);
            self.rf(sl.attachment.as_deref())?;
            let blend = pos_in(&BLEND, sl.blend.as_deref().unwrap_or("normal"), "블렌드")?;
            self.o.varint(blend);
            if self.nonessential && self.v42() {
                self.o.boolean(sl.visible);
            }
        }
        Ok(())
    }

    fn ik(&mut self) -> SpineResult<()> {
        let iks = self.iks.clone();
        self.o.varint(iks.len() as i64);
        for c in iks {
            self.o.string(c.name.as_deref());
            self.o.varint(c.order as i64);
            if !self.v42() {
                // 4.0·4.1: 플래그 묶음 없이 차례로
                self.o.boolean(c.skin);
                self.bone_list(&c.bones)?;
                let t = self.bone(s(&c.target))?;
                self.o.varint(t);
                self.o.float(c.mix);
                self.o.float(c.softness);
                self.o.byte(if c.bend_positive { 1 } else { 0xFF });
                self.o.boolean(c.compress);
                self.o.boolean(c.stretch);
                self.o.boolean(c.uniform);
                continue;
            }
            self.bone_list(&c.bones)?;
            let t = self.bone(s(&c.target))?;
            self.o.varint(t);
            let mut flags = (c.skin as i64)
                | (if c.bend_positive { 2 } else { 0 })
                | (if c.compress { 4 } else { 0 })
                | (if c.stretch { 8 } else { 0 })
                | (if c.uniform { 16 } else { 0 });
            if c.mix != 0.0 {
                flags |= 32;
            }
            if c.mix != 1.0 {
                flags |= 64;
            }
            if c.softness != 0.0 {
                flags |= 128;
            }
            self.o.byte(flags);
            if flags & 96 == 96 {
                self.o.float(c.mix);
            }
            if flags & 128 != 0 {
                self.o.float(c.softness);
            }
        }
        Ok(())
    }

    fn transform(&mut self) -> SpineResult<()> {
        let trs = self.trs.clone();
        self.o.varint(trs.len() as i64);
        for c in trs {
            self.o.string(c.name.as_deref());
            self.o.varint(c.order as i64);
            if !self.v42() {
                self.o.boolean(c.skin);
                self.bone_list(&c.bones)?;
                let t = self.bone(s(&c.target))?;
                self.o.varint(t);
                self.o.boolean(c.local);
                self.o.boolean(c.relative);
                for v in [c.rotation, c.x, c.y, c.scale_x, c.scale_y, c.shear_y] {
                    self.o.float(v);
                }
                if self.v38() {
                    // 3.8: 회전 · 이동 · 크기 · 기울임 mix 넷(축마다 따로가 아니다)
                    for v in [c.mixes[0], c.mixes[1], c.mixes[3], c.mixes[5]] {
                        self.o.float(v);
                    }
                    continue;
                }
                for v in c.mixes {
                    self.o.float(v);
                }
                continue;
            }
            self.bone_list(&c.bones)?;
            let t = self.bone(s(&c.target))?;
            self.o.varint(t);
            let mut f1 =
                (c.skin as i64) | (if c.local { 2 } else { 0 }) | (if c.relative { 4 } else { 0 });
            let mut vals1 = Vec::new();
            for (bit, v) in [
                (8, c.rotation),
                (16, c.x),
                (32, c.y),
                (64, c.scale_x),
                (128, c.scale_y),
            ] {
                if v != 0.0 {
                    f1 |= bit;
                    vals1.push(v);
                }
            }
            self.o.byte(f1);
            for v in vals1 {
                self.o.float(v);
            }
            let (mut f2, mut vals2) = (0i64, Vec::new());
            let items = [
                (1, c.shear_y),
                (2, c.mixes[0]),
                (4, c.mixes[1]),
                (8, c.mixes[2]),
                (16, c.mixes[3]),
                (32, c.mixes[4]),
                (64, c.mixes[5]),
            ];
            for (bit, v) in items {
                if v != 0.0 {
                    f2 |= bit;
                    vals2.push(v);
                }
            }
            self.o.byte(f2);
            for v in vals2 {
                self.o.float(v);
            }
        }
        Ok(())
    }

    /// 경로 제약 본문(뼈 목록 뒤부터).
    fn path_body(&mut self, c: &ModelConstraint) -> SpineResult<()> {
        let pos = pos_in(
            &POSITION_MODE,
            c.position_mode.as_deref().unwrap_or("percent"),
            "위치 방식",
        )?;
        let spc = pos_in(
            &SPACING_MODE,
            c.spacing_mode.as_deref().unwrap_or("length"),
            "간격 방식",
        )?;
        let rot = pos_in(
            &ROTATE_MODE,
            c.rotate_mode.as_deref().unwrap_or("tangent"),
            "회전 방식",
        )?;
        if !self.v42() {
            self.o.varint(pos);
            self.o.varint(spc);
            self.o.varint(rot);
            self.o.float(c.rotation);
        } else {
            let mut flags = if self.v43() {
                (c.skin as i64) | (pos << 1) | (spc << 2) | (rot << 4)
            } else {
                pos | (spc << 1) | (rot << 3)
            };
            if c.rotation != 0.0 {
                flags |= 128;
            }
            self.o.byte(flags);
            if flags & 128 != 0 {
                self.o.float(c.rotation);
            }
        }
        if self.v38() {
            // 3.8: 회전 · 이동 mix 둘
            for v in [c.position, c.spacing, c.mixes[0], c.mixes[1]] {
                self.o.float(v);
            }
            return Ok(());
        }
        for v in [c.position, c.spacing, c.mixes[0], c.mixes[1], c.mixes[2]] {
            self.o.float(v);
        }
        Ok(())
    }

    fn path_constraints(&mut self) -> SpineResult<()> {
        let paths = self.paths.clone();
        self.o.varint(paths.len() as i64);
        for c in paths {
            self.o.string(c.name.as_deref());
            self.o.varint(c.order as i64);
            self.o.boolean(c.skin);
            self.bone_list(&c.bones)?;
            let t = self.slot(s(&c.target))?;
            self.o.varint(t);
            self.path_body(c)?;
        }
        Ok(())
    }

    /// 물리 제약 본문(뼈 번호 뒤부터).
    fn physics_body(&mut self, c: &ModelConstraint) {
        let mut flags = c.skin as i64;
        let mut vs = Vec::new();
        for (bit, v) in [
            (2, c.x),
            (4, c.y),
            (8, c.rotate),
            (16, c.scale_x),
            (32, c.shear_x),
        ] {
            if v != 0.0 {
                flags |= bit;
                vs.push(v);
            }
        }
        if c.limit != 5000.0 {
            flags |= 64;
            vs.push(c.limit);
        }
        if c.mass != 1.0 {
            flags |= 128;
        }
        self.o.byte(flags);
        for v in vs {
            self.o.float(v);
        }
        self.o.byte(c.fps.round_ties_even() as i64);
        for v in [c.inertia, c.strength, c.damping] {
            self.o.float(v);
        }
        if flags & 128 != 0 {
            self.o.float(1.0 / c.mass);
        }
        self.o.float(c.wind);
        self.o.float(c.gravity);
        let mut f2 = 0i64;
        let globals = [
            c.inertia_global,
            c.strength_global,
            c.damping_global,
            c.mass_global,
            c.wind_global,
            c.gravity_global,
            c.mix_global,
        ];
        for (i, g) in globals.iter().enumerate() {
            if *g {
                f2 |= 1 << i;
            }
        }
        if c.mix != 1.0 {
            f2 |= 128;
        }
        self.o.byte(f2);
        if f2 & 128 != 0 {
            self.o.float(c.mix);
        }
    }

    fn physics_constraints(&mut self) -> SpineResult<()> {
        let phys = self.phys.clone();
        self.o.varint(phys.len() as i64);
        for c in phys {
            self.o.string(c.name.as_deref());
            self.o.varint(c.order as i64);
            let b = self.bone(s(&c.bone))?;
            self.o.varint(b);
            self.physics_body(c);
        }
        Ok(())
    }

    fn slider_body(&mut self, c: &ModelConstraint) -> SpineResult<()> {
        let mut flags = (c.skin as i64) | (if c.r#loop { 2 } else { 0 });
        if self.nonessential && c.max != 0.0 {
            // 비필수 데이터가 없으면 max 는 쓰지 않는다
            flags |= 8;
        }
        if c.mix != 0.0 {
            flags |= 16;
        }
        if c.mix != 1.0 {
            flags |= 32;
        }
        if c.bone.is_some() {
            flags |= 64;
            if c.local {
                flags |= 128;
            }
        }
        self.o.byte(flags);
        if flags & 8 != 0 {
            self.o.float(c.max);
        }
        if flags & 16 != 0 && flags & 32 != 0 {
            self.o.float(c.mix);
        }
        if flags & 64 != 0 {
            let b = self.bone(s(&c.bone))?;
            self.o.varint(b);
            self.o.float(c.property_offset);
            let p = pos_in(&PROPS, s(&c.property), "속성")?;
            self.o.byte(p);
            self.o.float(c.offset);
            self.o.float(c.scale);
        }
        Ok(())
    }

    /// 4.3: 제약을 적용 순서대로 한 목록에. 이름, 종류, 종류별 본문.
    fn constraints43(&mut self) -> SpineResult<()> {
        let mut cons = self.cons.clone();
        cons.sort_by_key(|c| c.order);
        self.con_index = index_of(cons.iter().map(|c| s(&c.name)));
        self.o.varint(cons.len() as i64);
        for c in cons {
            self.o.string(c.name.as_deref());
            match c.type_str() {
                "path" => {
                    self.o.byte(1);
                    self.bone_list(&c.bones)?;
                    let t = self.slot(s(&c.target))?;
                    self.o.varint(t);
                    self.path_body(c)?;
                }
                "physics" => {
                    self.o.byte(3);
                    let b = self.bone(s(&c.bone))?;
                    self.o.varint(b);
                    self.physics_body(c);
                }
                "slider" => {
                    self.o.byte(4);
                    self.slider_body(c)?;
                }
                "ik" => {
                    self.o.byte(0);
                    self.bone_list(&c.bones)?;
                    let t = self.bone(s(&c.target))?;
                    self.o.varint(t);
                    let two = c.bones.len() == 2;
                    // 4.3: 1 skin, 2 uniform, 4 굽힘 음수(두 뼈), 8 compress, 16 stretch (표본 실측 + 추정)
                    let mut flags = (c.skin as i64)
                        | (if c.uniform && !two { 2 } else { 0 })
                        | (if !c.bend_positive { 4 } else { 0 })
                        | (if c.compress && !two { 8 } else { 0 })
                        | (if c.stretch { 16 } else { 0 });
                    if c.mix != 0.0 {
                        flags |= 32;
                    }
                    if c.mix != 1.0 {
                        flags |= 64;
                    }
                    if c.softness != 0.0 {
                        flags |= 128;
                    }
                    self.o.byte(flags);
                    if flags & 96 == 96 {
                        self.o.float(c.mix);
                    }
                    if flags & 128 != 0 {
                        self.o.float(c.softness);
                    }
                }
                _ => {
                    // 변형 제약
                    self.o.byte(2);
                    self.bone_list(&c.bones)?;
                    let t = self.bone(s(&c.target))?;
                    self.o.varint(t);
                    let legacy;
                    let props: &[spine_model::ModelFromProperty] = match &c.properties {
                        Some(p) => p,
                        None => {
                            legacy = legacy_properties(c);
                            &legacy
                        }
                    };
                    let flags = (c.skin as i64)
                        | (if c.local { 2 } else { 0 })
                        | (if c.local { 4 } else { 0 })
                        | (if c.relative { 8 } else { 0 })
                        | ((props.len() as i64) << 5);
                    self.o.byte(flags);
                    let mut targets: HashSet<&str> = HashSet::new();
                    for p in props {
                        let i = pos_in(&PROPS, s(&p.name), "속성")?;
                        self.o.byte(i);
                        self.o.float(p.offset);
                        self.o.varint(p.to.len() as i64);
                        for t in &p.to {
                            let j = pos_in(&PROPS, s(&t.name), "속성")?;
                            self.o.byte(j);
                            self.o.float(t.offset);
                            self.o.float(t.max);
                            self.o.float(t.scale);
                            targets.insert(s(&t.name));
                        }
                    }
                    let offsets = [c.rotation, c.x, c.y, c.scale_x, c.scale_y, c.shear_y];
                    for arr in [offsets, c.mixes] {
                        // 대상 속성이 없는 값은 쓰지 않는다
                        let (mut bits, mut vs) = (0i64, Vec::new());
                        for (i, v) in arr.iter().enumerate() {
                            if targets.contains(PROPS[i]) && *v != 0.0 {
                                bits |= 1 << i;
                                vs.push(*v);
                            }
                        }
                        self.o.byte(bits);
                        for v in vs {
                            self.o.float(v);
                        }
                    }
                }
            }
        }
        Ok(())
    }

    // ------------------------------------------------------------ 스킨

    fn default_skin(&mut self) -> SpineResult<()> {
        match self.skins.iter().copied().find(|x| s(&x.name) == "default") {
            None => {
                self.o.varint(0);
                Ok(())
            }
            Some(d) => self.skin_slots(d),
        }
    }

    fn skin(&mut self, sk: &ModelSkin) -> SpineResult<()> {
        if !self.v42() {
            self.rf(sk.name.as_deref())?;
        } else {
            self.o.string(sk.name.as_deref());
        }
        if self.nonessential && self.v42() {
            let c = color_int(&sk.color)?;
            self.o.int(c);
        }
        self.bone_list(&sk.bones)?;
        if self.v43() {
            // 4.3: 제약은 통합 목록의 번호 하나로
            self.o.varint(sk.constraints.len() as i64);
            for n in &sk.constraints {
                let i = self.con(n)?;
                self.o.varint(i);
            }
        } else {
            let mut idxs = vec![&self.ik_index, &self.tr_index, &self.path_index];
            if self.v42() {
                idxs.push(&self.phys_index);
            }
            let mut out = Vec::new();
            for idx in idxs {
                let names: Vec<usize> = sk
                    .constraints
                    .iter()
                    .filter_map(|n| idx.get(n).copied())
                    .collect();
                out.push(names);
            }
            for names in out {
                self.o.varint(names.len() as i64);
                for i in names {
                    self.o.varint(i as i64);
                }
            }
        }
        self.skin_slots(sk)
    }

    fn skin_slots(&mut self, sk: &ModelSkin) -> SpineResult<()> {
        let empty = ModelAttachment::default();
        let mut by_slot: Ordered<Vec<(&str, &ModelAttachment)>> = Ordered::new();
        for e in &sk.attachments {
            by_slot
                .entry(s(&e.slot), Vec::new)
                .push((s(&e.key), e.attachment.as_ref().unwrap_or(&empty)));
        }
        self.o.varint(by_slot.len() as i64);
        let mut slots = by_slot.items;
        // 편집기는 슬롯 이름순, 그 안은 첨부 이름순
        slots.sort_by(|a, b| nat_cmp(&a.0, &b.0));
        for (slot, mut atts) in slots {
            let i = self.slot(&slot)?;
            self.o.varint(i);
            atts.sort_by(|a, b| nat_cmp(a.0, b.0));
            self.o.varint(atts.len() as i64);
            for (key, a) in atts {
                self.rf(Some(key))?;
                self.attachment(key, a)?;
            }
        }
        Ok(())
    }

    /// 4.0·4.1: 이름 참조(같으면 null) → 종류 바이트 → 종류별 필드(플래그 없음).
    fn attachment_old(&mut self, key: &str, a: &ModelAttachment) -> SpineResult<()> {
        let t = a.type_str();
        let code = att_type_code(t).ok_or_else(|| err(format!("알 수 없는 첨부 종류: {t}")))?;
        let name = a.name.as_deref().filter(|n| *n != key);
        self.rf(name)?;
        self.o.byte(code);
        let path = a.path.as_deref().filter(|p| !p.is_empty());
        match t {
            "region" => {
                self.rf(path)?;
                for v in [
                    a.rotation, a.x, a.y, a.scale_x, a.scale_y, a.width, a.height,
                ] {
                    self.o.float(v);
                }
                let c = color_int(&a.color)?;
                self.o.int(c);
                self.sequence_old(a);
            }
            "boundingbox" | "clipping" => {
                if t == "clipping" {
                    let e = self.slot(s(&a.end))?;
                    self.o.varint(e);
                }
                self.o.varint(a.vertex_count as i64);
                self.vertices_old(a)?;
                if self.nonessential {
                    let c = color_int(&a.color)?;
                    self.o.int(c);
                }
            }
            "mesh" => {
                self.rf(path)?;
                let c = color_int(&a.color)?;
                self.o.int(c);
                self.o.varint(a.vertex_count as i64);
                for v in a.uvs.as_deref().unwrap_or(&[]) {
                    self.o.float(*v);
                }
                let tri = a.triangles.as_deref().unwrap_or(&[]);
                self.o.varint(tri.len() as i64);
                for v in tri {
                    self.o.short(*v);
                }
                self.vertices_old(a)?;
                self.o.varint(a.hull as i64);
                self.sequence_old(a);
                if self.nonessential {
                    let edges = a.edges.as_deref().unwrap_or(&[]);
                    self.o.varint(edges.len() as i64);
                    for v in edges {
                        self.o.short(*v);
                    }
                    self.o.float(a.width);
                    self.o.float(a.height);
                }
            }
            "linkedmesh" => {
                self.rf(path)?;
                let c = color_int(&a.color)?;
                self.o.int(c);
                self.rf(a.skin.as_deref())?;
                self.rf(a.parent.as_deref())?;
                self.o.boolean(a.timelines);
                self.sequence_old(a);
                if self.nonessential {
                    self.o.float(a.width);
                    self.o.float(a.height);
                }
            }
            "path" => {
                self.o.boolean(a.closed);
                self.o.boolean(a.constant_speed);
                self.o.varint(a.vertex_count as i64);
                self.vertices_old(a)?;
                for v in a.lengths.as_deref().unwrap_or(&[]) {
                    self.o.float(*v);
                }
                if self.nonessential {
                    let c = color_int(&a.color)?;
                    self.o.int(c);
                }
            }
            "point" => {
                // 파이썬 PoC 에는 없다(런타임 형식: 회전, x, y, 비필수 색)
                self.o.float(a.rotation);
                self.o.float(a.x);
                self.o.float(a.y);
                if self.nonessential {
                    let c = color_int(&a.color)?;
                    self.o.int(c);
                }
            }
            _ => return Err(err(format!("알 수 없는 첨부 종류: {t}"))),
        }
        Ok(())
    }

    /// 4.1: 있음 여부 + 값. 4.0 에는 시퀀스가 없다.
    fn sequence_old(&mut self, a: &ModelAttachment) {
        if self.major >= (4, 1) {
            self.o.boolean(a.sequence.is_some());
            if a.sequence.is_some() {
                self.sequence_fields(a);
            }
        }
    }

    fn sequence_fields(&mut self, a: &ModelAttachment) {
        if let Some(q) = &a.sequence {
            self.o.varint(q.count);
            self.o.varint(q.start);
            self.o.varint(q.digits);
            self.o.varint(q.setup);
        }
    }

    /// 3.8 편집기는 정점 좌표를 소수 둘째 자리로 반올림해 쓴다(4.x 는 그대로).
    fn vertex_xy(&self, v: f64) -> f64 {
        if self.v38() {
            round_half_up(v, 2)
        } else {
            v
        }
    }

    /// 가중치 정점: [뼈 수, (뼈 번호, x, y, 가중치)×뼈 수]×정점.
    fn weighted_body(&mut self, v: &[f64]) {
        let mut i = 0;
        while i < v.len() {
            let n = v[i] as i64;
            self.o.varint(n);
            i += 1;
            for _ in 0..n {
                if i + 3 >= v.len() {
                    // 잘린 배열 — 파이썬은 IndexError. 남은 것만 쓰고 멈춘다
                    return;
                }
                self.o.varint(v[i] as i64);
                self.o.float(self.vertex_xy(v[i + 1]));
                self.o.float(self.vertex_xy(v[i + 2]));
                self.o.float(round_half_up(v[i + 3], 5)); // 가중치는 5자리로 반올림된 값이 저장된다
                i += 4;
            }
        }
    }

    fn vertices_old(&mut self, a: &ModelAttachment) -> SpineResult<()> {
        self.o.boolean(a.weighted);
        let v = a.vertices.as_deref().unwrap_or(&[]);
        if !a.weighted {
            for x in v {
                self.o.float(self.vertex_xy(*x));
            }
            return Ok(());
        }
        self.weighted_body(v);
        Ok(())
    }

    fn attachment(&mut self, key: &str, a: &ModelAttachment) -> SpineResult<()> {
        if !self.v42() {
            return self.attachment_old(key, a);
        }
        let t = a.type_str();
        let mut flags =
            att_type_code(t).ok_or_else(|| err(format!("알 수 없는 첨부 종류: {t}")))?;
        if a.name.as_deref() != Some(key) {
            flags |= 8;
        }
        let has_path = a.path.as_deref().map(|p| !p.is_empty()).unwrap_or(false);
        let not_white = a.color.as_deref() != Some("ffffffff");
        let name = a.name.as_deref();
        match t {
            "region" => {
                if has_path {
                    flags |= 16;
                }
                if not_white {
                    flags |= 32;
                }
                if a.sequence.is_some() {
                    flags |= 64;
                }
                if a.rotation != 0.0 {
                    flags |= 128;
                }
                self.o.byte(flags);
                if flags & 8 != 0 {
                    self.rf(name)?;
                }
                if flags & 16 != 0 {
                    self.rf(a.path.as_deref())?;
                }
                if flags & 32 != 0 {
                    let c = color_int(&a.color)?;
                    self.o.int(c);
                }
                if flags & 64 != 0 {
                    self.sequence_fields(a);
                }
                if flags & 128 != 0 {
                    self.o.float(a.rotation);
                }
                for v in [a.x, a.y, a.scale_x, a.scale_y, a.width, a.height] {
                    self.o.float(v);
                }
            }
            "boundingbox" | "clipping" => {
                if a.weighted {
                    flags |= 16;
                }
                if t == "clipping" && self.v43() && a.convex {
                    flags |= 32; // 4.3: 볼록 클리핑
                }
                self.o.byte(flags);
                if flags & 8 != 0 {
                    self.rf(name)?;
                }
                if t == "clipping" {
                    let e = self.slot(s(&a.end))?;
                    self.o.varint(e);
                }
                self.vertices(a);
                if self.nonessential {
                    let c = color_int(&a.color)?;
                    self.o.int(c);
                }
            }
            "mesh" => {
                if has_path {
                    flags |= 16;
                }
                if not_white {
                    flags |= 32;
                }
                if a.sequence.is_some() {
                    flags |= 64;
                }
                if a.weighted {
                    flags |= 128;
                }
                self.o.byte(flags);
                if flags & 8 != 0 {
                    self.rf(name)?;
                }
                if flags & 16 != 0 {
                    self.rf(a.path.as_deref())?;
                }
                if flags & 32 != 0 {
                    let c = color_int(&a.color)?;
                    self.o.int(c);
                }
                if flags & 64 != 0 {
                    self.sequence_fields(a);
                }
                self.o.varint(a.hull as i64);
                self.vertices(a);
                for v in a.uvs.as_deref().unwrap_or(&[]) {
                    self.o.float(*v);
                }
                for v in a.triangles.as_deref().unwrap_or(&[]) {
                    self.o.varint(*v);
                }
                if self.v43() {
                    self.o.varint(0); // 4.3 에서 생긴 값(뜻 미상, 표본은 모두 0)
                }
                if self.nonessential {
                    let edges = a.edges.as_deref().unwrap_or(&[]);
                    self.o.varint(edges.len() as i64);
                    for v in edges {
                        self.o.varint(*v);
                    }
                    self.o.float(a.width);
                    self.o.float(a.height);
                }
            }
            "linkedmesh" => {
                if has_path {
                    flags |= 16;
                }
                if not_white {
                    flags |= 32;
                }
                if a.sequence.is_some() {
                    flags |= 64;
                }
                if a.timelines {
                    flags |= 128;
                }
                self.o.byte(flags);
                if flags & 8 != 0 {
                    self.rf(name)?;
                }
                if flags & 16 != 0 {
                    self.rf(a.path.as_deref())?;
                }
                if flags & 32 != 0 {
                    let c = color_int(&a.color)?;
                    self.o.int(c);
                }
                if flags & 64 != 0 {
                    self.sequence_fields(a);
                }
                if self.v43() {
                    let ps = self.slot(s(&a.parent_slot))?;
                    self.o.varint(ps);
                }
                let skin = a
                    .skin
                    .as_deref()
                    .filter(|x| !x.is_empty())
                    .unwrap_or("default");
                let si = self
                    .skins
                    .iter()
                    .position(|x| s(&x.name) == skin)
                    .ok_or_else(|| err(format!("연결 메시의 스킨을 찾을 수 없습니다: {skin}")))?;
                self.o.varint(si as i64);
                self.rf(a.parent.as_deref())?;
                if self.nonessential {
                    self.o.float(a.width);
                    self.o.float(a.height);
                }
            }
            "path" => {
                if a.closed {
                    flags |= 16;
                }
                if a.constant_speed {
                    flags |= 32;
                }
                if a.weighted {
                    flags |= 64;
                }
                self.o.byte(flags);
                if flags & 8 != 0 {
                    self.rf(name)?;
                }
                self.vertices(a);
                for v in a.lengths.as_deref().unwrap_or(&[]) {
                    self.o.float(*v);
                }
                if self.nonessential {
                    let c = color_int(&a.color)?;
                    self.o.int(c);
                }
            }
            "point" => {
                // 파이썬 PoC 에는 없다(런타임 형식: 회전, x, y, 비필수 색)
                self.o.byte(flags);
                if flags & 8 != 0 {
                    self.rf(name)?;
                }
                self.o.float(a.rotation);
                self.o.float(a.x);
                self.o.float(a.y);
                if self.nonessential {
                    let c = color_int(&a.color)?;
                    self.o.int(c);
                }
            }
            _ => return Err(err(format!("알 수 없는 첨부 종류: {t}"))),
        }
        Ok(())
    }

    fn vertices(&mut self, a: &ModelAttachment) {
        self.o.varint(a.vertex_count as i64);
        let v = a.vertices.as_deref().unwrap_or(&[]);
        if !a.weighted {
            for x in v {
                self.o.float(*x);
            }
            return;
        }
        if self.v43() {
            // 4.3: 뼈 배열 길이(정점마다 1 + 뼈 수)를 먼저
            let (mut n, mut i) = (0i64, 0usize);
            while i < v.len() {
                let c = v[i] as i64;
                n += 1 + c;
                i += 1 + 4 * c.max(0) as usize;
            }
            self.o.varint(n);
        }
        self.weighted_body(v);
    }

    fn event_data(&mut self, e: &ModelEvent) -> SpineResult<()> {
        if !self.v42() {
            self.rf(e.name.as_deref())?;
        } else {
            self.o.string(e.name.as_deref());
        }
        self.o.svarint(e.int);
        self.o.float(e.float);
        self.o.string(Some(s(&e.string)));
        let audio = e.audio.as_deref().filter(|x| !x.is_empty());
        self.o.string(audio);
        if audio.is_some() {
            self.o.float(e.volume);
            self.o.float(e.balance);
        }
        Ok(())
    }

    // ------------------------------------------------------------ 애니메이션

    fn animation(&mut self, tls: &'a [ModelTimeline]) -> SpineResult<()> {
        let mut slots: Ordered<Vec<&ModelTimeline>> = Ordered::new();
        let mut bones: Ordered<Vec<&ModelTimeline>> = Ordered::new();
        let mut iks: Ordered<&ModelTimeline> = Ordered::new();
        let mut trs: Ordered<&ModelTimeline> = Ordered::new();
        let mut deforms: Ordered<Ordered<Vec<&ModelTimeline>>> = Ordered::new();
        let mut events: Vec<&ModelKey> = Vec::new();
        let mut paths: Ordered<Vec<&ModelTimeline>> = Ordered::new();
        let mut phys: Ordered<Vec<&ModelTimeline>> = Ordered::new();
        let mut sliders: Ordered<Vec<&ModelTimeline>> = Ordered::new();
        let mut draw: &[ModelKey] = &[];
        for t in tls {
            match t.type_str() {
                "color" | "attachment" => slots.entry(s(&t.slot), Vec::new).push(t),
                "rotate" | "translate" | "scale" | "shear" | "translatex" | "translatey" => {
                    bones.entry(s(&t.bone), Vec::new).push(t)
                }
                "ik" => iks.set(s(&t.constraint), t),
                "transform" => trs.set(s(&t.constraint), t),
                "deform" | "sequence" => deforms
                    .entry(s(&t.skin), Ordered::new)
                    .entry(s(&t.slot), Vec::new)
                    .push(t),
                "event" => events.extend(t.keys.iter()),
                "pathPosition" | "pathSpacing" | "pathMix" => {
                    paths.entry(s(&t.constraint), Vec::new).push(t)
                }
                "physicsWind" => {
                    // 빈 이름은 "모든 물리 제약"(physicsWindAll) 자리라 대상이 없으면 오류로 둔다
                    let c = t.constraint.as_deref().filter(|c| !c.is_empty());
                    let c = c.ok_or_else(|| {
                        err("물리 제약 타임라인에 대상 제약이 없습니다".to_string())
                    })?;
                    phys.entry(c, Vec::new).push(t)
                }
                "physicsWindAll" => phys.entry("", Vec::new).push(t),
                "sliderMix" => sliders.entry(s(&t.constraint), Vec::new).push(t),
                "drawOrder" => draw = &t.keys,
                _ => {}
            }
        }
        // 정의별 이벤트 타임라인을 시간순으로 합친다(안정 정렬)
        events.sort_by(|a, b| {
            a.frame
                .partial_cmp(&b.frame)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        let sum =
            |o: &Ordered<Vec<&ModelTimeline>>| o.items.iter().map(|(_, v)| v.len()).sum::<usize>();
        let count = sum(&slots)
            + sum(&bones)
            + iks.len()
            + trs.len()
            + deforms.items.iter().map(|(_, sk)| sum(sk)).sum::<usize>()
            + (!events.is_empty()) as usize
            + (!draw.is_empty()) as usize
            + sum(&paths)
            + sum(&phys)
            + sum(&sliders);
        self.o.varint(count as i64);

        // 슬롯: 이름순
        self.o.varint(slots.len() as i64);
        let mut sl = slots.items;
        sl.sort_by(|a, b| nat_cmp(&a.0, &b.0));
        for (slot, ts) in sl {
            let i = self.slot(&slot)?;
            self.o.varint(i);
            self.o.varint(ts.len() as i64);
            for t in ts {
                self.slot_timeline(t)?;
            }
        }
        // 뼈: 4.2 까지는 프로젝트 순서, 4.3 은 이름순
        self.o.varint(bones.len() as i64);
        let mut bo = bones.items;
        if self.v43() {
            bo.sort_by(|a, b| nat_cmp(&a.0, &b.0));
        }
        for (bone, ts) in bo {
            let i = self.bone(&bone)?;
            self.o.varint(i);
            self.o.varint(ts.len() as i64);
            for t in ts {
                self.bone_timeline(t)?;
            }
        }
        // IK
        self.o.varint(iks.len() as i64);
        let mut ik = Vec::new();
        for (n, t) in iks.items {
            ik.push((lookup(&self.ik_index, &n, "IK 제약")?, t));
        }
        ik.sort_by_key(|x| x.0);
        for (_, t) in ik {
            self.ik_timeline(t)?;
        }
        // 변형
        self.o.varint(trs.len() as i64);
        let mut tr = Vec::new();
        for (n, t) in trs.items {
            tr.push((lookup(&self.tr_index, &n, "변형 제약")?, t));
        }
        tr.sort_by_key(|x| x.0);
        for (_, t) in tr {
            let i = if self.v43() {
                self.con(s(&t.constraint))?
            } else {
                lookup(&self.tr_index, s(&t.constraint), "변형 제약")? as i64
            };
            self.o.varint(i);
            self.o.varint(t.keys.len() as i64);
            let bc = self.bezier_count(&t.keys, 6);
            self.o.varint(bc);
            self.values_timeline(&t.keys, 6);
        }
        // 경로
        let v43 = self.v43();
        self.o.varint(paths.len() as i64);
        let mut pa = Vec::new();
        for (n, ts) in paths.items {
            let i = if v43 {
                self.con(&n)?
            } else {
                lookup(&self.path_index, &n, "경로 제약")? as i64
            };
            pa.push((i, ts));
        }
        pa.sort_by_key(|x| x.0);
        for (i, ts) in pa {
            self.o.varint(i);
            self.o.varint(ts.len() as i64);
            for t in ts {
                let (code, nch) = match t.type_str() {
                    "pathPosition" => (0, 1),
                    "pathSpacing" => (1, 1),
                    _ => (2, 3),
                };
                self.typed_timeline(t, code, nch);
            }
        }
        if self.v42() {
            // 물리 번호는 1부터(0 = 모든 물리 제약)
            self.o.varint(phys.len() as i64);
            let mut ph = Vec::new();
            for (n, ts) in phys.items {
                let i = if n.is_empty() {
                    0
                } else if v43 {
                    self.con(&n)? + 1
                } else {
                    lookup(&self.phys_index, &n, "물리 제약")? as i64 + 1
                };
                ph.push((i, ts));
            }
            ph.sort_by_key(|x| x.0);
            for (i, ts) in ph {
                self.o.varint(i);
                self.o.varint(ts.len() as i64);
                for t in ts {
                    self.typed_timeline(t, 5, 1);
                }
            }
        }
        if v43 {
            self.o.varint(sliders.len() as i64);
            let mut sd = Vec::new();
            for (n, ts) in sliders.items {
                sd.push((self.con(&n)?, ts));
            }
            sd.sort_by_key(|x| x.0);
            for (i, ts) in sd {
                self.o.varint(i);
                self.o.varint(ts.len() as i64);
                for t in ts {
                    self.typed_timeline(t, 1, 1);
                }
            }
        }
        // 첨부(변형·시퀀스)
        self.o.varint(deforms.len() as i64);
        let mut de = Vec::new();
        for (skin, slots) in deforms.items {
            let i = self
                .skins
                .iter()
                .position(|x| s(&x.name) == skin)
                .ok_or_else(|| err(format!("스킨을 찾을 수 없습니다: {skin}")))?;
            de.push((i, slots));
        }
        de.sort_by_key(|x| x.0);
        for (i, slots) in de {
            self.o.varint(i as i64);
            self.o.varint(slots.len() as i64);
            let mut ss = slots.items;
            ss.sort_by(|a, b| nat_cmp(&a.0, &b.0));
            for (slot, ts) in ss {
                let si = self.slot(&slot)?;
                self.o.varint(si);
                self.o.varint(ts.len() as i64);
                for t in ts {
                    if t.type_str() == "sequence" {
                        self.sequence_timeline(t)?;
                    } else {
                        self.deform_timeline(t)?;
                    }
                }
            }
        }
        // 그리기 순서
        self.o.varint(draw.len() as i64);
        for k in draw {
            self.o.float(self.sec(k.frame));
            let mut changed = Vec::new();
            for (slot, off) in k.offsets.as_deref().unwrap_or(&[]) {
                changed.push((self.slot(slot)?, *off));
            }
            changed.sort_by_key(|x| x.0);
            self.o.varint(changed.len() as i64);
            for (i, off) in changed {
                self.o.varint(i);
                self.o.varint(off);
            }
        }
        if v43 {
            self.o.varint(0); // 드로우 오더 폴더 타임라인
        }
        self.o.varint(events.len() as i64);
        for k in events {
            self.event_key(k)?;
        }
        Ok(())
    }

    // ------------------------------------------------------------ 3.8 애니메이션

    /// 3.8 의 k → 다음 키 곡선: 0 직선 · 1 계단 · 2 베지어(정규화된 c1 c2 c3 c4).
    /// 3.8 프로젝트는 키에 적힌 곡선을 그대로, 4.x 프로젝트의 베지어는 Spine 의 3.8 내보내기처럼 직선으로 낸다.
    /// 값이 같은 두 키 사이는 4.x 와 마찬가지로 계단으로 낸다.
    fn curve38(&mut self, keys: &[ModelKey], i: usize, nch: usize) {
        let (k, nxt) = (&keys[i], &keys[i + 1]);
        let cv = curve_str(k);
        let same_dense = match (&k.dense, &nxt.dense) {
            (Some(a), Some(b)) => cv == "linear" && a.len() == b.len() && a.iter().zip(b).all(|(x, y)| (x - y).abs() <= 0.0001),
            _ => false,
        };
        // 3.8 프로젝트의 회전 키는 저장된 각도로 비교한다: -297.81 과 62.19 는 이어 붙이면 같은 값이지만 3.8 에서는 다른 키다
        let turned = matches!((k.angle38, nxt.angle38), (Some(a), Some(b)) if (a - b).abs() > 1e-6);
        if cv == "stepped" || (!turned && same_key_stepped(k, nxt, nch)) || same_dense {
            self.o.byte(CURVE_STEPPED);
        } else if let Some(c) = k.curve38.filter(|c| *c != [0.0, 0.0, 1.0, 1.0]) {
            // (0, 0, 1, 1) 은 직선과 같아 편집기도 직선으로 쓴다
            self.o.byte(CURVE_BEZIER);
            for v in c {
                self.o.float(v);
            }
        } else {
            self.o.byte(CURVE_LINEAR);
        }
    }

    /// 키마다 `시간, 값들` 을 쓰고 마지막 키가 아니면 곡선을 붙인다(3.8 의 공통 모양). nch 는 값 채널 수.
    fn keys38(&mut self, keys: &[ModelKey], nch: usize, mut values: impl FnMut(&mut Self, &ModelKey) -> SpineResult<()>) -> SpineResult<()> {
        for (i, k) in keys.iter().enumerate() {
            self.o.float(self.sec(k.frame));
            values(self, k)?;
            if i + 1 < keys.len() {
                self.curve38(keys, i, nch);
            }
        }
        Ok(())
    }

    fn animation38(&mut self, tls: &'a [ModelTimeline]) -> SpineResult<()> {
        let mut slots: Ordered<Vec<&ModelTimeline>> = Ordered::new();
        let mut bones: Ordered<Vec<&ModelTimeline>> = Ordered::new();
        let mut iks: Ordered<&ModelTimeline> = Ordered::new();
        let mut trs: Ordered<&ModelTimeline> = Ordered::new();
        let mut paths: Ordered<Vec<&ModelTimeline>> = Ordered::new();
        let mut deforms: Ordered<Ordered<Vec<&ModelTimeline>>> = Ordered::new();
        let mut events: Vec<&ModelKey> = Vec::new();
        let mut draw: &[ModelKey] = &[];
        for t in tls {
            match t.type_str() {
                "color" | "attachment" => slots.entry(s(&t.slot), Vec::new).push(t),
                "rotate" | "translate" | "scale" | "shear" => bones.entry(s(&t.bone), Vec::new).push(t),
                "ik" => iks.set(s(&t.constraint), t),
                "transform" => trs.set(s(&t.constraint), t),
                "pathPosition" | "pathSpacing" | "pathMix" => paths.entry(s(&t.constraint), Vec::new).push(t),
                "deform" => deforms.entry(s(&t.skin), Ordered::new).entry(s(&t.slot), Vec::new).push(t),
                "event" => events.extend(t.keys.iter()),
                "drawOrder" => draw = &t.keys,
                other => return Err(SpineError::Unsupported(format!("3.8 바이너리에 쓸 수 없는 타임라인: {other}"))),
            }
        }
        events.sort_by(|a, b| a.frame.partial_cmp(&b.frame).unwrap_or(std::cmp::Ordering::Equal));

        // 슬롯
        self.o.varint(slots.len() as i64);
        let mut sl = slots.items;
        sl.sort_by(|a, b| nat_cmp(&a.0, &b.0));
        for (slot, ts) in sl {
            let i = self.slot(&slot)?;
            self.o.varint(i);
            self.o.varint(ts.len() as i64);
            let two = self.m.slots.iter().find(|x| s(&x.name) == slot).is_some_and(|x| x.dark.is_some());
            for t in ts {
                if t.type_str() == "attachment" {
                    self.o.byte(0);
                    self.o.varint(t.keys.len() as i64);
                    for k in &t.keys {
                        self.o.float(self.sec(k.frame));
                        self.rf(k.name.as_deref())?;
                    }
                    continue;
                }
                self.o.byte(if two { 2 } else { 1 });
                self.o.varint(t.keys.len() as i64);
                self.keys38(&t.keys, if two { 7 } else { 4 }, |w, k| {
                    let v = vals(k);
                    let b = |c: usize| byte255(v.get(c).copied().unwrap_or(0.0)) & 0xFF;
                    w.o.int((b(0) << 24) | (b(1) << 16) | (b(2) << 8) | b(3));
                    if two {
                        w.o.int((b(4) << 16) | (b(5) << 8) | b(6)); // 어두운 색은 RGB 만(위 바이트 0)
                    }
                    Ok(())
                })?;
            }
        }
        // 뼈
        self.o.varint(bones.len() as i64);
        for (bone, ts) in bones.items {
            let i = self.bone(&bone)?;
            self.o.varint(i);
            self.o.varint(ts.len() as i64);
            for t in ts {
                let rotate = t.type_str() == "rotate";
                self.o.byte(match t.type_str() {
                    "rotate" => 0,
                    "translate" => 1,
                    "scale" => 2,
                    _ => 3,
                });
                self.o.varint(t.keys.len() as i64);
                self.keys38(&t.keys, if rotate { 1 } else { 2 }, |w, k| {
                    let v = vals(k);
                    if rotate {
                        w.o.float(k.angle38.unwrap_or_else(|| v.first().copied().unwrap_or(0.0)));
                    } else {
                        w.o.float(v.first().copied().unwrap_or(0.0));
                        w.o.float(v.get(1).copied().unwrap_or(0.0));
                    }
                    Ok(())
                })?;
            }
        }
        // IK: 시간, mix, softness, 굽힘 방향, compress, stretch
        self.o.varint(iks.len() as i64);
        let mut ik = Vec::new();
        for (n, t) in iks.items {
            ik.push((lookup(&self.ik_index, &n, "IK 제약")?, t));
        }
        ik.sort_by_key(|x| x.0);
        for (i, t) in ik {
            self.o.varint(i as i64);
            self.o.varint(t.keys.len() as i64);
            self.keys38(&t.keys, 2, |w, k| {
                let v = vals(k);
                let g = |i: usize| v.get(i).copied().unwrap_or(0.0);
                let (mix, soft, stretch, compress, bend) = (g(0), g(1), g(2), g(3), g(4));
                w.o.float(mix);
                w.o.float(soft);
                w.o.byte(if bend != 0.0 { 1 } else { 0xFF });
                w.o.boolean(compress != 0.0);
                w.o.boolean(stretch != 0.0);
                Ok(())
            })?;
        }
        // 변형 제약: 회전 · 이동 · 크기 · 기울임 mix
        self.o.varint(trs.len() as i64);
        let mut tr = Vec::new();
        for (n, t) in trs.items {
            tr.push((lookup(&self.tr_index, &n, "변형 제약")?, t));
        }
        tr.sort_by_key(|x| x.0);
        for (i, t) in tr {
            self.o.varint(i as i64);
            self.o.varint(t.keys.len() as i64);
            self.keys38(&t.keys, 6, |w, k| {
                let v = vals(k);
                for c in [0, 1, 3, 5] {
                    w.o.float(v.get(c).copied().unwrap_or(0.0));
                }
                Ok(())
            })?;
        }
        // 경로 제약
        self.o.varint(paths.len() as i64);
        let mut pa = Vec::new();
        for (n, ts) in paths.items {
            pa.push((lookup(&self.path_index, &n, "경로 제약")?, ts));
        }
        pa.sort_by_key(|x| x.0);
        for (i, ts) in pa {
            self.o.varint(i as i64);
            self.o.varint(ts.len() as i64);
            for t in ts {
                let (code, n) = match t.type_str() {
                    "pathPosition" => (0, 1),
                    "pathSpacing" => (1, 1),
                    _ => (2, 2),
                };
                self.o.byte(code);
                self.o.varint(t.keys.len() as i64);
                self.keys38(&t.keys, if n == 1 { 1 } else { 3 }, |w, k| {
                    let v = vals(k);
                    for c in 0..n {
                        w.o.float(v.get(c).copied().unwrap_or(0.0));
                    }
                    Ok(())
                })?;
            }
        }
        // 변형(deform)
        self.o.varint(deforms.len() as i64);
        let mut de = Vec::new();
        for (skin, slots) in deforms.items {
            let i = self.skins.iter().position(|x| s(&x.name) == skin).ok_or_else(|| err(format!("스킨을 찾을 수 없습니다: {skin}")))?;
            de.push((i, slots));
        }
        de.sort_by_key(|x| x.0);
        for (i, slots) in de {
            self.o.varint(i as i64);
            self.o.varint(slots.len() as i64);
            let mut ss = slots.items;
            ss.sort_by(|a, b| nat_cmp(&a.0, &b.0));
            for (slot, ts) in ss {
                let si = self.slot(&slot)?;
                self.o.varint(si);
                self.o.varint(ts.len() as i64);
                for t in ts {
                    let att = self.find_att(t)?;
                    let map = att.dense_map.as_deref().unwrap_or(&[]);
                    self.rf(t.attachment.as_deref())?;
                    self.o.varint(t.keys.len() as i64);
                    self.keys38(&t.keys, 1, |w, k| {
                        let dense = k.dense.as_deref().unwrap_or(&[]);
                        let vec: Vec<f64> = map.iter().map(|&j| if j >= 0 && (j as usize) < dense.len() { dense[j as usize] } else { 0.0 }).collect();
                        match vec.iter().position(|x| *x != 0.0) {
                            None => w.o.varint(0),
                            Some(start) => {
                                let end = vec.iter().rposition(|x| *x != 0.0).unwrap() + 1;
                                w.o.varint((end - start) as i64);
                                w.o.varint(start as i64);
                                for v in &vec[start..end] {
                                    w.o.float(*v);
                                }
                            }
                        }
                        Ok(())
                    })?;
                }
            }
        }
        // 그리기 순서
        self.o.varint(draw.len() as i64);
        for k in draw {
            self.o.float(self.sec(k.frame));
            let mut changed = Vec::new();
            for (slot, off) in k.offsets.as_deref().unwrap_or(&[]) {
                changed.push((self.slot(slot)?, *off));
            }
            changed.sort_by_key(|x| x.0);
            self.o.varint(changed.len() as i64);
            for (i, off) in changed {
                self.o.varint(i);
                self.o.varint(off);
            }
        }
        // 이벤트
        self.o.varint(events.len() as i64);
        for k in events {
            let e = k.event.as_ref().ok_or_else(|| err("이벤트 키에 이벤트가 없습니다".to_string()))?;
            let data = self.m.events.iter().find(|d| d.name == e.name).ok_or_else(|| err(format!("이벤트 정의를 찾을 수 없습니다: {}", s(&e.name))))?;
            self.o.float(self.sec(k.frame));
            let i = lookup(&self.event_index, s(&e.name), "이벤트")?;
            self.o.varint(i as i64);
            self.o.svarint(e.int);
            self.o.float(e.float);
            // 3.8: 문자열이 정의와 다를 때만 싣는다(있음 여부를 먼저 쓴다)
            let st = e.string.as_deref().filter(|x| *x != s(&data.string));
            self.o.boolean(st.is_some());
            if let Some(st) = st {
                self.o.string(Some(st));
            }
            if !s(&data.audio).is_empty() {
                self.o.float(e.volume);
                self.o.float(e.balance);
            }
        }
        Ok(())
    }

    fn bezier_count(&self, keys: &[ModelKey], nch: usize) -> i64 {
        let mut n = 0;
        for i in 0..keys.len().saturating_sub(1) {
            if self.curve(keys, i, nch).0 == CURVE_BEZIER {
                n += nch as i64;
            }
        }
        n
    }

    /// 곡선 종류와 손잡이(시간은 초, 값은 그대로).
    fn curve(&self, keys: &[ModelKey], i: usize, nch: usize) -> Curve {
        let (k, nxt) = (&keys[i], &keys[i + 1]);
        let cv = curve_str(k);
        if cv == "stepped" || same_key_stepped(k, nxt, nch) {
            return (CURVE_STEPPED, Vec::new());
        }
        if let (Some(d0), Some(d1)) = (&k.dense, &nxt.dense) {
            if cv == "linear"
                && d0.len() == d1.len()
                && d0.iter().zip(d1).all(|(a, b)| (a - b).abs() <= 0.0001)
            {
                return (CURVE_STEPPED, Vec::new());
            }
        }
        if cv == "linear" {
            return (CURVE_LINEAR, Vec::new());
        }
        let (t0, t1) = (k.frame, nxt.frame);
        let (kv, nv) = (vals(k), vals(nxt));
        let (kh, nh) = (hds(k), hds(nxt));
        let mut hs = Vec::with_capacity(nch);
        for c in 0..nch {
            let (v0, v1) = if !kv.is_empty() {
                (kv[c], nv[c])
            } else {
                (0.0, 1.0)
            };
            let (cx1, cy1) = match kh.get(c) {
                Some(h) if h[2] != UNSET => (h[2], h[3]),
                _ => (t0 + (t1 - t0) / 3.0, v0),
            };
            let (cx2, cy2) = match nh.get(c) {
                Some(h) if h[0] != UNSET => (h[0], h[1]),
                _ => (t1 - (t1 - t0) / 3.0, v1),
            };
            hs.push([self.sec(cx1), cy1, self.sec(cx2), cy2]);
        }
        (CURVE_BEZIER, hs)
    }

    fn write_curve(&mut self, keys: &[ModelKey], i: usize, nch: usize) {
        let (kind, hs) = self.curve(keys, i, nch);
        self.o.byte(kind);
        if kind == CURVE_BEZIER {
            for h in hs {
                for v in h {
                    self.o.float(v);
                }
            }
        }
    }

    /// 런타임은 다음 키를 읽은 뒤 앞 구간의 곡선을 읽는다.
    fn values_timeline(&mut self, keys: &[ModelKey], nch: usize) {
        for (i, k) in keys.iter().enumerate() {
            self.o.float(self.sec(k.frame));
            let v = vals(k);
            for c in 0..nch {
                self.o.float(v.get(c).copied().unwrap_or(0.0));
            }
            if i > 0 {
                self.write_curve(keys, i - 1, nch);
            }
        }
    }

    /// 종류 바이트, 키 수, 베지어 수, 키들 — 경로·물리·슬라이더 타임라인 공통.
    fn typed_timeline(&mut self, t: &ModelTimeline, code: i64, nch: usize) {
        self.o.byte(code);
        self.o.varint(t.keys.len() as i64);
        let bc = self.bezier_count(&t.keys, nch);
        self.o.varint(bc);
        self.values_timeline(&t.keys, nch);
    }

    fn bone_timeline(&mut self, t: &ModelTimeline) -> SpineResult<()> {
        let ty = t.type_str();
        let code = match ty {
            "rotate" => 0,
            "translate" => 1,
            "translatex" => 2,
            "translatey" => 3,
            "scale" => 4,
            "shear" => 7,
            _ => return Err(err(format!("알 수 없는 뼈 타임라인: {ty}"))),
        };
        let nch = if matches!(ty, "rotate" | "translatex" | "translatey") {
            1
        } else {
            2
        };
        self.typed_timeline(t, code, nch);
        Ok(())
    }

    fn slot_timeline(&mut self, t: &ModelTimeline) -> SpineResult<()> {
        let keys = &t.keys;
        if t.type_str() == "attachment" {
            self.o.byte(0);
            self.o.varint(keys.len() as i64);
            for k in keys {
                self.o.float(self.sec(k.frame));
                self.rf(k.name.as_deref())?;
            }
            return Ok(());
        }
        let slot = self
            .m
            .slots
            .iter()
            .find(|x| x.name == t.slot)
            .ok_or_else(|| err(format!("슬롯을 찾을 수 없습니다: {}", s(&t.slot))))?;
        let two = slot.dark.is_some();
        let nch = if two { 7 } else { 4 };
        self.o.byte(if two { 3 } else { 1 });
        self.o.varint(keys.len() as i64);
        let bc = self.bezier_count(keys, nch);
        self.o.varint(bc);
        for (i, k) in keys.iter().enumerate() {
            self.o.float(self.sec(k.frame));
            let v = vals(k);
            for c in 0..nch {
                self.o
                    .byte((v.get(c).copied().unwrap_or(0.0) * 255.0).round_ties_even() as i64);
            }
            if i > 0 {
                self.write_curve(keys, i - 1, nch);
            }
        }
        Ok(())
    }

    fn ik_timeline(&mut self, t: &ModelTimeline) -> SpineResult<()> {
        let keys = &t.keys;
        let idx = if self.v43() {
            self.con(s(&t.constraint))?
        } else {
            lookup(&self.ik_index, s(&t.constraint), "IK 제약")? as i64
        };
        self.o.varint(idx);
        self.o.varint(keys.len() as i64);
        let bc = self.bezier_count(keys, 2);
        self.o.varint(bc);
        let five = |k: &ModelKey| -> [f64; 5] {
            let v = vals(k);
            let g = |i: usize| v.get(i).copied().unwrap_or(0.0);
            [g(0), g(1), g(2), g(3), g(4)]
        };
        if !self.v42() {
            // 4.0·4.1: 시간, mix, softness, 곡선, 굽힘 방향, compress, stretch
            for (i, k) in keys.iter().enumerate() {
                let [mix, soft, stretch, compress, bend] = five(k);
                self.o.float(self.sec(k.frame));
                self.o.float(mix);
                self.o.float(soft);
                if i > 0 {
                    self.write_curve(keys, i - 1, 2);
                }
                self.o.byte(if bend != 0.0 { 1 } else { 0xFF });
                self.o.boolean(compress != 0.0);
                self.o.boolean(stretch != 0.0);
            }
            return Ok(());
        }
        let buggy = self.vtuple.as_slice() >= [4, 2, 0].as_slice()
            && self.vtuple.as_slice() < [4, 2, 22].as_slice();
        for (i, k) in keys.iter().enumerate() {
            let [mut mix, mut soft, stretch, compress, bend] = five(k);
            if i > 0 && buggy {
                // 4.2.00~4.2.20 편집기는 두 번째 키부터 mix·softness 를 앞 키 값으로 썼다(표본 실측)
                let p = five(&keys[i - 1]);
                mix = p[0];
                soft = p[1];
            }
            let mut flags = 0i64;
            if mix != 0.0 {
                flags |= 1;
            }
            if mix != 1.0 {
                flags |= 2;
            }
            if soft != 0.0 {
                flags |= 4;
            }
            if bend != 0.0 {
                flags |= 8;
            }
            if compress != 0.0 {
                flags |= 16;
            }
            if stretch != 0.0 {
                flags |= 32;
            }
            let mut hs = Vec::new();
            if i > 0 {
                let (kind, h) = self.curve(keys, i - 1, 2);
                if kind == CURVE_STEPPED {
                    flags |= 64;
                } else if kind == CURVE_BEZIER {
                    flags |= 128;
                }
                hs = h;
            }
            self.o.byte(flags);
            self.o.float(self.sec(k.frame));
            if flags & 3 == 3 {
                self.o.float(mix);
            }
            if flags & 4 != 0 {
                self.o.float(soft);
            }
            if flags & 128 != 0 {
                for h in hs {
                    for v in h {
                        self.o.float(v);
                    }
                }
            }
        }
        Ok(())
    }

    fn sequence_timeline(&mut self, t: &ModelTimeline) -> SpineResult<()> {
        self.rf(t.attachment.as_deref())?;
        self.o.byte(1);
        self.o.varint(t.keys.len() as i64);
        for k in &t.keys {
            self.o.float(self.sec(k.frame));
            let v = vals(k);
            let g = |i: usize| v.get(i).copied().unwrap_or(0.0);
            let (mode, index, delay) = (g(0), g(1), g(2));
            self.o.int(((index as i64) << 4) | (mode as i64));
            self.o.float(delay);
        }
        Ok(())
    }

    fn find_att(&self, t: &ModelTimeline) -> SpineResult<&'a ModelAttachment> {
        for sk in &self.m.skins {
            if sk.name == t.skin {
                for e in &sk.attachments {
                    if e.slot == t.slot && e.key == t.attachment {
                        if let Some(a) = &e.attachment {
                            return Ok(a);
                        }
                    }
                }
            }
        }
        Err(err(format!(
            "첨부를 찾을 수 없습니다: {}/{}/{}",
            s(&t.skin),
            s(&t.slot),
            s(&t.attachment)
        )))
    }

    fn deform_timeline(&mut self, t: &ModelTimeline) -> SpineResult<()> {
        let att = self.find_att(t)?;
        let keys = &t.keys;
        self.rf(t.attachment.as_deref())?;
        if self.major >= (4, 1) {
            self.o.byte(0); // 4.1~: 첨부 타임라인 종류(0 = deform). 4.0 에는 없다
        }
        self.o.varint(keys.len() as i64);
        let bc = self.bezier_count(keys, 1);
        self.o.varint(bc);
        let map = att.dense_map.as_deref().unwrap_or(&[]);
        for (i, k) in keys.iter().enumerate() {
            let dense = k.dense.as_deref().unwrap_or(&[]);
            let vec: Vec<f64> = map
                .iter()
                .map(|&j| {
                    if j >= 0 && (j as usize) < dense.len() {
                        dense[j as usize]
                    } else {
                        0.0
                    }
                })
                .collect();
            self.o.float(self.sec(k.frame));
            if i > 0 {
                self.write_curve(keys, i - 1, 1);
            }
            // 바이너리는 정확히 0 인 앞뒤만 자른다(JSON 과 다름)
            let first = vec.iter().position(|x| *x != 0.0);
            match first {
                None => self.o.varint(0),
                Some(start) => {
                    let end = vec.iter().rposition(|x| *x != 0.0).unwrap() + 1;
                    self.o.varint((end - start) as i64);
                    self.o.varint(start as i64);
                    for v in &vec[start..end] {
                        self.o.float(*v);
                    }
                }
            }
        }
        Ok(())
    }

    fn event_key(&mut self, k: &ModelKey) -> SpineResult<()> {
        let e = k
            .event
            .as_ref()
            .ok_or_else(|| err("이벤트 키에 이벤트가 없습니다".to_string()))?;
        let data = self
            .m
            .events
            .iter()
            .find(|d| d.name == e.name)
            .ok_or_else(|| err(format!("이벤트 정의를 찾을 수 없습니다: {}", s(&e.name))))?;
        self.o.float(self.sec(k.frame));
        let i = lookup(&self.event_index, s(&e.name), "이벤트")?;
        self.o.varint(i as i64);
        self.o.svarint(e.int);
        self.o.float(e.float);
        let st = e.string.as_deref().filter(|x| *x != s(&data.string));
        self.o.string(st);
        if !s(&data.audio).is_empty() {
            self.o.float(e.volume);
            self.o.float(e.balance);
        }
        Ok(())
    }
}
