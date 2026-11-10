//! 스켈레톤 모델 → Spine 런타임 JSON. C# `SkeletonJsonWriter.cs` + `SpineExportNumbers.cs` 포팅.
//!
//! 공개 API(계약): [`write_json`]
//!
//! 4.0·4.1·4.2 는 옛 목록형, 4.3 은 constraints 통합형.
//! 4.0~4.2 는 값 종류마다 정해진 소수 자릿수로 반올림하고 끝의 0을 지운다.
//! 4.3 은 반올림하지 않고 float32 의 가장 짧은 표기를 쓴다. 기본값과 같은 필드는 쓰지 않는다.

mod json;
mod natural;
mod numbers;
mod sha1;

use std::cmp::Ordering;
use std::collections::{HashMap, HashSet};

use spine_model::{
    ModelAnimation, ModelAttachment, ModelBone, ModelConstraint, ModelEvent, ModelFromProperty, ModelKey, ModelSkin,
    ModelSkinEntry, ModelSlot, ModelTimeline, ModelToProperty, SkeletonModel, SpineError, SpineResult,
};

use json::{dumps_styled, write_map_entries, JsonMap, J};

pub use json::JsonFormat;
use natural::{natural_cmp, ordinal_cmp};
use numbers::{f32r, fmt_f32, fmt_round, round_half_up};

/// 숫자 표기 함수(다른 출력기·테스트에서 쓸 수 있게 공개).
pub mod number {
    pub use crate::numbers::{fmt_f32, fmt_round, java_double, round_half_up};
}

/// version: "4.0.xx" ~ "4.3.xx". nonessential: 비필수 데이터 포함.
pub fn write_json(model: &SkeletonModel, version: &str, nonessential: bool) -> SpineResult<String> {
    write_json_styled(model, version, nonessential, JsonFormat::Json, false)
}

/// [`write_json`] 에 Spine 내보내기 대화상자의 Format · Pretty print 를 더한 것.
pub fn write_json_styled(model: &SkeletonModel, version: &str, nonessential: bool, format: JsonFormat, pretty: bool) -> SpineResult<String> {
    let w = Writer::new(model, version, nonessential)?;
    let output = w.build()?;
    Ok(dumps_styled(&J::Map(output), format, pretty))
}

const UNSET: f64 = 2147483648.0;

/// 시퀀스 키의 모드 이름(C# SkeletonModelReader.SequenceModes).
const SEQUENCE_MODES: [&str; 7] = ["hold", "once", "loop", "pingpong", "onceReverse", "loopReverse", "pingpongReverse"];

/// 값 종류(소수 자릿수).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Kind {
    Pos,
    Scale,
    Mix,
    Time,
    CTime,
    Uv,
    Weight,
    Deform,
}

impl Kind {
    fn digits(self) -> i32 {
        match self {
            Kind::Pos => 2,
            Kind::Scale => 4,
            Kind::Mix => 3,
            Kind::Time => 4,
            Kind::CTime => 3,
            Kind::Uv => 5,
            Kind::Weight => 5,
            Kind::Deform => 5,
        }
    }
}

use Kind::*;

fn format_err(msg: String) -> SpineError {
    SpineError::Format(format!("프로젝트 구조를 해석하지 못했습니다: {}", msg))
}

/// C# 에서 null 키는 예외지만 여기서는 빈 문자열로 둔다.
fn key(s: &Option<String>) -> &str {
    s.as_deref().unwrap_or("")
}

fn is_null_or_empty(s: &Option<String>) -> bool {
    s.as_deref().is_none_or(|v| v.is_empty())
}

fn eq(s: &Option<String>, v: &str) -> bool {
    s.as_deref() == Some(v)
}

fn values(k: &ModelKey) -> &[f64] {
    k.values.as_deref().unwrap_or(&[])
}

fn handles(k: &ModelKey) -> &[[f64; 4]] {
    k.handles.as_deref().unwrap_or(&[])
}

fn color8(x: f64) -> i32 {
    (x * 255.0).round_ties_even() as i32
}

/// float32 표현 사이의 간격 수(편집기 계산 오차로 끝 비트만 다른 키는 같은 값으로 본다).
fn ulps(a: f64, b: f64) -> i64 {
    (ulp_key(a) - ulp_key(b)).abs()
}

fn ulp_key(x: f64) -> i64 {
    let n = (x as f32).to_bits() as i32 as i64;
    if n >= 0 {
        n
    } else {
        -2147483648i64 - n
    }
}

fn color_text(values: &[f64]) -> String {
    values.iter().map(|&x| format!("{:02x}", color8(x))).collect()
}

/// 값이 같은 두 키 사이를 계단으로 낼지. IK 키는 mix·softness 만 보고 직선일 때만 계단이다.
fn same_key_stepped(k: &ModelKey, next: &ModelKey, channels: usize) -> bool {
    let a = values(k);
    let b = values(next);
    if a.is_empty() || a.len() != b.len() {
        return false;
    }

    let ik = a.len() == 5;
    if channels == 4 || channels == 7 {
        // 색은 8비트 값으로 비교한다(알파 0과 0.0011은 같은 키다)
        for i in 0..a.len() {
            if color8(a[i]) != color8(b[i]) {
                return false;
            }
        }
    } else {
        let count = if ik { a.len().min(2) } else { a.len() };
        for i in 0..count {
            if !((a[i] - b[i]).abs() <= 1e-6 || ulps(a[i], b[i]) <= 1) {
                return false;
            }
        }
    }

    if eq(&k.curve, "linear") {
        return true;
    }
    if ik {
        return false;
    }

    let kh = handles(k);
    let nh = handles(next);
    for c in 0..channels {
        if !(c < kh.len() && c < nh.len() && (kh[c][3] - a[c]).abs() <= 1e-5 && (nh[c][1] - b[c]).abs() <= 1e-5) {
            return false;
        }
    }
    true
}

fn put_curve(o: &mut JsonMap, curve: Option<J>) {
    if let Some(c) = curve {
        o.set("curve", c);
    }
}

fn set_if(o: &mut JsonMap, key: &str, on: bool) {
    if on {
        o.set(key, J::Bool(true));
    }
}

fn header_fields(o: &mut JsonMap, c: &ModelConstraint) {
    o.set("name", J::opt(&c.name));
    if c.order != 0 {
        o.set("order", J::Int(c.order as i64));
    }
    if c.skin {
        o.set("skin", J::Bool(true));
    }
}

fn bend_fields(o: &mut JsonMap, c: &ModelConstraint) {
    if c.bones.len() == 2 {
        if !c.bend_positive {
            o.set("bendPositive", J::Bool(false));
        }
        if c.stretch {
            o.set("stretch", J::Bool(true));
        }
    } else {
        if c.compress {
            o.set("compress", J::Bool(true));
        }
        if c.stretch {
            o.set("stretch", J::Bool(true));
        }
        if c.uniform {
            o.set("uniform", J::Bool(true));
        }
    }
}

/// 위치·간격의 자릿수: 비율 모드는 scale(4자리), 길이 모드는 pos(2자리).
fn path_kinds(c: &ModelConstraint) -> (Kind, Kind) {
    let pos = if eq(&c.position_mode, "percent") { Scale } else { Pos };
    let spacing = if eq(&c.spacing_mode, "percent") || eq(&c.spacing_mode, "proportional") { Scale } else { Pos };
    (pos, spacing)
}

const PROP_NAMES: [&str; 6] = ["rotate", "x", "y", "scaleX", "scaleY", "shearY"];

fn legacy_properties(c: &ModelConstraint) -> Vec<ModelFromProperty> {
    // 4.2 이하 프로젝트의 변형 제약을 4.3 형식으로: 여섯 속성이 각자 자기 자신으로, mix 가 0 인 속성은 빠진다
    let mut output = Vec::new();
    for i in 0..6 {
        if c.mixes[i] != 0.0 {
            let name = PROP_NAMES[i];
            output.push(ModelFromProperty {
                name: Some(name.to_string()),
                offset: 0.0,
                to: vec![ModelToProperty {
                    name: Some(name.to_string()),
                    offset: 0.0,
                    max: if name == "scaleX" || name == "scaleY" { 1.0 } else { 100.0 },
                    scale: 1.0,
                }],
            });
        }
    }
    output
}

fn target_names(props: &[ModelFromProperty]) -> HashSet<String> {
    props.iter().flat_map(|p| p.to.iter()).filter_map(|t| t.name.clone()).collect()
}

fn sequence_field(o: &mut JsonMap, a: &ModelAttachment) {
    let q = match &a.sequence {
        Some(q) => q,
        None => return,
    };
    let mut d = JsonMap::new();
    d.set("count", J::Int(q.count));
    if q.start != 1 {
        d.set("start", J::Int(q.start));
    }
    if q.digits != 0 {
        d.set("digits", J::Int(q.digits));
    }
    if q.setup != 0 {
        d.set("setup", J::Int(q.setup));
    }
    o.set("sequence", J::Map(d));
}

fn path_and_color(o: &mut JsonMap, a: &ModelAttachment) {
    if !is_null_or_empty(&a.path) {
        o.set("path", J::opt(&a.path));
    }
    if !eq(&a.color, "ffffffff") {
        o.set("color", J::opt(&a.color));
    }
}

/// 슬롯·첨부 키 모두 자연 정렬. 이름이 다르지만 자연 정렬로 같으면 뒤의 키를 보지 않는다(파이썬 튜플 비교와 같음).
fn row_order(a: &ModelSkinEntry, b: &ModelSkinEntry) -> Ordering {
    if a.slot != b.slot {
        return natural_cmp(key(&a.slot), key(&b.slot));
    }
    if a.key == b.key {
        Ordering::Equal
    } else {
        natural_cmp(key(&a.key), key(&b.key))
    }
}

/// .NET 의 double 기본 비교(NaN 이 가장 작다).
fn cmp_f64(a: f64, b: f64) -> Ordering {
    match a.partial_cmp(&b) {
        Some(o) => o,
        None => b.is_nan().cmp(&a.is_nan()),
    }
}

struct Writer<'a> {
    m: &'a SkeletonModel,
    version: String,
    major: i32,
    minor: i32,
    v43: bool,
    nonessential: bool,
    fps: f64,
}

impl<'a> Writer<'a> {
    fn new(model: &'a SkeletonModel, version: &str, nonessential: bool) -> SpineResult<Writer<'a>> {
        let bad = || SpineError::Usage(format!("내보낼 버전은 4.0.xx ~ 4.3.xx 형식이어야 합니다: {}", version));
        let parts: Vec<&str> = version.split('.').collect();
        if parts.len() < 2 {
            return Err(bad());
        }
        let major: i32 = parts[0].parse().map_err(|_| bad())?;
        let minor: i32 = parts[1].parse().map_err(|_| bad())?;
        let mut w = Writer {
            m: model,
            version: version.to_string(),
            major,
            minor,
            v43: false,
            nonessential,
            fps: model.fps,
        };
        w.v43 = w.at_least(4, 3);
        Ok(w)
    }

    fn at_least(&self, major: i32, minor: i32) -> bool {
        self.major > major || (self.major == major && self.minor >= minor)
    }

    // ------------------------------------------------------------ 숫자

    fn n(&self, x: f64, kind: Kind) -> J {
        J::Num(if self.v43 { fmt_f32(x, true) } else { fmt_round(x, kind.digits()) })
    }

    /// 4.3 에서 정수도 '.0' 을 붙이는 자리(정점·메시 크기 등).
    fn nf(&self, x: f64, kind: Kind) -> J {
        J::Num(if self.v43 { fmt_f32(x, false) } else { fmt_round(x, kind.digits()) })
    }

    /// 프레임 → 초. 4.0~4.2 편집기는 float32 로 1/fps 를 곱하고, 4.3 은 fps 로 나눈다.
    fn sec(&self, frame: f64) -> f64 {
        if self.v43 {
            return f32r(f32r(frame) / f32r(self.fps));
        }
        f32r(f32r(frame) * f32r(1.0 / self.fps))
    }

    fn time_of(&self, frame: f64) -> J {
        self.n(self.sec(frame), Time)
    }

    fn is_default(&self, x: f64, d: f64, kind: Kind) -> bool {
        if self.v43 {
            // 4.3 도 값 종류별 자릿수로 기본값을 판정한다(뼈 scaleY 0.999634 는 쓰고, 위치 0.004 는 뺀다)
            return round_half_up(f32r(x) - f32r(d), kind.digits()) == 0.0;
        }
        round_half_up(x, kind.digits()) == round_half_up(d, kind.digits())
    }

    /// 정점 좌표·가중치: 4.3 도 값은 자릿수로 반올림한다(표기만 float).
    fn vert(&self, x: f64, kind: Kind) -> J {
        if self.v43 {
            return J::Num(fmt_f32(round_half_up(x, kind.digits()), false));
        }
        J::Num(fmt_round(x, kind.digits()))
    }

    /// 기본값이 아니면 쓴다.
    fn put(&self, o: &mut JsonMap, key: &str, value: f64, fallback: f64, kind: Kind) {
        if !self.is_default(value, fallback, kind) {
            o.set(key, self.n(value, kind));
        }
    }

    // ------------------------------------------------------------ 전체

    fn build(&self) -> SpineResult<JsonMap> {
        let m = self.m;
        let mut output = JsonMap::new();
        output.set("skeleton", J::Map(self.skeleton()));
        output.set("bones", J::List(m.bones.iter().map(|b| self.bone(b)).collect()));
        output.set("slots", J::List(m.slots.iter().map(|s| self.slot(s)).collect()));
        let cons = &m.constraints;
        if self.v43 {
            if !cons.is_empty() {
                output.set("constraints", J::List(cons.iter().map(|c| self.constraint43(c)).collect()));
            }
        } else {
            let iks = sorted_of_type(cons, "ik");
            let trs = sorted_of_type(cons, "transform");
            let paths = sorted_of_type(cons, "path");
            let phys = sorted_of_type(cons, "physics");
            if cons.iter().any(|c| eq(&c.r#type, "slider")) {
                return Err(SpineError::Usage("슬라이더 제약은 Spine 4.3 전용이라 이 버전으로 내보낼 수 없습니다".to_string()));
            }
            if !iks.is_empty() {
                output.set("ik", J::List(iks.iter().map(|c| self.ik(c)).collect()));
            }
            if !trs.is_empty() {
                output.set("transform", J::List(trs.iter().map(|c| self.transform(c)).collect()));
            }
            if !paths.is_empty() {
                output.set("path", J::List(paths.iter().map(|c| self.path(c)).collect()));
            }
            if !phys.is_empty() && self.at_least(4, 2) {
                output.set("physics", J::List(phys.iter().map(|c| self.physics(c)).collect()));
            } else if !phys.is_empty() {
                return Err(SpineError::Usage("물리 제약은 Spine 4.2 이상에서만 내보낼 수 있습니다".to_string()));
            }
        }

        let mut skins = Vec::with_capacity(m.skins.len());
        for s in &m.skins {
            skins.push(self.skin(s)?);
        }
        output.set("skins", J::List(skins));
        if !m.events.is_empty() {
            let mut events = JsonMap::new();
            for e in &m.events {
                events.set(key(&e.name), J::Map(self.event(e)));
            }
            output.set("events", J::Map(events));
        }

        let mut animations = JsonMap::new();
        for a in &m.animations {
            let value = self.animation(a)?;
            animations.set(key(&a.name), J::Map(value));
        }
        if animations.len() > 0 {
            output.set("animations", J::Map(animations));
        }

        // 해시는 맨 앞에 온다
        let hash = hash(&output);
        let mut with_hash = JsonMap::new();
        with_hash.set("hash", J::Str(hash));
        if let Some(J::Map(skeleton)) = output.get("skeleton") {
            for (k, v) in skeleton.entries() {
                with_hash.set(k, v.clone());
            }
        }
        output.set("skeleton", J::Map(with_hash));
        Ok(output)
    }

    fn skeleton(&self) -> JsonMap {
        let m = self.m;
        let mut s = JsonMap::new();
        s.set("spine", J::str(&self.version));
        if let Some(b) = &m.bounds {
            if !b.is_empty() {
                s.set("x", self.n(b[0], Pos));
                s.set("y", self.n(b[1], Pos));
                s.set("width", self.n(b[2], Pos));
                s.set("height", self.n(b[3], Pos));
            }
        }
        if m.reference_scale != 100.0 && self.at_least(4, 2) {
            s.set("referenceScale", self.n(m.reference_scale, Pos));
        }
        if self.nonessential {
            if m.fps != 30.0 {
                s.set("fps", self.n(m.fps, Pos));
            }
            s.set("images", J::opt(&m.images));
            let audio = match &m.audio {
                Some(a) if !a.is_empty() || !self.v43 => J::str(a),
                _ => J::Null,
            };
            s.set("audio", audio);
        }
        s
    }

    fn bone(&self, b: &ModelBone) -> J {
        let mut o = JsonMap::new();
        o.set("name", J::opt(&b.name));
        if b.parent.is_some() {
            o.set("parent", J::opt(&b.parent));
        }
        if b.length != 0.0 {
            o.set("length", self.n(b.length, Pos));
        }
        self.put(&mut o, "rotation", b.rotation, 0.0, Pos);
        self.put(&mut o, "x", b.x, 0.0, Pos);
        self.put(&mut o, "y", b.y, 0.0, Pos);
        self.put(&mut o, "scaleX", b.scale_x, 1.0, Scale);
        self.put(&mut o, "scaleY", b.scale_y, 1.0, Scale);
        self.put(&mut o, "shearX", b.shear_x, 0.0, Pos);
        self.put(&mut o, "shearY", b.shear_y, 0.0, Pos);
        if !eq(&b.inherit, "normal") {
            o.set(if self.at_least(4, 2) { "inherit" } else { "transform" }, J::opt(&b.inherit));
        }
        if b.skin {
            o.set("skin", J::Bool(true));
        }
        if self.nonessential {
            if !is_null_or_empty(&b.color) && !eq(&b.color, "989898ff") && !eq(&b.color, "9b9b9bff") {
                o.set("color", J::opt(&b.color));
            }
            if !is_null_or_empty(&b.icon) && !eq(&b.icon, "bone") && self.at_least(4, 1) {
                o.set("icon", J::opt(&b.icon));
            }
        }
        J::Map(o)
    }

    fn slot(&self, s: &ModelSlot) -> J {
        let mut o = JsonMap::new();
        o.set("name", J::opt(&s.name));
        o.set("bone", J::opt(&s.bone));
        if !eq(&s.color, "ffffffff") {
            o.set("color", J::opt(&s.color));
        }
        if !is_null_or_empty(&s.dark) {
            o.set("dark", J::opt(&s.dark));
        }
        if s.attachment.is_some() {
            o.set("attachment", J::opt(&s.attachment));
        }
        if !eq(&s.blend, "normal") {
            o.set("blend", J::opt(&s.blend));
        }
        if self.nonessential && !s.visible && self.at_least(4, 1) {
            o.set("visible", J::Bool(false));
        }
        J::Map(o)
    }

    fn ik(&self, c: &ModelConstraint) -> J {
        let mut o = JsonMap::new();
        header_fields(&mut o, c);
        o.set("bones", J::strings(&c.bones));
        o.set("target", J::opt(&c.target));
        self.put(&mut o, "mix", c.mix, 1.0, Mix);
        self.put(&mut o, "softness", c.softness, 0.0, Pos);
        bend_fields(&mut o, c);
        J::Map(o)
    }

    fn transform(&self, c: &ModelConstraint) -> J {
        let mut o = JsonMap::new();
        header_fields(&mut o, c);
        o.set("bones", J::strings(&c.bones));
        o.set("target", J::opt(&c.target));
        self.put(&mut o, "rotation", c.rotation, 0.0, Pos);
        if c.local {
            o.set("local", J::Bool(true));
        }
        if c.relative {
            o.set("relative", J::Bool(true));
        }
        self.put(&mut o, "x", c.x, 0.0, Pos);
        self.put(&mut o, "y", c.y, 0.0, Pos);
        self.put(&mut o, "scaleX", c.scale_x, 0.0, Scale);
        self.put(&mut o, "scaleY", c.scale_y, 0.0, Scale);
        self.put(&mut o, "shearY", c.shear_y, 0.0, Pos);
        self.mixes(&mut o, &c.mixes, false);
        J::Map(o)
    }

    fn path_fields(&self, o: &mut JsonMap, c: &ModelConstraint) {
        if !eq(&c.position_mode, "percent") {
            o.set("positionMode", J::opt(&c.position_mode));
        }
        if !eq(&c.spacing_mode, "length") {
            o.set("spacingMode", J::opt(&c.spacing_mode));
        }
        if !eq(&c.rotate_mode, "tangent") {
            o.set("rotateMode", J::opt(&c.rotate_mode));
        }
        let (pos, spc) = path_kinds(c);
        self.put(o, "rotation", c.rotation, 0.0, Pos);
        self.put(o, "position", c.position, 0.0, pos);
        self.put(o, "spacing", c.spacing, 0.0, spc);
        self.put(o, "mixRotate", c.mixes[ModelConstraint::MIX_ROTATE], 1.0, Mix);
        self.put(o, "mixX", c.mixes[ModelConstraint::MIX_X], 1.0, Mix);
        self.put(o, "mixY", c.mixes[ModelConstraint::MIX_Y], c.mixes[ModelConstraint::MIX_X], Mix);
    }

    fn path(&self, c: &ModelConstraint) -> J {
        let mut o = JsonMap::new();
        header_fields(&mut o, c);
        o.set("bones", J::strings(&c.bones));
        o.set("target", J::opt(&c.target));
        self.path_fields(&mut o, c);
        J::Map(o)
    }

    fn physics_fields(&self, o: &mut JsonMap, c: &ModelConstraint) {
        self.put(o, "x", c.x, 0.0, Scale);
        self.put(o, "y", c.y, 0.0, Scale);
        self.put(o, "rotate", c.rotate, 0.0, Scale);
        self.put(o, "scaleX", c.scale_x, 0.0, Scale);
        self.put(o, "shearX", c.shear_x, 0.0, Scale);
        self.put(o, "limit", c.limit, 5000.0, Pos);
        self.put(o, "fps", c.fps, 60.0, Pos);
        // 관성·감쇠의 생략 기준은 런타임 버전마다 다르다: 4.3 은 0.5·0.85, 4.2 는 1
        self.put(o, "inertia", c.inertia, if self.at_least(4, 3) { 0.5 } else { 1.0 }, Scale);
        self.put(o, "strength", c.strength, 100.0, Pos);
        self.put(o, "damping", c.damping, if self.at_least(4, 3) { 0.85 } else { 1.0 }, Scale);
        self.put(o, "mass", c.mass, 1.0, Scale);
        self.put(o, "wind", c.wind, 0.0, Pos);
        self.put(o, "gravity", c.gravity, 0.0, Pos);
        self.put(o, "mix", c.mix, 1.0, Scale);
        set_if(o, "inertiaGlobal", c.inertia_global);
        set_if(o, "strengthGlobal", c.strength_global);
        set_if(o, "dampingGlobal", c.damping_global);
        set_if(o, "massGlobal", c.mass_global);
        set_if(o, "windGlobal", c.wind_global);
        set_if(o, "gravityGlobal", c.gravity_global);
        set_if(o, "mixGlobal", c.mix_global);
    }

    fn physics(&self, c: &ModelConstraint) -> J {
        let mut o = JsonMap::new();
        header_fields(&mut o, c);
        o.set("bone", J::opt(&c.bone));
        self.physics_fields(&mut o, c);
        J::Map(o)
    }

    /// mix 필드 여섯 개. 순서: rotate, x, y, scaleX, scaleY, shearY.
    fn mixes(&self, o: &mut JsonMap, c: &[f64; 6], keyed: bool) {
        let defaults = [
            1.0,
            1.0,
            c[ModelConstraint::MIX_X],
            1.0,
            if self.v43 && keyed { 1.0 } else { c[ModelConstraint::MIX_SCALE_X] }, // 4.3 키의 mixScaleY 기본값은 1
            1.0,
        ];
        let kind = if keyed { Scale } else { Mix };
        for i in 0..6 {
            self.put(o, ModelConstraint::MIX_NAMES[i], c[i], defaults[i], kind);
        }
    }

    fn constraint43(&self, c: &ModelConstraint) -> J {
        let mut o = JsonMap::new();
        o.set("type", J::opt(&c.r#type));
        o.set("name", J::opt(&c.name));
        if c.skin {
            o.set("skin", J::Bool(true));
        }

        if eq(&c.r#type, "ik") {
            o.set("target", J::opt(&c.target));
            o.set("bones", J::strings(&c.bones));
            self.put(&mut o, "mix", c.mix, 1.0, Pos);
            self.put(&mut o, "softness", c.softness, 0.0, Pos);
            bend_fields(&mut o, c);
            return J::Map(o);
        }

        if eq(&c.r#type, "physics") {
            o.set("bone", J::opt(&c.bone));
            self.physics_fields(&mut o, c);
            return J::Map(o);
        }

        if eq(&c.r#type, "slider") {
            set_if(&mut o, "loop", c.r#loop);
            if !is_null_or_empty(&c.animation) {
                o.set("animation", J::opt(&c.animation));
            }
            if !is_null_or_empty(&c.bone) {
                o.set("bone", J::opt(&c.bone));
            }
            if !is_null_or_empty(&c.property) {
                o.set("property", J::opt(&c.property));
            }
            self.put(&mut o, "max", c.max, 0.0, Pos);
            if !self.is_default(c.scale, 1.0, Scale) {
                o.set("scale", self.n(c.scale, Pos)); // 4.3 이라 자릿수는 쓰이지 않는다
            }
            set_if(&mut o, "local", c.local);
            return J::Map(o);
        }

        if eq(&c.r#type, "path") {
            o.set("slot", J::opt(&c.target));
            o.set("bones", J::strings(&c.bones));
            self.path_fields(&mut o, c);
            return J::Map(o);
        }

        o.set("source", J::opt(&c.target));
        o.set("bones", J::strings(&c.bones));
        let legacy;
        let props: &[ModelFromProperty] = match &c.properties {
            Some(p) => p,
            None => {
                legacy = legacy_properties(c);
                &legacy
            }
        };
        let targets = target_names(props);
        self.put_if_target(&mut o, &targets, "rotation", "rotate", c.rotation);
        self.put_if_target(&mut o, &targets, "x", "x", c.x);
        self.put_if_target(&mut o, &targets, "y", "y", c.y);
        self.put_if_target(&mut o, &targets, "scaleX", "scaleX", c.scale_x);
        self.put_if_target(&mut o, &targets, "scaleY", "scaleY", c.scale_y);
        self.put_if_target(&mut o, &targets, "shearY", "shearY", c.shear_y);
        let mut po = JsonMap::new();
        for from in props {
            let mut d = JsonMap::new();
            self.put(&mut d, "offset", from.offset, 0.0, Pos);
            let mut to = JsonMap::new();
            for t in &from.to {
                let mut td = JsonMap::new();
                self.put(&mut td, "offset", t.offset, 0.0, Pos);
                self.put(&mut td, "max", t.max, 1.0, Pos);
                self.put(&mut td, "scale", t.scale, 1.0, Pos);
                to.set(key(&t.name), J::Map(td));
            }
            d.set("to", J::Map(to));
            po.set(key(&from.name), J::Map(d));
        }
        o.set("properties", J::Map(po));
        if c.local {
            o.set("localSource", J::Bool(true));
            o.set("localTarget", J::Bool(true));
        }
        set_if(&mut o, "additive", c.relative);
        let mx = &c.mixes;
        self.put_if_have(&mut o, &targets, "mixRotate", "rotate", mx[ModelConstraint::MIX_ROTATE], 1.0);
        self.put_if_have(&mut o, &targets, "mixX", "x", mx[ModelConstraint::MIX_X], 1.0);
        self.put_if_have(
            &mut o,
            &targets,
            "mixY",
            "y",
            mx[ModelConstraint::MIX_Y],
            if targets.contains("x") { mx[ModelConstraint::MIX_X] } else { 1.0 },
        );
        self.put_if_have(&mut o, &targets, "mixScaleX", "scaleX", mx[ModelConstraint::MIX_SCALE_X], 1.0);
        self.put_if_have(
            &mut o,
            &targets,
            "mixScaleY",
            "scaleY",
            mx[ModelConstraint::MIX_SCALE_Y],
            if targets.contains("scaleX") { mx[ModelConstraint::MIX_SCALE_X] } else { 1.0 },
        );
        self.put_if_have(&mut o, &targets, "mixShearY", "shearY", mx[ModelConstraint::MIX_SHEAR_Y], 1.0);
        J::Map(o)
    }

    // 대상 속성이 없는 오프셋은 쓰지 않는다
    fn put_if_target(&self, o: &mut JsonMap, targets: &HashSet<String>, key: &str, prop: &str, value: f64) {
        if targets.contains(prop) {
            self.put(o, key, value, 0.0, Pos);
        }
    }

    fn put_if_have(&self, o: &mut JsonMap, targets: &HashSet<String>, key: &str, prop: &str, value: f64, fallback: f64) {
        if targets.contains(prop) {
            self.put(o, key, value, fallback, Pos);
        }
    }

    // ------------------------------------------------------------ 스킨

    fn constraint_type(&self, name: &str) -> Option<&str> {
        self.m.constraints.iter().find(|c| eq(&c.name, name)).and_then(|c| c.r#type.as_deref())
    }

    fn constraint(&self, name: &Option<String>) -> SpineResult<&'a ModelConstraint> {
        self.m
            .constraints
            .iter()
            .find(|c| c.name == *name)
            .ok_or_else(|| SpineError::Format(format!("제약을 찾을 수 없습니다: {}", key(name))))
    }

    fn skin(&self, s: &ModelSkin) -> SpineResult<J> {
        let mut o = JsonMap::new();
        o.set("name", J::opt(&s.name));
        if !s.bones.is_empty() {
            o.set("bones", J::strings(&s.bones));
        }

        let of_type = |ty: &str| -> Vec<String> {
            s.constraints.iter().filter(|n| self.constraint_type(n) == Some(ty)).cloned().collect()
        };
        let iks = of_type("ik");
        let trs = of_type("transform");
        let paths = of_type("path");
        let phys = of_type("physics");
        if !iks.is_empty() {
            o.set("ik", J::strings(&iks));
        }
        if !trs.is_empty() {
            o.set("transform", J::strings(&trs));
        }
        if !paths.is_empty() {
            o.set("path", J::strings(&paths));
        }
        if !phys.is_empty() {
            o.set("physics", J::strings(&phys));
        }

        let mut rows: Vec<&ModelSkinEntry> = s.attachments.iter().collect();
        rows.sort_by(|a, b| row_order(a, b));
        let mut atts = JsonMap::new();
        for row in rows {
            let att = row
                .attachment
                .as_ref()
                .ok_or_else(|| format_err(format!("첨부가 없습니다: {}/{}", key(&row.slot), key(&row.key))))?;
            let value = self.attachment(att)?;
            atts.get_or_add_map(key(&row.slot)).set(key(&row.key), J::Map(value));
        }
        if atts.len() > 0 {
            // 첨부가 없는 스킨은 키를 쓰지 않는다
            o.set("attachments", J::Map(atts));
        }
        Ok(J::Map(o))
    }

    fn attachment(&self, a: &ModelAttachment) -> SpineResult<JsonMap> {
        let mut o = JsonMap::new();
        let t = a.r#type.as_deref();
        if t != Some("region") {
            o.set("type", J::opt(&a.r#type));
        }
        if a.name != a.key {
            o.set("name", J::opt(&a.name));
        }

        match t {
            Some("region") => {
                path_and_color(&mut o, a);
                self.put(&mut o, "x", a.x, 0.0, Pos);
                self.put(&mut o, "y", a.y, 0.0, Pos);
                self.put(&mut o, "scaleX", a.scale_x, 1.0, Scale);
                self.put(&mut o, "scaleY", a.scale_y, 1.0, Scale);
                self.put(&mut o, "rotation", a.rotation, 0.0, Pos);
                o.set("width", self.n(a.width, Pos));
                o.set("height", self.n(a.height, Pos));
                sequence_field(&mut o, a);
            }
            Some("mesh") => {
                path_and_color(&mut o, a);
                let uvs = a.uvs.as_deref().unwrap_or(&[]);
                o.set("uvs", J::List(uvs.iter().map(|&x| self.n(x, Uv)).collect()));
                o.set(
                    "triangles",
                    match &a.triangles {
                        Some(t) => J::List(t.iter().map(|&x| J::Int(x)).collect()),
                        None => J::Null,
                    },
                );
                o.set("vertices", J::List(self.verts(a)));
                o.set("hull", J::Int(a.hull as i64));
                if self.nonessential {
                    if let Some(edges) = &a.edges {
                        if !edges.is_empty() {
                            o.set("edges", J::List(edges.iter().map(|&x| J::Int(x)).collect()));
                        }
                    }
                    o.set("width", self.nf(a.width, Pos));
                    o.set("height", self.nf(a.height, Pos));
                }
                sequence_field(&mut o, a);
            }
            Some("linkedmesh") => {
                path_and_color(&mut o, a);
                if !is_null_or_empty(&a.skin) {
                    o.set("skin", J::opt(&a.skin));
                }
                o.set(if self.v43 { "source" } else { "parent" }, J::opt(&a.parent));
                if !a.timelines {
                    o.set(if self.at_least(4, 1) { "timelines" } else { "deform" }, J::Bool(false));
                }
                if self.nonessential {
                    o.set("width", self.nf(a.width, Pos));
                    o.set("height", self.nf(a.height, Pos));
                }
            }
            Some("path") => {
                if a.closed {
                    o.set("closed", J::Bool(true));
                }
                if !a.constant_speed {
                    o.set("constantSpeed", J::Bool(false));
                }
                let lengths = a.lengths.as_deref().unwrap_or(&[]);
                o.set("lengths", J::List(lengths.iter().map(|&x| self.nf(x, Pos)).collect()));
                o.set("vertexCount", J::Int(a.vertex_count as i64));
                o.set("vertices", J::List(self.verts(a)));
                if self.nonessential && !eq(&a.color, "ff7f00ff") {
                    o.set("color", J::opt(&a.color));
                }
            }
            Some(ty @ ("boundingbox" | "clipping")) => {
                if ty == "clipping" {
                    o.set("end", J::opt(&a.end));
                    if self.v43 && a.convex {
                        o.set("convex", J::Bool(true));
                    }
                }
                o.set("vertexCount", J::Int(a.vertex_count as i64));
                o.set("vertices", J::List(self.verts(a)));
                if self.nonessential && !eq(&a.color, if ty == "boundingbox" { "60f000ff" } else { "ffffffff" }) {
                    o.set("color", J::opt(&a.color));
                }
            }
            Some("point") => {
                self.put(&mut o, "x", a.x, 0.0, Pos);
                self.put(&mut o, "y", a.y, 0.0, Pos);
                self.put(&mut o, "rotation", a.rotation, 0.0, Pos);
                if self.nonessential && !eq(&a.color, "f1f100ff") {
                    o.set("color", J::opt(&a.color));
                }
            }
            _ => return Err(format_err(format!("알 수 없는 첨부 종류: {}", t.unwrap_or("null")))),
        }
        Ok(o)
    }

    fn verts(&self, a: &ModelAttachment) -> Vec<J> {
        let v = a.vertices.as_deref().unwrap_or(&[]);
        if !a.weighted {
            return v.iter().map(|&x| self.vert(x, Pos)).collect();
        }
        let mut output = Vec::with_capacity(v.len());
        let mut i = 0;
        while i < v.len() {
            let n = v[i] as i64;
            output.push(self.count(n));
            i += 1;
            for _ in 0..n {
                output.push(self.count(v[i] as i64));
                output.push(self.vert(v[i + 1], Pos));
                output.push(self.vert(v[i + 2], Pos));
                output.push(self.vert(v[i + 3], Weight));
                i += 4;
            }
        }
        output
    }

    // 4.3 은 정점 배열 전체가 float 표기
    fn count(&self, n: i64) -> J {
        if self.v43 {
            J::Num(format!("{}.0", n))
        } else {
            J::Int(n)
        }
    }

    // ------------------------------------------------------------ 이벤트

    fn event(&self, e: &ModelEvent) -> JsonMap {
        let mut o = JsonMap::new();
        if e.int != 0 {
            o.set("int", J::Int(e.int));
        }
        if e.float != 0.0 {
            o.set("float", self.n(e.float, Pos));
        }
        if !is_null_or_empty(&e.string) {
            o.set("string", J::opt(&e.string));
        }
        if !is_null_or_empty(&e.audio) {
            o.set("audio", J::opt(&e.audio));
            o.set("volume", self.n(e.volume, Pos));
            o.set("balance", self.n(e.balance, Pos));
        }
        o
    }

    // ------------------------------------------------------------ 애니메이션

    fn animation(&self, an: &ModelAnimation) -> SpineResult<JsonMap> {
        let mut sec = JsonMap::new();
        let mut events: Vec<&ModelTimeline> = Vec::new();
        for t in &an.timelines {
            match t.type_str() {
                "color" | "attachment" => {
                    if t.type_str() == "attachment" {
                        let keys = self.att_keys(t);
                        sec.get_or_add_map("slots").get_or_add_map(key(&t.slot)).set("attachment", J::List(keys));
                    } else {
                        let (keys, name) = self.color_keys(t)?;
                        sec.get_or_add_map("slots").get_or_add_map(key(&t.slot)).set(name, J::List(keys));
                    }
                }
                "rotate" | "translate" | "scale" | "shear" => {
                    let keys = self.bone_keys(t);
                    sec.get_or_add_map("bones").get_or_add_map(key(&t.bone)).set(t.type_str(), J::List(keys));
                }
                "ik" => {
                    let keys = self.ik_keys(t)?;
                    sec.get_or_add_map("ik").set(key(&t.constraint), J::List(keys));
                }
                "transform" => {
                    let keys = self.transform_keys(t)?;
                    sec.get_or_add_map("transform").set(key(&t.constraint), J::List(keys));
                }
                "deform" => {
                    let keys = self.deform_keys(t)?;
                    if !self.at_least(4, 1) {
                        // 4.0: deform/<스킨>/<슬롯>/<첨부> = 키 목록
                        sec.get_or_add_map("deform")
                            .get_or_add_map(key(&t.skin))
                            .get_or_add_map(key(&t.slot))
                            .set(key(&t.attachment), J::List(keys));
                    } else {
                        sec.get_or_add_map("attachments")
                            .get_or_add_map(key(&t.skin))
                            .get_or_add_map(key(&t.slot))
                            .get_or_add_map(key(&t.attachment))
                            .set("deform", J::List(keys));
                    }
                }
                "event" => events.push(t),
                "drawOrder" => {
                    let keys = self.draw_keys(t)?;
                    sec.set("drawOrder", J::List(keys));
                }
                "translatex" | "translatey" => {
                    let keys = self.value_keys(t, 0.0, Pos);
                    sec.get_or_add_map("bones").get_or_add_map(key(&t.bone)).set(t.type_str(), J::List(keys));
                }
                "sequence" => {
                    let keys = self.seq_keys(t)?;
                    sec.get_or_add_map("attachments")
                        .get_or_add_map(key(&t.skin))
                        .get_or_add_map(key(&t.slot))
                        .get_or_add_map(key(&t.attachment))
                        .set("sequence", J::List(keys));
                }
                "physicsWind" => {
                    let keys = self.value_keys(t, 0.0, Pos);
                    sec.get_or_add_map("physics").get_or_add_map(key(&t.constraint)).set("wind", J::List(keys));
                }
                "physicsWindAll" => {
                    let keys = self.value_keys(t, 0.0, Pos);
                    sec.get_or_add_map("physics").get_or_add_map("").set("wind", J::List(keys));
                }
                "sliderMix" => {
                    let keys = self.value_keys(t, 1.0, Mix);
                    sec.get_or_add_map("slider").get_or_add_map(key(&t.constraint)).set("mix", J::List(keys));
                }
                ty @ ("pathPosition" | "pathSpacing" | "pathMix") => {
                    let name = match ty {
                        "pathPosition" => "position",
                        "pathSpacing" => "spacing",
                        _ => "mix",
                    };
                    let keys = self.path_keys(t)?;
                    sec.get_or_add_map("path").get_or_add_map(key(&t.constraint)).set(name, J::List(keys));
                }
                _ => {}
            }
        }

        if !events.is_empty() {
            // 이벤트 타임라인은 정의마다 따로 있다: 시간순으로 합친다(안정 정렬)
            let mut keys: Vec<&ModelKey> = events.iter().flat_map(|t| t.keys.iter()).collect();
            keys.sort_by(|a, b| cmp_f64(a.frame, b.frame));
            let list = self.event_keys(&keys)?;
            sec.set("events", J::List(list));
        }

        if self.v43 && sec.contains("bones") {
            // 4.3 은 뼈도 이름순
            sort_child(&mut sec, "bones", 0);
        }
        for k in ["slots", "ik", "transform", "constraints", "path", "physics"] {
            if sec.contains(k) {
                sort_child(&mut sec, k, 0);
            }
        }
        for name in ["attachments", "deform"] {
            if sec.contains(name) {
                // 스킨 → 슬롯 → 첨부 세 단계 모두 이름순
                sort_child(&mut sec, name, 2);
            }
        }

        let mut output = JsonMap::new();
        let mut entries: HashMap<String, J> = sec.into_entries().into_iter().collect();
        for k in ["slots", "bones", "ik", "transform", "path", "physics", "slider", "attachments", "deform", "drawOrder", "events"] {
            if let Some(v) = entries.remove(k) {
                output.set(k, v);
            }
        }
        Ok(output)
    }

    fn key_base(&self, k: &ModelKey) -> JsonMap {
        let mut o = JsonMap::new();
        if k.frame != 0.0 {
            o.set("time", self.time_of(k.frame));
        }
        o
    }

    fn curve_of(&self, keys: &[ModelKey], i: usize, kinds: &[Kind]) -> Option<J> {
        let k = &keys[i];
        if i == keys.len() - 1 {
            return None;
        }
        if eq(&k.curve, "stepped") {
            return Some(J::str("stepped"));
        }

        let next = &keys[i + 1];
        if let Some(kd) = &k.dense {
            if eq(&k.curve, "linear") && next.dense.as_ref().map(|d| d.len()) == Some(kd.len()) {
                let nd = next.dense.as_ref().unwrap();
                let flat = kd.iter().zip(nd.iter()).all(|(a, b)| (a - b).abs() <= 0.0001);
                if flat {
                    return Some(J::str("stepped"));
                }
            }
        }

        if same_key_stepped(k, next, kinds.len()) {
            return Some(J::str("stepped"));
        }
        if eq(&k.curve, "linear") {
            return None;
        }

        let t0 = k.frame;
        let t1 = next.frame;
        let kv = values(k);
        let nv = values(next);
        let kh = handles(k);
        let nh = handles(next);
        let mut output = Vec::with_capacity(kinds.len() * 4);
        for (c, &kind) in kinds.iter().enumerate() {
            let mut v0 = 0.0;
            let mut v1 = 1.0; // 값 없는 타임라인(변형 등): 곡선은 0→1 비율
            if !kv.is_empty() {
                v0 = kv[c];
                v1 = nv[c];
            }
            let h0 = kh.get(c);
            let h1 = nh.get(c);
            let mut cx1 = t0 + (t1 - t0) / 3.0;
            let mut cy1 = v0;
            if let Some(h) = h0 {
                if h[2] != UNSET {
                    cx1 = h[2];
                    cy1 = h[3];
                }
            }
            let mut cx2 = t1 - (t1 - t0) / 3.0;
            let mut cy2 = v1;
            if let Some(h) = h1 {
                if h[0] != UNSET {
                    cx2 = h[0];
                    cy2 = h[1];
                }
            }
            output.push(self.n(self.sec(cx1), CTime));
            output.push(self.n(cy1, kind));
            output.push(self.n(self.sec(cx2), CTime));
            output.push(self.n(cy2, kind));
        }
        Some(J::List(output))
    }

    fn value_keys(&self, t: &ModelTimeline, fallback: f64, kind: Kind) -> Vec<J> {
        let mut output = Vec::with_capacity(t.keys.len());
        for (i, k) in t.keys.iter().enumerate() {
            let mut o = self.key_base(k);
            self.put(&mut o, "value", values(k)[0], fallback, kind);
            put_curve(&mut o, self.curve_of(&t.keys, i, &[kind]));
            output.push(J::Map(o));
        }
        output
    }

    fn seq_keys(&self, t: &ModelTimeline) -> SpineResult<Vec<J>> {
        let mut output = Vec::with_capacity(t.keys.len());
        for (i, k) in t.keys.iter().enumerate() {
            let mut o = self.key_base(k);
            let v = values(k);
            let mode = SEQUENCE_MODES
                .get(v[0] as i32 as usize)
                .filter(|_| v[0] as i32 >= 0)
                .ok_or_else(|| format_err(format!("알 수 없는 시퀀스 모드: {}", v[0])))?;
            if *mode != "hold" {
                o.set("mode", J::str(mode));
            }
            if v[1] as i32 != 0 {
                o.set("index", J::Int(v[1] as i32 as i64));
            }
            let prev = if i > 0 { values(&t.keys[i - 1])[2] } else { 0.0 }; // 앞 키와 같으면 쓰지 않는다
            if v[2] != prev {
                o.set("delay", self.n(v[2], Time));
            }
            output.push(J::Map(o));
        }
        Ok(output)
    }

    fn path_keys(&self, t: &ModelTimeline) -> SpineResult<Vec<J>> {
        let c = self.constraint(&t.constraint)?;
        let (pos, spc) = path_kinds(c);
        let mut output = Vec::with_capacity(t.keys.len());
        for (i, k) in t.keys.iter().enumerate() {
            let mut o = self.key_base(k);
            let v = values(k);
            let kinds: Vec<Kind>;
            if t.type_str() == "pathMix" {
                self.put(&mut o, "mixRotate", v[0], 1.0, Mix);
                self.put(&mut o, "mixX", v[1], 1.0, Mix);
                self.put(&mut o, "mixY", v[2], v[1], Mix);
                kinds = vec![Mix, Mix, Mix];
            } else {
                let kind = if t.type_str() == "pathPosition" { pos } else { spc };
                self.put(&mut o, "value", v[0], 0.0, kind);
                kinds = vec![kind];
            }
            put_curve(&mut o, self.curve_of(&t.keys, i, &kinds));
            output.push(J::Map(o));
        }
        Ok(output)
    }

    fn bone_keys(&self, t: &ModelTimeline) -> Vec<J> {
        let ty = t.type_str();
        let d = if ty == "scale" { 1.0 } else { 0.0 };
        let ckind = if ty == "scale" { Mix } else { Pos };
        let mut output = Vec::with_capacity(t.keys.len());
        for (i, k) in t.keys.iter().enumerate() {
            let mut o = self.key_base(k);
            let v = values(k);
            let kinds: &[Kind] = if ty == "rotate" {
                self.put(&mut o, "value", v[0], 0.0, Pos);
                &[Pos]
            } else {
                self.put(&mut o, "x", v[0], d, ckind);
                self.put(&mut o, "y", v[1], d, ckind);
                if ckind == Mix {
                    &[Mix, Mix]
                } else {
                    &[Pos, Pos]
                }
            };
            put_curve(&mut o, self.curve_of(&t.keys, i, kinds));
            output.push(J::Map(o));
        }
        output
    }

    fn color_keys(&self, t: &ModelTimeline) -> SpineResult<(Vec<J>, &'static str)> {
        let slot = self
            .m
            .slots
            .iter()
            .find(|s| s.name == t.slot)
            .ok_or_else(|| format_err(format!("슬롯을 찾을 수 없습니다: {}", key(&t.slot))))?;
        let two = slot.dark.is_some();
        let mut output = Vec::with_capacity(t.keys.len());
        for (i, k) in t.keys.iter().enumerate() {
            let mut o = self.key_base(k);
            let v = values(k);
            let light = color_text(&v[..v.len().min(4)]);
            let channels;
            if two {
                o.set("light", J::Str(light));
                let rest = if v.len() > 4 { &v[4..v.len().min(7)] } else { &[][..] };
                o.set("dark", J::Str(color_text(rest)));
                channels = 7;
            } else {
                o.set("color", J::Str(light));
                channels = 4;
            }
            let kinds = vec![Pos; channels];
            put_curve(&mut o, self.curve_of(&t.keys, i, &kinds));
            output.push(J::Map(o));
        }
        Ok((output, if two { "rgba2" } else { "rgba" }))
    }

    fn att_keys(&self, t: &ModelTimeline) -> Vec<J> {
        let mut output = Vec::with_capacity(t.keys.len());
        for k in &t.keys {
            let mut o = self.key_base(k);
            if k.name.is_some() || !self.at_least(4, 1) {
                o.set("name", J::opt(&k.name));
            }
            output.push(J::Map(o));
        }
        output
    }

    fn ik_keys(&self, t: &ModelTimeline) -> SpineResult<Vec<J>> {
        let two = self.constraint(&t.constraint)?.bones.len() == 2;
        let mut output = Vec::with_capacity(t.keys.len());
        for (i, k) in t.keys.iter().enumerate() {
            let mut o = self.key_base(k);
            let v = values(k);
            self.put(&mut o, "mix", v[0], 1.0, Mix);
            self.put(&mut o, "softness", v[1], 0.0, Pos);
            if two && v[4] == 0.0 {
                o.set("bendPositive", J::Bool(false));
            }
            // 값 순서: mix, softness, stretch, compress, bend (차등 표본으로 확인)
            if v[3] != 0.0 && !two {
                o.set("compress", J::Bool(true));
            }
            if v[2] != 0.0 {
                o.set("stretch", J::Bool(true));
            }
            put_curve(&mut o, self.curve_of(&t.keys, i, &[Mix, Pos]));
            output.push(J::Map(o));
        }
        Ok(output)
    }

    fn transform_keys(&self, t: &ModelTimeline) -> SpineResult<Vec<J>> {
        let mut output = Vec::with_capacity(t.keys.len());
        let kinds = [Mix; 6];
        for (i, k) in t.keys.iter().enumerate() {
            let mut o = self.key_base(k);
            let used = self.transform_used(t, values(k))?;
            self.mixes(&mut o, &used, true);
            put_curve(&mut o, self.curve_of(&t.keys, i, &kinds));
            output.push(J::Map(o));
        }
        Ok(output)
    }

    /// 4.3: 제약에 없는 속성의 mix 는 쓰지 않는다(런타임 기본값 1로 둬도 쓰이지 않는다).
    fn transform_used(&self, t: &ModelTimeline, values: &[f64]) -> SpineResult<[f64; 6]> {
        if values.len() < 6 {
            return Err(format_err(format!("변형 제약 키의 값이 모자랍니다: {}", key(&t.constraint))));
        }
        let mut v = [0.0; 6];
        v.copy_from_slice(&values[..6]);
        if !self.v43 {
            return Ok(v);
        }
        let c = self.constraint(&t.constraint)?;
        let have = match &c.properties {
            Some(p) => target_names(p),
            None => target_names(&legacy_properties(c)),
        };
        for i in 0..6 {
            if !have.contains(PROP_NAMES[i]) {
                v[i] = if i == ModelConstraint::MIX_Y { v[ModelConstraint::MIX_X] } else { 1.0 };
            }
        }
        Ok(v)
    }

    fn deform_keys(&self, t: &ModelTimeline) -> SpineResult<Vec<J>> {
        let att = self.find_attachment(t)?;
        let setup = att.setup.as_deref().unwrap_or(&[]);
        let mut output = Vec::with_capacity(t.keys.len());
        let threshold = 0.01f32 as f64;
        for (i, k) in t.keys.iter().enumerate() {
            let mut o = self.key_base(k);
            let vec = deform_vector(att, k);
            // 앞뒤에서 설정 위치와 0.01 이내로 같은 값은 잘라낸다(편집기 규칙, float32 로 비교)
            let mut start: i64 = -1;
            let mut end: i64 = -1;
            let count = vec.len().min(setup.len());
            for j in 0..count {
                let s0 = setup[j];
                if f32r(f32r(s0 + vec[j]) - s0).abs() > threshold {
                    if start < 0 {
                        start = j as i64;
                    }
                    end = j as i64 + 1;
                }
            }
            if start >= 0 {
                if start != 0 {
                    o.set("offset", J::Int(start));
                }
                let values: Vec<J> = (start as usize..end as usize).map(|j| self.n(vec[j], Deform)).collect();
                o.set("vertices", J::List(values));
            }
            put_curve(&mut o, self.curve_of(&t.keys, i, &[Pos]));
            output.push(J::Map(o));
        }
        Ok(output)
    }

    fn find_attachment(&self, t: &ModelTimeline) -> SpineResult<&'a ModelAttachment> {
        for s in &self.m.skins {
            if s.name == t.skin {
                for row in &s.attachments {
                    if row.slot == t.slot && row.key == t.attachment {
                        if let Some(a) = &row.attachment {
                            return Ok(a);
                        }
                    }
                }
            }
        }
        Err(SpineError::Format(format!(
            "첨부를 찾을 수 없습니다: {}/{}/{}",
            key(&t.skin),
            key(&t.slot),
            key(&t.attachment)
        )))
    }

    fn draw_keys(&self, t: &ModelTimeline) -> SpineResult<Vec<J>> {
        let mut index: HashMap<&str, usize> = HashMap::new();
        for (i, s) in self.m.slots.iter().enumerate() {
            index.insert(key(&s.name), i); // 이름이 겹치면 뒤의 것(dict 와 같음)
        }
        let mut output = Vec::with_capacity(t.keys.len());
        for k in &t.keys {
            let mut o = self.key_base(k);
            let offsets = k.offsets.as_deref().unwrap_or(&[]);
            let mut changed: Vec<(usize, &(String, i64))> = Vec::with_capacity(offsets.len());
            for r in offsets {
                let at = *index
                    .get(r.0.as_str())
                    .ok_or_else(|| format_err(format!("슬롯을 찾을 수 없습니다: {}", r.0)))?;
                changed.push((at, r));
            }
            changed.sort_by_key(|(at, _)| *at);
            if !changed.is_empty() {
                let list = changed
                    .iter()
                    .map(|(_, r)| {
                        let mut e = JsonMap::new();
                        e.set("slot", J::Str(r.0.clone()));
                        e.set("offset", J::Int(r.1));
                        J::Map(e)
                    })
                    .collect();
                o.set("offsets", J::List(list));
            }
            output.push(J::Map(o));
        }
        Ok(output)
    }

    fn event_keys(&self, keys: &[&ModelKey]) -> SpineResult<Vec<J>> {
        let mut output = Vec::with_capacity(keys.len());
        for k in keys {
            let mut o = self.key_base(k);
            let e = k.event.as_ref().ok_or_else(|| format_err("이벤트 키에 값이 없습니다".to_string()))?;
            o.set("name", J::opt(&e.name));
            let data = self
                .m
                .events
                .iter()
                .find(|d| d.name == e.name)
                .ok_or_else(|| format_err(format!("이벤트를 찾을 수 없습니다: {}", key(&e.name))))?;
            if e.int != data.int {
                o.set("int", J::Int(e.int));
            }
            if e.float != data.float {
                o.set("float", self.n(e.float, Pos));
            }
            if e.string.is_some() && e.string != data.string {
                o.set("string", J::opt(&e.string));
            }
            if !is_null_or_empty(&data.audio) {
                if e.volume != data.volume {
                    o.set("volume", self.n(e.volume, Pos));
                }
                if e.balance != data.balance {
                    o.set("balance", self.n(e.balance, Pos));
                }
            }
            output.push(J::Map(o));
        }
        Ok(output)
    }
}

fn deform_vector(att: &ModelAttachment, k: &ModelKey) -> Vec<f64> {
    let dense = k.dense.as_deref().unwrap_or(&[]);
    let map = att.dense_map.as_deref().unwrap_or(&[]);
    map.iter().map(|&j| if j >= 0 && (j as usize) < dense.len() { dense[j as usize] } else { 0.0 }).collect()
}

fn sorted_of_type<'m>(cons: &'m [ModelConstraint], ty: &str) -> Vec<&'m ModelConstraint> {
    let mut v: Vec<&ModelConstraint> = cons.iter().filter(|c| eq(&c.r#type, ty)).collect();
    v.sort_by(|a, b| match (&a.name, &b.name) {
        (Some(x), Some(y)) => ordinal_cmp(x, y),
        (x, y) => x.is_some().cmp(&y.is_some()),
    });
    v
}

/// sec[key] 객체를 자연 정렬한다. depth 만큼 아래 단계 객체도 함께 정렬한다.
fn sort_child(sec: &mut JsonMap, key: &str, depth: usize) {
    let child = std::mem::take(sec.get_or_add_map(key));
    sec.set(key, J::Map(sort_deep(child, depth)));
}

fn sort_deep(map: JsonMap, depth: usize) -> JsonMap {
    let sorted = map.sorted_natural();
    if depth == 0 {
        return sorted;
    }
    let mut out = JsonMap::new();
    for (k, v) in sorted.into_entries() {
        let v = match v {
            J::Map(m) => J::Map(sort_deep(m, depth - 1)),
            other => other,
        };
        out.set(&k, v);
    }
    out
}

/// 편집기의 해시 계산법은 알 수 없다. 내용으로 정해지는 8바이트 값을 쓴다(런타임은 비교에만 쓴다).
fn hash(output: &JsonMap) -> String {
    let mut body = String::with_capacity(1 << 16);
    write_map_entries(&mut body, output.entries().iter().filter(|(k, _)| k != "skeleton").map(|(k, v)| (k, v)));
    let digest = sha1::sha1(body.as_bytes());
    sha1::base64(&digest[..8]).trim_end_matches('=').to_string()
}
