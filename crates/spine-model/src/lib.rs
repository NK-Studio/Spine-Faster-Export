//! 버전 중립 스켈레톤 모델(Spine 4.2 JSON 과 같은 이름).
//!
//! C# `NKStudio.SpineEventInjector.Export.SkeletonModel` 을 그대로 옮겼다.
//! 값은 반올림하지 않고 기본값도 모두 채운다. 무엇을 생략할지는 출력기가 버전 규칙대로 정한다.
//! 시간은 프레임 단위 그대로 둔다.
//!
//! C# 의 참조형(string, List) 중 null 이 될 수 있는 것은 `Option` 으로 옮겼다.
//! C# 에서 `= new List<..>()` 로 항상 채워지는 목록은 `Vec` 이다.

use std::fmt;

/// 내보내기 파이프라인 공통 오류. 메시지는 사용자에게 그대로 보이는 한국어 문장이다.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SpineError {
    /// 파일 형식이 예상과 다르다(C# SpineFormatException).
    Format(String),
    /// 사용자가 고칠 수 있는 잘못된 사용(C# SpineEventUsageException) — 지원하지 않는 버전 등.
    Usage(String),
    /// 지원하지 않는 기능(파이썬 ModelError / Unsupported).
    Unsupported(String),
    /// 파일 입출력 오류.
    Io(String),
}

impl SpineError {
    pub fn message(&self) -> &str {
        match self {
            SpineError::Format(m) | SpineError::Usage(m) | SpineError::Unsupported(m) | SpineError::Io(m) => m,
        }
    }
}

impl fmt::Display for SpineError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.message())
    }
}

impl std::error::Error for SpineError {}

impl From<std::io::Error> for SpineError {
    fn from(e: std::io::Error) -> Self {
        SpineError::Io(e.to_string())
    }
}

pub type SpineResult<T> = Result<T, SpineError>;

#[derive(Debug, Clone, Default)]
pub struct SkeletonModel {
    pub name: Option<String>,
    pub format: Option<String>,
    pub fps: f64,
    pub images: Option<String>,
    pub audio: Option<String>,
    pub reference_scale: f64,
    pub bounds: Option<Vec<f64>>,
    pub bones: Vec<ModelBone>,
    pub slots: Vec<ModelSlot>,
    pub constraints: Vec<ModelConstraint>,
    pub skins: Vec<ModelSkin>,
    pub events: Vec<ModelEvent>,
    pub animations: Vec<ModelAnimation>,
    /// `.skel` 문자열 표(파이썬 model.py string_table): 스켈레톤 첨부 목록(태그 6, 자리표시자 포함) 순서로
    /// 첨부 이름, 그리고 RegionAtt 13 / MeshAtt 14 경로가 비어 있지 않으면 그 경로.
    pub strings: Vec<String>,
}

#[derive(Debug, Clone, Default)]
pub struct ModelBone {
    pub name: Option<String>,
    pub parent: Option<String>,
    pub length: f64,
    pub x: f64,
    pub y: f64,
    pub rotation: f64,
    pub scale_x: f64,
    pub scale_y: f64,
    pub shear_x: f64,
    pub shear_y: f64,
    pub inherit: Option<String>,
    pub color: Option<String>,
    pub icon: Option<String>,
    pub skin: bool,
    /// 비필수: I(b,2,1) != 0
    pub visible: bool,
    /// 비필수(4.2+): F(b,33,1.0)
    pub editor33: f64,
    /// 비필수(4.2+): F(b,34,0.0)
    pub editor34: f64,
}

#[derive(Debug, Clone, Default)]
pub struct ModelSlot {
    pub name: Option<String>,
    pub bone: Option<String>,
    pub color: Option<String>,
    pub dark: Option<String>,
    pub attachment: Option<String>,
    pub blend: Option<String>,
    pub visible: bool,
}

/// 변형 제약의 대상 속성.
#[derive(Debug, Clone, Default)]
pub struct ModelToProperty {
    pub name: Option<String>,
    pub offset: f64,
    pub max: f64,
    pub scale: f64,
}

/// 변형 제약의 입력 속성과 그 대상들.
#[derive(Debug, Clone, Default)]
pub struct ModelFromProperty {
    pub name: Option<String>,
    pub offset: f64,
    pub to: Vec<ModelToProperty>,
}

/// 제약 하나. 종류별로 쓰는 필드만 채운다.
#[derive(Debug, Clone, Default)]
pub struct ModelConstraint {
    /// "ik" · "transform" · "path" · "physics" · "slider"
    pub r#type: Option<String>,
    pub name: Option<String>,
    pub order: i32,
    pub skin: bool,
    pub bones: Vec<String>,
    pub target: Option<String>,
    /// 자리는 [`ModelConstraint::MIX_ROTATE`] … [`ModelConstraint::MIX_SHEAR_Y`].
    pub mixes: [f64; 6],

    // ik · physics · slider 공통
    pub mix: f64,

    // ik
    pub softness: f64,
    pub bend_positive: bool,
    pub compress: bool,
    pub stretch: bool,
    pub uniform: bool,

    // transform (x·y·scale_x·scale_y·rotation 은 physics, path 도 함께 쓴다)
    pub rotation: f64,
    pub x: f64,
    pub y: f64,
    pub scale_x: f64,
    pub scale_y: f64,
    pub shear_y: f64,
    pub local: bool,
    pub relative: bool,
    pub properties: Option<Vec<ModelFromProperty>>,

    // path
    pub spacing_mode: Option<String>,
    pub rotate_mode: Option<String>,
    pub position_mode: Option<String>,
    pub position: f64,
    pub spacing: f64,

    // physics
    pub bone: Option<String>,
    pub rotate: f64,
    pub shear_x: f64,
    pub limit: f64,
    pub fps: f64,
    pub inertia: f64,
    pub strength: f64,
    pub damping: f64,
    pub mass: f64,
    pub wind: f64,
    pub gravity: f64,
    pub inertia_global: bool,
    pub strength_global: bool,
    pub damping_global: bool,
    pub mass_global: bool,
    pub wind_global: bool,
    pub gravity_global: bool,
    pub mix_global: bool,

    // slider
    pub animation: Option<String>,
    pub property: Option<String>,
    pub r#loop: bool,
    pub max: f64,
    pub scale: f64,
    pub offset: f64,
    /// 슬라이더: prop.F["p0"] (prop 이 없으면 0)
    pub property_offset: f64,
}

impl ModelConstraint {
    pub const MIX_ROTATE: usize = 0;
    pub const MIX_X: usize = 1;
    pub const MIX_Y: usize = 2;
    pub const MIX_SCALE_X: usize = 3;
    pub const MIX_SCALE_Y: usize = 4;
    pub const MIX_SHEAR_Y: usize = 5;
    pub const MIX_NAMES: [&'static str; 6] = ["mixRotate", "mixX", "mixY", "mixScaleX", "mixScaleY", "mixShearY"];

    pub fn type_str(&self) -> &str {
        self.r#type.as_deref().unwrap_or("")
    }
}

#[derive(Debug, Clone, Default)]
pub struct ModelSkin {
    pub name: Option<String>,
    /// 비필수(4.2+): rgba(leaf(s,8))
    pub color: Option<String>,
    pub bones: Vec<String>,
    pub constraints: Vec<String>,
    pub attachments: Vec<ModelSkinEntry>,
}

#[derive(Debug, Clone, Default)]
pub struct ModelSkinEntry {
    pub slot: Option<String>,
    pub key: Option<String>,
    pub attachment: Option<ModelAttachment>,
}

#[derive(Debug, Clone, Default)]
pub struct ModelSequence {
    pub count: i64,
    pub start: i64,
    pub digits: i64,
    pub setup: i64,
}

/// 첨부 하나. type: region · mesh · linkedmesh · path · boundingbox · clipping.
#[derive(Debug, Clone)]
pub struct ModelAttachment {
    pub name: Option<String>,
    pub key: Option<String>,
    pub r#type: Option<String>,
    /// C# 기본값은 빈 문자열.
    pub path: Option<String>,
    pub color: Option<String>,
    pub sequence: Option<ModelSequence>,

    // region
    pub x: f64,
    pub y: f64,
    pub scale_x: f64,
    pub scale_y: f64,
    pub rotation: f64,
    pub width: f64,
    pub height: f64,

    // 정점 첨부 공통: 런타임 형식 정점 배열, 조밀한(편집기) 배열 → 런타임 순서 변환표, 설정 위치
    pub vertices: Option<Vec<f64>>,
    pub weighted: bool,
    pub dense_map: Option<Vec<i32>>,
    pub setup: Option<Vec<f64>>,
    pub vertex_count: i32,

    // path
    pub closed: bool,
    pub constant_speed: bool,
    pub lengths: Option<Vec<f64>>,

    // clipping
    pub end: Option<String>,
    pub convex: bool,

    // linkedmesh
    pub parent: Option<String>,
    pub skin: Option<String>,
    pub timelines: bool,
    /// linkedmesh(4.3): Name(AttSlot(parent))
    pub parent_slot: Option<String>,

    // mesh
    pub uvs: Option<Vec<f64>>,
    pub triangles: Option<Vec<i64>>,
    pub edges: Option<Vec<i64>>,
    pub hull: i32,
}

impl Default for ModelAttachment {
    fn default() -> Self {
        ModelAttachment {
            name: None,
            key: None,
            r#type: None,
            path: Some(String::new()),
            color: None,
            sequence: None,
            x: 0.0,
            y: 0.0,
            scale_x: 0.0,
            scale_y: 0.0,
            rotation: 0.0,
            width: 0.0,
            height: 0.0,
            vertices: None,
            weighted: false,
            dense_map: None,
            setup: None,
            vertex_count: 0,
            closed: false,
            constant_speed: false,
            lengths: None,
            end: None,
            convex: false,
            parent: None,
            skin: None,
            timelines: false,
            parent_slot: None,
            uvs: None,
            triangles: None,
            edges: None,
            hull: 0,
        }
    }
}

impl ModelAttachment {
    pub fn type_str(&self) -> &str {
        self.r#type.as_deref().unwrap_or("")
    }
}

#[derive(Debug, Clone, Default)]
pub struct ModelEvent {
    pub name: Option<String>,
    pub int: i64,
    pub float: f64,
    pub string: Option<String>,
    pub audio: Option<String>,
    pub volume: f64,
    pub balance: f64,
}

#[derive(Debug, Clone, Default)]
pub struct ModelAnimation {
    pub name: Option<String>,
    /// 비필수(4.3): rgba(leaf(an,10)), 없으면 "ffffffff"
    pub color: Option<String>,
    /// I(an,9) != 0 이면 애니메이션 정리를 하지 않는다.
    pub no_clean: bool,
    pub timelines: Vec<ModelTimeline>,
}

#[derive(Debug, Clone, Default)]
pub struct ModelTimeline {
    pub r#type: Option<String>,
    pub code: i32,
    pub bone: Option<String>,
    pub slot: Option<String>,
    pub constraint: Option<String>,
    pub attachment: Option<String>,
    pub skin: Option<String>,
    pub keys: Vec<ModelKey>,
}

impl ModelTimeline {
    pub fn type_str(&self) -> &str {
        self.r#type.as_deref().unwrap_or("")
    }
}

/// 이벤트 키의 값(스켈레톤 정의와 다를 때만 JSON 에 쓴다).
#[derive(Debug, Clone, Default)]
pub struct ModelKeyEvent {
    pub name: Option<String>,
    pub int: i64,
    pub float: f64,
    pub string: Option<String>,
    pub volume: f64,
    pub balance: f64,
}

#[derive(Debug, Clone, Default)]
pub struct ModelKey {
    pub frame: f64,
    pub curve: Option<String>,
    pub values: Option<Vec<f64>>,
    /// 채널마다 곡선 손잡이 4개.
    pub handles: Option<Vec<[f64; 4]>>,
    pub name: Option<String>,
    /// 변형 키의 조밀한 값(변형 키가 아니면 None).
    pub dense: Option<Vec<f64>>,
    pub event: Option<ModelKeyEvent>,
    pub offsets: Option<Vec<(String, i64)>>,
}

/// atlas 페이지 하나. rgba 는 위쪽 줄부터, 픽셀당 4바이트(패커가 PMA 등을 적용한 최종 값).
#[derive(Debug, Clone, Default)]
pub struct AtlasPage {
    pub name: String,
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}

/// 아틀라스 결과.
#[derive(Debug, Clone, Default)]
pub struct AtlasOutput {
    pub atlas_text: String,
    pub pages: Vec<AtlasPage>,
}
