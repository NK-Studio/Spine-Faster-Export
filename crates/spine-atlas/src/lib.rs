//! 이미지 폴더 → atlas 텍스트 + 페이지 RGBA. C# `SpineAtlasPacker.cs`·`SpinePngCodec.cs` + 파이썬 `atlas_packer.py` 의 PMA/블리드/에일리어스 옵션.
//!
//! 설정은 Spine 의 Texture Packer Settings 와 같은 항목·JSON 키(`pack.json`)를 쓴다.
//! 기본값([`AtlasSettings::default`])의 결과는 C# 과 같고(여백 2·가장자리 여백·투명 테두리 잘라내기(알파 3 초과)·회전·PMA),
//! PMA/에일리어스/최대 크기 옵션은 파이썬 PoC 와 같다. 블리드는 Spine 처럼 페이지 전체에 한다(영역 사이 여백까지).
//! 페이지 안 배치는 Spine 과 같지 않을 수 있다(런타임은 atlas 좌표를 쓰므로 결과에는 영향이 없다).
//!
//! 공개 API(계약): [`AtlasSettings`], [`pack`], [`pack_scales`], [`encode_png`]. 빠진 이미지 목록이 필요하면 [`pack_detailed`].

mod natural;
mod png_codec;
mod polygon;
mod resize;

use std::collections::{HashMap, HashSet};
use std::fmt::Write as _;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use unicode_normalization::UnicodeNormalization;

use rayon::prelude::*;
use serde::{Deserialize, Serialize};
use spine_model::{AtlasOutput, AtlasPage, ModelAttachment, ModelSkin, ModelSkinEntry, SkeletonModel, SpineError, SpineResult};

pub use natural::natural_cmp;
pub use png_codec::decode_file as decode_png_file;

const SCORE_SHIFT: i64 = 16;
const BEST_NONE: i64 = 1 << 40;
const WIDTH_FACTORS: [f64; 11] = [0.8, 0.9, 1.0, 1.1, 1.2, 1.35, 1.5, 1.75, 2.0, 2.5, 3.0];

/// 텍스처 필터(atlas `filter:` 줄).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum TextureFilter {
    Nearest,
    Linear,
    MipMap,
    MipMapNearestNearest,
    MipMapLinearNearest,
    MipMapNearestLinear,
    MipMapLinearLinear,
}

/// 텍스처 랩(atlas `repeat:` 줄). Spine atlas 는 ClampToEdge 와 Repeat 만 구분한다.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum TextureWrap {
    ClampToEdge,
    Repeat,
    MirroredRepeat,
}

/// 런타임이 텍스처를 올릴 때 쓸 픽셀 형식(atlas `format:` 줄). PNG 는 늘 RGBA 로 쓴다.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum PixelFormat {
    Alpha,
    Intensity,
    LuminanceAlpha,
    RGB565,
    RGBA4444,
    RGB888,
    RGBA8888,
}

impl TextureFilter {
    fn as_str(self) -> &'static str {
        match self {
            TextureFilter::Nearest => "Nearest",
            TextureFilter::Linear => "Linear",
            TextureFilter::MipMap => "MipMap",
            TextureFilter::MipMapNearestNearest => "MipMapNearestNearest",
            TextureFilter::MipMapLinearNearest => "MipMapLinearNearest",
            TextureFilter::MipMapNearestLinear => "MipMapNearestLinear",
            TextureFilter::MipMapLinearLinear => "MipMapLinearLinear",
        }
    }
}

impl PixelFormat {
    fn as_str(self) -> &'static str {
        match self {
            PixelFormat::Alpha => "Alpha",
            PixelFormat::Intensity => "Intensity",
            PixelFormat::LuminanceAlpha => "LuminanceAlpha",
            PixelFormat::RGB565 => "RGB565",
            PixelFormat::RGBA4444 => "RGBA4444",
            PixelFormat::RGB888 => "RGB888",
            PixelFormat::RGBA8888 => "RGBA8888",
        }
    }
}

/// Spine Texture Packer Settings. JSON 키는 Spine 이 저장하는 `pack.json` 과 같다(camelCase).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct AtlasSettings {
    // Regions
    pub strip_whitespace_x: bool,
    pub strip_whitespace_y: bool,
    /// 90도 회전을 허용한다.
    pub rotation: bool,
    /// 같은 이미지를 한 자리에 합친다.
    pub alias: bool,
    /// 완전히 투명한 이미지는 atlas 에 넣지 않는다.
    pub ignore_blank_images: bool,
    /// 이 값 이하의 알파는 투명으로 보고 잘라낸다.
    pub alpha_threshold: u8,
    // Region Padding
    pub padding_x: u32,
    pub padding_y: u32,
    /// 페이지 가장자리에도 여백을 둔다.
    pub edge_padding: bool,
    /// 여백을 영역 가장자리 픽셀로 채운다.
    pub duplicate_padding: bool,
    // Pages
    pub min_width: u32,
    pub min_height: u32,
    pub max_width: u32,
    pub max_height: u32,
    /// 페이지 크기를 2의 거듭제곱으로.
    pub pot: bool,
    /// 페이지 크기를 4의 배수로(Divisible by 4).
    pub multiple_of_four: bool,
    /// 페이지를 정사각형으로.
    pub square: bool,
    // Runtime
    pub filter_min: TextureFilter,
    pub filter_mag: TextureFilter,
    pub wrap_x: TextureWrap,
    pub wrap_y: TextureWrap,
    pub format: PixelFormat,
    // Output
    /// "png" 만 지원한다.
    pub output_format: String,
    /// 격자 배치(같은 크기 칸에 차례로).
    pub grid: bool,
    /// Spine 4.x 의 Packing: "rectangles" · "grid" · "polygons". 읽을 때 "grid" 면 `grid` 를 켠다([`normalize`](Self::normalize)).
    /// "polygons" 는 메시만 쓰는 이미지를 메시가 덮는 모양대로 묶는다(`polygon` 모듈).
    pub packing: String,
    /// 미리 곱한 알파.
    #[serde(rename = "premultiplyAlpha")]
    pub pma: bool,
    /// 투명 픽셀 RGB 를 주변색으로 채움(PMA 를 끌 때만 적용).
    pub bleed: bool,
    pub bleed_iterations: u32,
    /// 배율마다 atlas 를 하나씩 만든다(비면 1).
    pub scale: Vec<f32>,
    /// 배율별 파일 이름 접미사.
    pub scale_suffix: Vec<String>,
    // Options
    pub atlas_extension: String,
    /// «이미지 폴더»로 묶을 때: 하위 폴더를 이 폴더의 페이지에 함께 묶는다(자기 `pack.json` 이 있는 하위 폴더는 따로).
    pub combine_subdirectories: bool,
    /// 이 폴더(와 하위 폴더)의 이미지를 묶지 않는다. 폴더 안 `pack.json` 에만 쓰는 값이다.
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub ignore: bool,
    /// 영역 이름에서 폴더를 뺀다.
    pub flatten_paths: bool,
    /// 이름 끝의 `_숫자` 를 `index:` 로 쓴다.
    pub use_indexes: bool,
    /// 덜 촘촘하지만 빠르게 묶는다.
    pub fast: bool,
    /// 영역 테두리를 분홍 선으로 그린다.
    pub debug: bool,
    /// 두 번째 페이지부터 파일 이름과 번호 사이에 넣는 글자(Spine `separator`, 기본 `_`): `이름.png`, `이름_2.png` …
    pub separator: String,
    /// Legacy output: 3.8 이하 런타임이 읽는 옛 atlas 형식(`rotate` · `xy` · `size` · `orig` · `offset` · `index`)으로 쓴다.
    pub legacy_output: bool,
    /// 옛 형식의 머리말을 Spine 3.8 편집기가 쓰던 글자로 쓴다(`size: 1534,529` · `filter: Linear,Linear`: 쉼표 뒤에 빈칸이 없다).
    /// Spine 4.x 의 Legacy output 은 `size: 1534, 529` 처럼 쓴다. 런타임은 둘 다 읽는다. pack.json 에는 없는 값이다.
    #[serde(skip)]
    pub legacy_editor38: bool,
}

impl Default for AtlasSettings {
    fn default() -> Self {
        AtlasSettings {
            strip_whitespace_x: true,
            strip_whitespace_y: true,
            rotation: true,
            alias: false,
            ignore_blank_images: false,
            alpha_threshold: 3,
            padding_x: 2,
            padding_y: 2,
            edge_padding: true,
            duplicate_padding: false,
            min_width: 0,
            min_height: 0,
            max_width: 4096,
            max_height: 4096,
            pot: false,
            multiple_of_four: false,
            square: false,
            filter_min: TextureFilter::Linear,
            filter_mag: TextureFilter::Linear,
            wrap_x: TextureWrap::ClampToEdge,
            wrap_y: TextureWrap::ClampToEdge,
            format: PixelFormat::RGBA8888,
            output_format: "png".into(),
            grid: false,
            packing: "rectangles".into(),
            pma: true,
            bleed: false,
            bleed_iterations: 2,
            scale: vec![1.0],
            scale_suffix: vec![String::new()],
            atlas_extension: ".atlas".into(),
            combine_subdirectories: false,
            ignore: false,
            flatten_paths: false,
            use_indexes: false,
            fast: false,
            debug: false,
            separator: "_".into(),
            legacy_output: false,
            legacy_editor38: false,
        }
    }
}

impl AtlasSettings {
    /// 최대 폭·높이를 함께 바꾼다.
    pub fn with_max_size(mut self, size: u32) -> Self {
        self.max_width = size;
        self.max_height = size;
        self
    }

    /// Spine 이 저장한 설정을 읽은 뒤: `packing` 과 `grid` 를 맞춘다(둘 중 하나라도 격자면 격자).
    pub fn normalize(mut self) -> Self {
        if self.packing.eq_ignore_ascii_case("grid") {
            self.grid = true;
        }
        self.packing = if self.grid { "grid" } else if self.packing.eq_ignore_ascii_case("polygons") { "polygons" } else { "rectangles" }.into();
        self
    }

    /// Packing: Polygons 인가.
    pub fn polygons(&self) -> bool {
        !self.grid && self.packing.eq_ignore_ascii_case("polygons")
    }

    /// 저장할 때: `packing` 을 `grid` 에 맞춘다.
    pub fn for_save(mut self) -> Self {
        if self.grid {
            self.packing = "grid".into();
        } else if self.packing.eq_ignore_ascii_case("grid") {
            self.packing = "rectangles".into();
        }
        self
    }

    /// 쓸 배율과 접미사 목록. 0 이하 배율은 뺀다. 비면 (1, "").
    pub fn scales(&self) -> Vec<(f32, String)> {
        let mut v: Vec<(f32, String)> = self
            .scale
            .iter()
            .enumerate()
            .filter(|(_, s)| s.is_finite() && **s > 0.0)
            .map(|(i, s)| (*s, self.scale_suffix.get(i).cloned().unwrap_or_default()))
            .collect();
        if v.is_empty() {
            v.push((1.0, String::new()));
        }
        v
    }

    fn check(&self) -> SpineResult<()> {
        if !self.output_format.is_empty() && !self.output_format.eq_ignore_ascii_case("png") {
            return Err(SpineError::Unsupported(format!("출력 형식 {} 은 지원하지 않습니다(PNG 만).", self.output_format)));
        }
        if self.max_width < 16 || self.max_height < 16 {
            return Err(SpineError::Usage("최대 폭·높이는 16 이상이어야 합니다.".into()));
        }
        if self.min_width > self.max_width || self.min_height > self.max_height {
            return Err(SpineError::Usage("최소 크기가 최대 크기보다 큽니다.".into()));
        }
        if self.padding_x > 1024 || self.padding_y > 1024 {
            return Err(SpineError::Usage("여백이 너무 큽니다.".into()));
        }
        let scales = self.scales();
        for (i, (_, sfx)) in scales.iter().enumerate() {
            if scales.iter().skip(i + 1).any(|(_, o)| o == sfx) {
                return Err(SpineError::Usage(format!("배율 접미사가 겹칩니다: \"{sfx}\"(배율마다 다른 접미사를 적으세요).")));
            }
        }
        Ok(())
    }
}

// ---------------------------------------------------------------- 영역

struct Region {
    /// 이미지 이름(폴더 안 경로, 확장자 없음).
    name: String,
    /// atlas 에 쓰는 이름(Flatten paths · Use indexes 적용).
    out_name: String,
    index: Option<i64>,
    orig_w: i32,
    orig_h: i32,
    left: i32,
    w: i32,
    h: i32,
    bottom_off: i32,
    pix: Vec<u8>,
    /// 에일리어스 비교 키(에일리어스를 켰을 때만).
    key: Option<AliasKey>,
    /// 다각형 패킹: 메시 삼각형이 걸친 픽셀(hull 발자국, w×h, 0 이 아니면 걸침). None 이면 사각형 전체.
    /// 자리는 이 발자국으로 잡고, 페이지에는 바깥 한 줄을 더해 옮긴다(`blit` 의 덧칠).
    mask: Option<Vec<u8>>,
    x: i32,
    y: i32,
    rotated: bool,
    /// 180도 더 돌린다(다각형 패킹의 모양 영역만): rotated 와 합쳐 `rotate:180` · `rotate:270`.
    flip: bool,
}

impl Region {
    fn stripped(&self) -> bool {
        self.w != self.orig_w || self.h != self.orig_h
    }
}

/// (left, top, w, h, 원본 w, 원본 h, 완전 투명 픽셀 RGB 를 지운 straight 픽셀, 다각형 패킹의 모양).
#[derive(PartialEq, Eq, Hash)]
struct AliasKey(i32, i32, i32, i32, i32, i32, Vec<u8>, Option<Vec<u8>>);

/// atlas 이름: Flatten paths 면 폴더를 빼고, Use indexes 면 끝의 `_숫자` 를 번호로 뗀다.
fn atlas_name(name: &str, s: &AtlasSettings) -> (String, Option<i64>) {
    let n = if s.flatten_paths { name.rsplit('/').next().unwrap_or(name) } else { name };
    if s.use_indexes {
        if let Some(pos) = n.rfind('_') {
            let digits = &n[pos + 1..];
            if !digits.is_empty() && digits.len() < 10 && digits.bytes().all(|b| b.is_ascii_digit()) {
                return (n[..pos].to_string(), digits.parse().ok());
            }
        }
    }
    (n.to_string(), None)
}

/// 투명 테두리 잘라내기(알파가 문턱값 이하인 줄·칸).
/// shape(메시만 쓰는 이미지에서 메시가 걸친 픽셀, 이미지 전체 크기)이 있으면 알파 대신 그 테두리 상자로 자른다
/// (Spine 도 데이터 내보내기에서는 메시 이미지를 메시 UV 로 자른다). keep_shape 면 그 모양을 영역에 남긴다(다각형 패킹).
/// 메시와 영역 첨부가 함께 쓰는 이미지는 shape 없이 strip 을 끄고 부른다(자르지 않는다).
/// 돌려주는 bool: 문턱값을 넘는 픽셀이 하나도 없는 이미지.
#[allow(clippy::too_many_arguments)]
fn build_region(name: String, w: i32, h: i32, rgba: &[u8], strip_x: bool, strip_y: bool, shape: Option<Vec<u8>>, keep_shape: bool, settings: &AtlasSettings) -> (Region, bool) {
    let (wu, hu) = (w as usize, h as usize);
    let th = settings.alpha_threshold;
    let (mut left, mut top, mut right, mut bottom) = (w, h, -1, -1);
    for y in 0..hu {
        let row = &rgba[y * wu * 4..(y + 1) * wu * 4];
        let Some(first) = (0..wu).find(|&x| row[x * 4 + 3] > th) else { continue };
        let last = (first..wu).rev().find(|&x| row[x * 4 + 3] > th).unwrap();
        top = top.min(y as i32);
        bottom = y as i32;
        left = left.min(first as i32);
        right = right.max(last as i32);
    }
    let blank = right < 0;
    let shape_box = shape.as_ref().and_then(|m| polygon::mask_bounds(m, wu, hu));
    let shape = shape.filter(|_| shape_box.is_some());
    let (left, right, top, bottom) = match shape_box {
        Some((l, t, r, b)) => {
            let (l, r) = if strip_x { (l, r) } else { (0, w - 1) };
            let (t, b) = if strip_y { (t, b) } else { (0, h - 1) };
            (l, r, t, b)
        }
        None => {
            // 다 투명한 이미지는 자르는 축을 1픽셀로 둔다(C# 과 같음)
            let (left, right) = if !strip_x { (0, w - 1) } else if blank { (0, 0) } else { (left, right) };
            let (top, bottom) = if !strip_y { (0, h - 1) } else if blank { (0, 0) } else { (top, bottom) };
            (left, right, top, bottom)
        }
    };
    let rw = (right - left + 1) as usize;
    let rh = (bottom - top + 1) as usize;
    let mut straight = Vec::with_capacity(rw * rh * 4);
    for y in top as usize..=bottom as usize {
        let s = (y * wu + left as usize) * 4;
        straight.extend_from_slice(&rgba[s..s + rw * 4]);
    }
    let mask = shape.map(|m| {
        let mut cut = Vec::with_capacity(rw * rh);
        for y in top as usize..=bottom as usize {
            let s = y * wu + left as usize;
            cut.extend_from_slice(&m[s..s + rw]);
        }
        cut
    });
    // 자른 뒤 모양이 사각형을 다 덮으면 사각형으로 다룬다. 다각형 패킹이 아니면 모양은 자르는 데만 쓴다.
    let mask = mask.filter(|m| keep_shape && m.contains(&0));

    let key = if settings.alias {
        let mut k = straight.clone();
        for p in k.chunks_exact_mut(4) {
            if p[3] == 0 {
                p[0] = 0;
                p[1] = 0;
                p[2] = 0;
            }
        }
        Some(AliasKey(left, top, rw as i32, rh as i32, w, h, k, mask.clone()))
    } else {
        None
    };

    let pix = if settings.pma {
        for p in straight.chunks_exact_mut(4) {
            let a = p[3] as u32;
            // 미리 곱한 알파(버림, Spine 과 같음)
            p[0] = (p[0] as u32 * a / 255) as u8;
            p[1] = (p[1] as u32 * a / 255) as u8;
            p[2] = (p[2] as u32 * a / 255) as u8;
        }
        straight
    } else {
        // 블리드는 페이지를 다 그린 뒤 한꺼번에 한다(`blit`)
        straight
    };

    let (out_name, index) = atlas_name(&name, settings);
    let region = Region {
        name,
        out_name,
        index,
        orig_w: w,
        orig_h: h,
        left,
        w: rw as i32,
        h: rh as i32,
        bottom_off: h - 1 - bottom,
        pix,
        key,
        mask,
        x: 0,
        y: 0,
        rotated: false,
        flip: false,
    };
    (region, blank)
}

/// 완전 투명 픽셀의 RGB 를 주변 8칸 평균(정수 나눗셈)으로 채운다(알파는 그대로). 다 그린 페이지에 2회 적용한다
/// (Spine 과 같다: 영역 사이 여백에도 가까운 영역의 색이 번져, 알파를 미리 곱하지 않은 텍스처의 가장자리가 어두워지지 않는다).
/// 한 회차 안에서는 앞서 채운 값을 쓰지 않고, 회차가 끝나야 다음 회차의 재료가 된다.
/// (파이썬은 "앞 회차에 새로 채운 픽셀의 이웃"만 보지만, 그 밖의 빈 픽셀은 이미 채워진 이웃이 없으므로 결과가 같다.)
fn bleed_rgb(w: usize, h: usize, mut px: Vec<u8>, iterations: usize) -> Vec<u8> {
    let mut blank: Vec<bool> = px.chunks_exact(4).map(|p| p[3] == 0).collect();
    let mut changed: Vec<(usize, [u8; 3])> = Vec::new();
    for _ in 0..iterations {
        changed.clear();
        for y in 0..h {
            for x in 0..w {
                let k = y * w + x;
                if !blank[k] {
                    continue;
                }
                let (mut r, mut g, mut b, mut c) = (0u32, 0u32, 0u32, 0u32);
                for ny in y.saturating_sub(1)..(y + 2).min(h) {
                    for nx in x.saturating_sub(1)..(x + 2).min(w) {
                        let n = ny * w + nx;
                        if n != k && !blank[n] {
                            let j = n * 4;
                            r += px[j] as u32;
                            g += px[j + 1] as u32;
                            b += px[j + 2] as u32;
                            c += 1;
                        }
                    }
                }
                if c > 0 {
                    changed.push((k, [(r / c) as u8, (g / c) as u8, (b / c) as u8]));
                }
            }
        }
        if changed.is_empty() {
            break;
        }
        for &(k, rgb) in &changed {
            px[k * 4..k * 4 + 3].copy_from_slice(&rgb);
            blank[k] = false;
        }
    }
    px
}

// ---------------------------------------------------------------- 이름 모으기

/// 첨부가 쓰는 이미지 이름(경로가 있으면 경로). region·mesh·linkedmesh 만. (정렬된 이름, 메시가 쓰는 이름 집합)
fn region_names(model: &SkeletonModel) -> (Vec<String>, HashSet<String>) {
    let mut names: HashSet<String> = HashSet::new();
    let mut mesh: HashSet<String> = HashSet::new();
    for skin in &model.skins {
        for row in &skin.attachments {
            let Some(a) = &row.attachment else { continue };
            let t = a.type_str();
            if t != "region" && t != "mesh" && t != "linkedmesh" {
                continue;
            }
            for frame in image_frames(row, a) {
                if t != "region" {
                    mesh.insert(frame.clone());
                }
                names.insert(frame);
            }
        }
    }
    let mut sorted: Vec<String> = names.into_iter().collect();
    // C#: OrderBy(Ordinal).OrderBy(Natural) — 자연 순, 같으면 Ordinal 순
    sorted.sort_by(|a, b| natural::natural_cmp(a, b).then_with(|| natural::ordinal_cmp(a, b)));
    (sorted, mesh)
}

/// 첨부 하나가 쓰는 이미지 이름(시퀀스면 프레임마다).
fn image_frames(row: &ModelSkinEntry, a: &ModelAttachment) -> Vec<String> {
    let non_empty = |s: &Option<String>| s.as_deref().filter(|s| !s.is_empty()).map(str::to_owned);
    let n = non_empty(&a.path).or_else(|| non_empty(&a.name)).or_else(|| row.key.clone()).unwrap_or_default();
    match &a.sequence {
        None => vec![n],
        Some(q) => (0..q.count.max(0)).map(|i| format!("{}{}", n, zfill(q.start + i, q.digits))).collect(),
    }
}

/// 메시의 삼각형을 UV 로(삼각형마다 u0,v0,u1,v1,u2,v2). 데이터가 없거나 어긋나면 None.
fn mesh_tris(a: &ModelAttachment) -> Option<Vec<[f64; 6]>> {
    let (uvs, tri) = (a.uvs.as_deref()?, a.triangles.as_deref()?);
    if tri.len() < 3 {
        return None;
    }
    let uv = |i: i64| -> Option<(f64, f64)> {
        let k = usize::try_from(i).ok()?.checked_mul(2)?;
        Some((*uvs.get(k)?, *uvs.get(k + 1)?))
    };
    tri.chunks_exact(3)
        .map(|t| {
            let (a, b, c) = (uv(t[0])?, uv(t[1])?, uv(t[2])?);
            Some([a.0, a.1, b.0, b.1, c.0, c.1])
        })
        .collect()
}

/// 연결 메시(linkedmesh)의 원본 메시를 찾는다: 적힌 스킨(없으면 같은 스킨 → 모든 스킨)에서 슬롯·이름이 같은 메시.
fn linked_parent<'a>(model: &'a SkeletonModel, skin: &ModelSkin, row: &ModelSkinEntry, a: &ModelAttachment) -> Option<&'a ModelAttachment> {
    let parent = a.parent.as_deref().filter(|p| !p.is_empty())?;
    let slot = a.parent_slot.as_deref().filter(|s| !s.is_empty()).or(row.slot.as_deref());
    let find = |s: &'a ModelSkin| {
        s.attachments.iter().find_map(|r| {
            let p = r.attachment.as_ref()?;
            (p.type_str() == "mesh" && r.key.as_deref() == Some(parent) && r.slot.as_deref() == slot).then_some(p)
        })
    };
    let named = a.skin.as_deref().filter(|s| !s.is_empty()).or(skin.name.as_deref());
    model.skins.iter().filter(|s| s.name.as_deref() == named).find_map(find).or_else(|| model.skins.iter().find_map(find))
}

/// 다각형 패킹용: 메시만 쓰는 이미지 이름 → 그 메시들의 삼각형(UV).
/// 영역 첨부도 쓰거나 메시 데이터를 찾지 못한 이미지는 넣지 않는다(사각형으로 묶는다).
fn mesh_shapes(model: &SkeletonModel) -> HashMap<String, Vec<[f64; 6]>> {
    let mut found: HashMap<String, Option<Vec<[f64; 6]>>> = HashMap::new();
    for skin in &model.skins {
        for row in &skin.attachments {
            let Some(a) = &row.attachment else { continue };
            let tris = match a.type_str() {
                "region" => None,
                "mesh" => mesh_tris(a),
                "linkedmesh" => linked_parent(model, skin, row, a).and_then(mesh_tris),
                _ => continue,
            };
            for frame in image_frames(row, a) {
                match (found.get_mut(&frame), &tris) {
                    (Some(Some(all)), Some(t)) => all.extend_from_slice(t),
                    (Some(slot), _) => *slot = None,
                    (None, _) => {
                        found.insert(frame, tris.clone());
                    }
                }
            }
        }
    }
    found.into_iter().filter_map(|(k, v)| v.map(|v| (k, v))).collect()
}

/// 파이썬 str.zfill: 부호는 앞에 두고 0을 채운다.
fn zfill(value: i64, width: i64) -> String {
    let s = value.to_string();
    let width = width.max(0) as usize;
    if s.len() >= width {
        return s;
    }
    match s.strip_prefix('-') {
        Some(rest) => format!("-{}{}", "0".repeat(width - s.len()), rest),
        None => format!("{}{}", "0".repeat(width - s.len()), s),
    }
}

// ---------------------------------------------------------------- MaxRects

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
struct Rect {
    x: i32,
    y: i32,
    w: i32,
    h: i32,
}

#[derive(Clone, Copy)]
struct Placed {
    idx: usize,
    x: i32,
    y: i32,
    rotated: bool,
    flip: bool,
}

#[inline]
fn score(dw: i64, dh: i64) -> i64 {
    if dw < dh {
        (dw << SCORE_SHIFT) | dh
    } else {
        (dh << SCORE_SHIFT) | dw
    }
}

/// 여백·회전 설정.
#[derive(Clone, Copy)]
struct Geo {
    px: i32,
    py: i32,
    /// 가장자리 여백(Edge padding 이면 px·py, 아니면 0).
    ex: i32,
    ey: i32,
    rotation: bool,
}

impl Geo {
    fn of(s: &AtlasSettings) -> Geo {
        let (px, py) = (s.padding_x as i32, s.padding_y as i32);
        let (ex, ey) = if s.edge_padding { (px, py) } else { (0, 0) };
        Geo { px, py, ex, ey, rotation: s.rotation }
    }
}

/// MaxRects(짧은 변 맞춤). 좌표·크기에 여백 포함. (놓인 것, 못 놓은 것)
fn pack_page(order: &[usize], regions: &[Region], width: i32, height: i32, g: Geo) -> (Vec<Placed>, Vec<usize>) {
    // 영역은 오른쪽·아래 여백까지 차지한다. 마지막 영역의 여백은 가장자리 여백이 되거나(Edge) 페이지 밖으로 나간다.
    let mut free = vec![Rect { x: g.ex, y: g.ey, w: width - 2 * g.ex + g.px, h: height - 2 * g.ey + g.py }];
    let mut next: Vec<Rect> = Vec::new();
    let mut is_piece: Vec<bool> = Vec::new();
    let mut pieces: Vec<usize> = Vec::new();
    let mut drop: Vec<bool> = Vec::new();
    let mut placed = Vec::new();
    let mut left = Vec::new();
    for &idx in order {
        let r = &regions[idx];
        let (w0, h0) = (r.w + g.px, r.h + g.py);
        let (rw0, rh0) = (r.h + g.px, r.w + g.py);
        let mut best = BEST_NONE;
        let mut spot = Rect { x: 0, y: 0, w: 0, h: 0 };
        for f in &free {
            // 점수 (짧은 변 여유, 긴 변 여유, 회전)을 정수 하나로 합쳐 비교한다
            if w0 <= f.w && h0 <= f.h {
                let key = score((f.w - w0) as i64, (f.h - h0) as i64) << 1;
                if key < best {
                    best = key;
                    spot = *f;
                }
            }
            if g.rotation && rw0 <= f.w && rh0 <= f.h {
                let key = (score((f.w - rw0) as i64, (f.h - rh0) as i64) << 1) | 1;
                if key < best {
                    best = key;
                    spot = *f;
                }
            }
            if best == 0 {
                break; // 빈틈 없이 맞는 자리는 더 찾지 않는다
            }
        }
        if best == BEST_NONE {
            left.push(idx);
            continue;
        }
        let rotated = best & 1 != 0;
        let (x, y) = (spot.x, spot.y);
        let (w, h) = if rotated { (rw0, rh0) } else { (w0, h0) };
        placed.push(Placed { idx, x, y, rotated, flip: false });
        split_free(&mut free, &mut next, &mut pieces, &mut is_piece, &mut drop, x, y, x + w, y + h);
    }
    (placed, left)
}

/// 겹치는 빈칸만 쪼갠다. 안 겹친 빈칸끼리는 이미 서로를 포함하지 않으므로 새 조각만 포함 검사를 한다.
#[allow(clippy::too_many_arguments)]
fn split_free(
    free: &mut Vec<Rect>,
    next: &mut Vec<Rect>,
    pieces: &mut Vec<usize>,
    is_piece: &mut Vec<bool>,
    drop: &mut Vec<bool>,
    x: i32,
    y: i32,
    right: i32,
    bottom: i32,
) {
    next.clear();
    pieces.clear();
    for &f in free.iter() {
        let fr = f.x + f.w;
        let fb = f.y + f.h;
        if x >= fr || right <= f.x || y >= fb || bottom <= f.y {
            next.push(f);
            continue;
        }
        if x > f.x {
            pieces.push(next.len());
            next.push(Rect { x: f.x, y: f.y, w: x - f.x, h: f.h });
        }
        if right < fr {
            pieces.push(next.len());
            next.push(Rect { x: right, y: f.y, w: fr - right, h: f.h });
        }
        if y > f.y {
            pieces.push(next.len());
            next.push(Rect { x: f.x, y: f.y, w: f.w, h: y - f.y });
        }
        if bottom < fb {
            pieces.push(next.len());
            next.push(Rect { x: f.x, y: bottom, w: f.w, h: fb - bottom });
        }
    }
    if pieces.is_empty() {
        std::mem::swap(free, next);
        return;
    }
    is_piece.clear();
    is_piece.resize(next.len(), false);
    for &i in pieces.iter() {
        is_piece[i] = true;
    }
    drop.clear();
    drop.resize(next.len(), false);
    let mut any = false;
    for &i in pieces.iter() {
        let a = next[i];
        let ar = a.x + a.w;
        let ab = a.y + a.h;
        for (j, b) in next.iter().enumerate() {
            if a.x >= b.x
                && a.y >= b.y
                && ar <= b.x + b.w
                && ab <= b.y + b.h
                && i != j
                && (a != *b || i > j || !is_piece[j])
            {
                drop[i] = true;
                any = true;
                break;
            }
        }
    }
    free.clear();
    if any {
        free.extend(next.iter().zip(drop.iter()).filter(|(_, &d)| !d).map(|(r, _)| *r));
    } else {
        std::mem::swap(free, next);
    }
}

struct Page {
    w: i32,
    h: i32,
    /// 놓인 순서대로의 영역 번호.
    regions: Vec<usize>,
}

/// 쓴 크기 → 페이지 크기(최소 크기 · 2의 거듭제곱 · 정사각형). 최대 크기는 넘지 않는다.
fn page_size(used_w: i32, used_h: i32, s: &AtlasSettings, max_w: i32, max_h: i32) -> (i32, i32) {
    let mut w = used_w.max(s.min_width as i32).max(1);
    let mut h = used_h.max(s.min_height as i32).max(1);
    if s.pot {
        w = (w as u32).next_power_of_two() as i32;
        h = (h as u32).next_power_of_two() as i32;
    }
    if s.multiple_of_four {
        w = (w + 3) / 4 * 4;
        h = (h + 3) / 4 * 4;
    }
    if s.square {
        w = w.max(h);
        h = w;
    }
    // 최대 크기가 4의 배수가 아니면 4의 배수로 내린다(영역이 안 들어가면 그대로 둔다).
    let fit = |v: i32, max: i32, used: i32| if s.multiple_of_four && v > max && max / 4 * 4 >= used { max / 4 * 4 } else { v.min(max) };
    (fit(w, max_w, used_w), fit(h, max_h, used_h))
}

fn page_limits(s: &AtlasSettings) -> (i32, i32) {
    let mw = s.max_width.min(i32::MAX as u32 / 4) as i32;
    let mh = s.max_height.min(i32::MAX as u32 / 4) as i32;
    if s.square {
        (mw.min(mh), mw.min(mh))
    } else {
        (mw, mh)
    }
}

fn too_big(regions: &[Region], idx: usize, max_w: i32, max_h: i32) -> SpineError {
    SpineError::Format(format!("{}×{} 페이지에 들어가지 않는 이미지가 있습니다: {}", max_w, max_h, regions[idx].name))
}

/// 여러 폭을 시험해 넓이가 가장 작은 배치를 고른다. 한 장에 안 들어가면 페이지를 나눈다.
/// 폭마다의 시험은 서로 독립이라 병렬로 돌리고, 고르는 것은 폭 순서대로 한다(결과는 순차와 같다).
fn pack_pages(regions: &mut [Region], packed: &[usize], s: &AtlasSettings) -> SpineResult<Vec<Page>> {
    let g = Geo::of(s);
    let (max_w, max_h) = page_limits(s);
    let mut todo: Vec<usize> = packed.to_vec();
    todo.sort_by(|&a, &b| {
        let (ra, rb) = (&regions[a], &regions[b]);
        rb.w.max(rb.h)
            .cmp(&ra.w.max(ra.h))
            .then(rb.w.min(rb.h).cmp(&ra.w.min(ra.h)))
            .then_with(|| natural::ordinal_cmp(&ra.name, &rb.name))
    });
    let factors: &[f64] = if s.fast { &[1.0] } else { &WIDTH_FACTORS };
    let mut pages = Vec::new();
    while !todo.is_empty() {
        let area: i64 = todo.iter().map(|&i| (regions[i].w + g.px) as i64 * (regions[i].h + g.py) as i64).sum();
        let dim = |i: usize| if g.rotation { regions[i].w.max(regions[i].h) } else { regions[i].w };
        let widest = todo.iter().map(|&i| dim(i) + 2 * g.ex).max().unwrap();
        let side = widest.max((area as f64).sqrt() as i32);
        let mut widths: Vec<i32> = factors.iter().map(|f| max_w.min(widest.max((side as f64 * f) as i32))).collect();
        widths.sort_unstable();
        widths.dedup();

        let regs: &[Region] = regions;
        let trials: Vec<(Vec<Placed>, Vec<usize>)> =
            widths.par_iter().map(|&wide| pack_page(&todo, regs, wide, max_h, g)).collect();

        let mut best: Option<(usize, i64, i32, i32, usize)> = None; // (못 놓은 수, 넓이, w, h, 시험 번호)
        for (t, (placed, left)) in trials.iter().enumerate() {
            if placed.is_empty() {
                continue;
            }
            let used_w = placed.iter().map(|p| p.x + if p.rotated { regs[p.idx].h } else { regs[p.idx].w }).max().unwrap() + g.ex;
            let used_h = placed.iter().map(|p| p.y + if p.rotated { regs[p.idx].w } else { regs[p.idx].h }).max().unwrap() + g.ey;
            let (pw, ph) = page_size(used_w, used_h, s, max_w, max_h);
            let page_area = pw as i64 * ph as i64;
            let better = match best {
                None => true,
                Some((bl, ba, ..)) => left.len() < bl || (left.len() == bl && page_area < ba),
            };
            if better {
                best = Some((left.len(), page_area, pw, ph, t));
            }
        }
        let Some((_, _, bw, bh, t)) = best else {
            return Err(too_big(regions, todo[0], max_w, max_h));
        };
        let (placed, left) = trials.into_iter().nth(t).unwrap();
        for p in &placed {
            let r = &mut regions[p.idx];
            r.x = p.x;
            r.y = p.y;
            r.rotated = p.rotated;
            r.flip = p.flip;
        }
        pages.push(Page { w: bw, h: bh, regions: placed.iter().map(|p| p.idx).collect() });
        todo = left;
    }
    Ok(pages)
}

/// 격자 배치: 가장 큰 영역 크기의 칸에 이름 순으로 왼쪽→오른쪽, 위→아래. 회전하지 않는다.
fn pack_grid(regions: &mut [Region], packed: &[usize], s: &AtlasSettings) -> SpineResult<Vec<Page>> {
    let g = Geo::of(s);
    let (max_w, max_h) = page_limits(s);
    let cell_w = packed.iter().map(|&i| regions[i].w).max().unwrap_or(1) + g.px;
    let cell_h = packed.iter().map(|&i| regions[i].h).max().unwrap_or(1) + g.py;
    let cols = (max_w - 2 * g.ex + g.px) / cell_w;
    let rows = (max_h - 2 * g.ey + g.py) / cell_h;
    if cols < 1 || rows < 1 {
        let big = *packed.iter().max_by_key(|&&i| regions[i].w.max(regions[i].h)).unwrap();
        return Err(too_big(regions, big, max_w, max_h));
    }
    let per_page = (cols * rows) as usize;
    let mut pages = Vec::new();
    for chunk in packed.chunks(per_page) {
        for (k, &i) in chunk.iter().enumerate() {
            let (c, r) = ((k as i32) % cols, (k as i32) / cols);
            regions[i].x = g.ex + c * cell_w;
            regions[i].y = g.ey + r * cell_h;
            regions[i].rotated = false;
            regions[i].flip = false;
        }
        let n = chunk.len() as i32;
        let used_cols = n.min(cols);
        let used_rows = (n + cols - 1) / cols;
        let used_w = g.ex + used_cols * cell_w - g.px + g.ex;
        let used_h = g.ey + used_rows * cell_h - g.py + g.ey;
        let (w, h) = page_size(used_w, used_h, s, max_w, max_h);
        pages.push(Page { w, h, regions: chunk.to_vec() });
    }
    Ok(pages)
}

/// 페이지 위 영역 사각형(회전 반영).
fn page_rect(r: &Region) -> (usize, usize, usize, usize) {
    let (w, h) = if r.rotated { (r.h, r.w) } else { (r.w, r.h) };
    (r.x as usize, r.y as usize, w as usize, h as usize)
}

fn blit(page_w: i32, page_h: i32, ids: &[usize], regions: &[Region], s: &AtlasSettings) -> Vec<u8> {
    let pw = page_w as usize;
    let ph = page_h as usize;
    let mut out = vec![0u8; pw * ph * 4];
    for &i in ids {
        let r = &regions[i];
        let (rw, rh, rx, ry) = (r.w as usize, r.h as usize, r.x as usize, r.y as usize);
        if let Some(hull) = &r.mask {
            // 다각형 패킹: 발자국만 옮긴다(그 밖은 다른 영역의 자리일 수 있다). 바깥 한 줄은 아래에서 덧칠한다.
            for y in 0..rh {
                for (a, b) in polygon::runs(&hull[y * rw..(y + 1) * rw]) {
                    if !r.rotated && !r.flip {
                        let d = ((ry + y) * pw + rx + a) * 4;
                        out[d..d + (b - a + 1) * 4].copy_from_slice(&r.pix[(y * rw + a) * 4..(y * rw + b + 1) * 4]);
                    } else {
                        for x in a..=b {
                            let (tx, ty) = polygon::turn(rw, rh, r.rotated, r.flip, x, y);
                            let d = ((ry + ty) * pw + rx + tx) * 4;
                            out[d..d + 4].copy_from_slice(&r.pix[(y * rw + x) * 4..(y * rw + x + 1) * 4]);
                        }
                    }
                }
            }
        } else if !r.rotated {
            for y in 0..rh {
                let d = ((ry + y) * pw + rx) * 4;
                out[d..d + rw * 4].copy_from_slice(&r.pix[y * rw * 4..(y + 1) * rw * 4]);
            }
        } else {
            // 90도 회전: 원본 (x, y) → 페이지 (r.x + y, r.y + w - 1 - x)
            for x in 0..rw {
                let row = (ry + rw - 1 - x) * pw + rx;
                let dst = &mut out[row * 4..(row + rh) * 4];
                for y in 0..rh {
                    let s = (y * rw + x) * 4;
                    dst[y * 4..y * 4 + 4].copy_from_slice(&r.pix[s..s + 4]);
                }
            }
        }
    }
    // 덧칠: 모양의 바깥 한 줄(선형 필터가 읽는 줄)과 사각형의 duplicate padding.
    // 모양은 이웃과 빈 픽셀 한 줄까지 붙을 수 있다. 영역마다 제 바로 바깥 한 줄을 읽으므로(선형 필터),
    // 두 영역의 바깥 줄이 만나는 픽셀은 비워 두고 한 영역만 읽는 픽셀에만 그 영역의 픽셀을 쓴다
    // (어느 쪽도 남의 픽셀을 읽지 않게).
    let margin = polygon::shape_margin(s.padding_x as i32, s.padding_y as i32);
    let (ax, ay) = if s.duplicate_padding { ((s.padding_x / 2) as usize, (s.padding_y / 2) as usize) } else { (0, 0) };
    // (영역, 바깥 한 줄: 발자국을 (1,1) 에 둔 (w+2)×(h+2) 배열에서 한 번 넓혀 발자국을 뺀 것 — 영역 사각형 밖 한 줄까지 본다)
    let rings: Vec<(usize, Vec<u8>)> = if margin > 0 {
        ids.iter()
            .filter_map(|&i| {
                let r = &regions[i];
                let hull = r.mask.as_ref()?;
                let (w, h) = (r.w as usize, r.h as usize);
                let mut base = vec![0u8; (w + 2) * (h + 2)];
                for y in 0..h {
                    base[(y + 1) * (w + 2) + 1..(y + 1) * (w + 2) + 1 + w].copy_from_slice(&hull[y * w..(y + 1) * w]);
                }
                let mut ring = polygon::widen(&base, w + 2, h + 2, 1);
                ring.iter_mut().zip(&base).for_each(|(m, &b)| *m = (*m != 0 && b == 0) as u8);
                Some((i, ring))
            })
            .collect()
    } else {
        Vec::new()
    };
    // 모양대로 놓은 영역은 여백이 다른 영역의 자리일 수 있어 늘리지 않는다(Spine 과 같음)
    let rects = || ids.iter().filter(|&&i| regions[i].mask.is_none()).map(|&i| page_rect(&regions[i]));
    // 바깥 줄의 픽셀마다: (페이지 픽셀 번호, 영역 안이면 그 픽셀의 pix 위치)
    let ring_spots = |i: usize, ring: &[u8], each: &mut dyn FnMut(usize, Option<usize>)| {
        let r = &regions[i];
        let (w, h) = (r.w as i64, r.h as i64);
        for ey in 0..h + 2 {
            for ex in (0..w + 2).filter(|&ex| ring[(ey * (w + 2) + ex) as usize] != 0) {
                let (x, y) = (ex - 1, ey - 1);
                // polygon::turn 과 같은 규칙(영역 밖 좌표까지)
                let (tx, ty) = match (r.rotated, r.flip) {
                    (false, false) => (x, y),
                    (true, false) => (y, w - 1 - x),
                    (false, true) => (w - 1 - x, h - 1 - y),
                    (true, true) => (h - 1 - y, x),
                };
                let (px, py) = (r.x as i64 + tx, r.y as i64 + ty);
                if px < 0 || py < 0 || px >= pw as i64 || py >= ph as i64 {
                    continue;
                }
                let inside = x >= 0 && y >= 0 && x < w && y < h;
                each(py as usize * pw + px as usize, inside.then(|| ((y * w + x) * 4) as usize));
            }
        }
    };
    if rings.is_empty() {
        for rect in rects() {
            duplicate_edges(&mut out, pw, ph, rect, ax, ay, None);
        }
    } else {
        // 빈 픽셀마다 그 픽셀을 읽는 영역 수
        let mut claims = vec![0u8; pw * ph];
        for (i, ring) in &rings {
            ring_spots(*i, ring, &mut |at, _| claims[at] = claims[at].saturating_add(1));
        }
        for rect in rects() {
            padding_frame(pw, ph, rect, ax.max(1), ay.max(1), &mut |at, _| claims[at] = claims[at].saturating_add(1));
        }
        for (i, ring) in &rings {
            let pix = &regions[*i].pix;
            ring_spots(*i, ring, &mut |at, src| {
                if let Some(src) = src.filter(|_| claims[at] == 1) {
                    out[at * 4..at * 4 + 4].copy_from_slice(&pix[src..src + 4]);
                }
            });
        }
        for rect in rects() {
            duplicate_edges(&mut out, pw, ph, rect, ax, ay, Some(&claims));
        }
    }
    // 블리드는 PMA 를 끌 때만 의미가 있다(Spine 도 PMA 켜짐에서는 결과가 같다)
    if s.bleed && !s.pma {
        out = bleed_rgb(pw, ph, out, s.bleed_iterations as usize);
    }
    if s.debug {
        for &i in ids {
            match &regions[i].mask {
                Some(_) => draw_shape(&mut out, pw, &regions[i]),
                None => draw_rect(&mut out, pw, ph, page_rect(&regions[i])),
            }
        }
        draw_rect(&mut out, pw, ph, (0, 0, pw, ph));
    }
    out
}

/// 사각형을 가로 ax · 세로 ay 만큼 넓힌 테두리(사각형 밖, 페이지 안)의 픽셀마다:
/// (그 픽셀의 번호, 가장 가까운 사각형 안 픽셀의 번호).
fn padding_frame(pw: usize, ph: usize, (x, y, w, h): (usize, usize, usize, usize), ax: usize, ay: usize, each: &mut dyn FnMut(usize, usize)) {
    if w == 0 || h == 0 || (ax == 0 && ay == 0) {
        return;
    }
    let (x1, y1) = (x + w - 1, y + h - 1);
    for row in y.saturating_sub(ay)..=(y1 + ay).min(ph - 1) {
        for col in x.saturating_sub(ax)..=(x1 + ax).min(pw - 1) {
            if (x..=x1).contains(&col) && (y..=y1).contains(&row) {
                continue;
            }
            each(row * pw + col, row.clamp(y, y1) * pw + col.clamp(x, x1));
        }
    }
}

/// Duplicate padding: 영역 가장자리 픽셀을 여백 절반만큼 바깥으로 늘린다(모서리 포함).
/// claims 가 있으면 이 영역만 노리는 픽셀(값이 1)에만 쓴다.
fn duplicate_edges(out: &mut [u8], pw: usize, ph: usize, rect: (usize, usize, usize, usize), ax: usize, ay: usize, claims: Option<&[u8]>) {
    padding_frame(pw, ph, rect, ax, ay, &mut |to, from| {
        if claims.is_none_or(|c| c[to] == 1) {
            out.copy_within(from * 4..from * 4 + 4, to * 4);
        }
    });
}

/// Debug: 모양의 테두리를 분홍 선으로(다각형 패킹).
fn draw_shape(out: &mut [u8], pw: usize, r: &Region) {
    let Some(m) = &r.mask else { return };
    let (w, h) = (r.w as usize, r.h as usize);
    let on = |x: usize, y: usize| m[y * w + x] != 0;
    for y in 0..h {
        for x in 0..w {
            let edge = on(x, y) && (x == 0 || y == 0 || x + 1 == w || y + 1 == h || !on(x - 1, y) || !on(x + 1, y) || !on(x, y - 1) || !on(x, y + 1));
            if edge {
                let (tx, ty) = polygon::turn(w, h, r.rotated, r.flip, x, y);
                let o = ((r.y as usize + ty) * pw + r.x as usize + tx) * 4;
                out[o..o + 4].copy_from_slice(&[255, 0, 255, 255]);
            }
        }
    }
}

/// Debug: 사각형 테두리를 분홍 선으로.
fn draw_rect(out: &mut [u8], pw: usize, ph: usize, (x, y, w, h): (usize, usize, usize, usize)) {
    if w == 0 || h == 0 {
        return;
    }
    let mut put = |px: usize, py: usize| {
        if px < pw && py < ph {
            let o = (py * pw + px) * 4;
            out[o..o + 4].copy_from_slice(&[255, 0, 255, 255]);
        }
    };
    for i in x..x + w {
        put(i, y);
        put(i, y + h - 1);
    }
    for j in y..y + h {
        put(x, j);
        put(x + w - 1, j);
    }
}

// ---------------------------------------------------------------- 진입점

/// [`pack_detailed`] 의 결과: atlas + 이미지 폴더에 없어 투명한 1픽셀로 둔 영역 이름(C# `AtlasMessage`).
#[derive(Debug, Clone, Default)]
pub struct PackResult {
    pub output: AtlasOutput,
    pub missing: Vec<String>,
    /// Ignore blank images 로 뺀 영역 이름.
    pub ignored: Vec<String>,
}

/// name: 페이지 PNG 이름의 바탕(예: "spineboy" → 한 장이면 "spineboy.png", 여러 장이면 Spine 처럼 "spineboy.png", "spineboy_2.png", "spineboy_3.png" …).
/// 폴더에 없는 이미지는 Spine 에디터처럼 그리지 않는다(투명한 1픽셀 영역으로 두어 런타임이 첨부를 찾게 한다).
/// 배율 목록 중 첫 배율만 쓴다(모든 배율은 [`pack_scales`]).
pub fn pack(model: &SkeletonModel, images_dir: &Path, name: &str, settings: &AtlasSettings) -> SpineResult<AtlasOutput> {
    pack_detailed(model, images_dir, name, settings).map(|r| r.output)
}

/// 페이지를 PNG 바이트로.
pub fn encode_png(page: &AtlasPage) -> Vec<u8> {
    png_codec::encode(page.width, page.height, &page.rgba)
}

/// [`pack`] 과 같고, 빠지거나 뺀 영역 이름도 돌려준다.
pub fn pack_detailed(model: &SkeletonModel, images_dir: &Path, name: &str, settings: &AtlasSettings) -> SpineResult<PackResult> {
    let (_, suffix) = settings.scales().remove(0);
    let sources = attachment_sources(model, images_dir)?;
    pack_at(&sources, &format!("{name}{suffix}"), settings, &suffix)
}

/// 배율마다 하나씩: (접미사, 결과). 파일 이름은 `name + 접미사`.
pub fn pack_scales(model: &SkeletonModel, images_dir: &Path, name: &str, settings: &AtlasSettings) -> SpineResult<Vec<(String, PackResult)>> {
    pack_sources_scales(&attachment_sources(model, images_dir)?, name, settings)
}

/// [`pack_scales`] 와 같지만 묶을 이미지를 직접 준다(이미지 폴더째 묶기, 여러 스켈레톤을 한 atlas 로).
/// 폴더 설정([`Source::settings`])이 있는 이미지는 그 설정과 배율로 묶는다. atlas 는 접미사마다 하나이고
/// (settings 의 배율 다음에 폴더에만 있는 배율 순), 폴더는 자기 배율 목록에 그 접미사가 있을 때만 들어간다.
pub fn pack_sources_scales(sources: &[Source], name: &str, settings: &AtlasSettings) -> SpineResult<Vec<(String, PackResult)>> {
    settings.check()?;
    let mut suffixes: Vec<String> = settings.scales().into_iter().map(|(_, s)| s).collect();
    let mut seen: Vec<&AtlasSettings> = Vec::new();
    for s in sources.iter().filter_map(|s| s.settings.as_deref()) {
        if seen.contains(&s) {
            continue;
        }
        s.check()?;
        seen.push(s);
        for (_, sfx) in s.scales() {
            if !suffixes.contains(&sfx) {
                suffixes.push(sfx);
            }
        }
    }
    suffixes.into_iter().map(|suffix| pack_at(sources, &format!("{name}{suffix}"), settings, &suffix).map(|r| (suffix, r))).collect()
}

/// atlas 에 넣을 이미지 하나.
#[derive(Debug, Clone, PartialEq)]
pub struct Source {
    /// 영역 이름(이미지 폴더 안 경로, 확장자 없음).
    pub name: String,
    /// PNG 파일. 없으면 투명한 1픽셀 영역으로 둔다.
    pub path: PathBuf,
    /// 메시가 쓰는 이미지(여백을 잘라내지 않는다).
    pub mesh: bool,
    /// 페이지를 따로 쓰는 묶음(이미지 폴더 안 폴더 경로, 끝에 `/`, 이미지 폴더 자신은 ""). None 이면 이미지 폴더 자신과 같은 묶음.
    /// 묶음은 Spine 처럼 폴더 나무를 깊이 우선(이름은 Ordinal 순, 부모 먼저)으로 쓴다.
    pub group: Option<String>,
    /// 이 묶음의 설정(폴더 안 `pack.json` 을 부모 설정에 덮은 것). None 이면 묶을 때 준 설정.
    pub settings: Option<Arc<AtlasSettings>>,
    /// 다각형 패킹용: 이 이미지를 메시만 쓸 때 그 메시들의 삼각형(UV 0~1, 왼쪽 위 원점, 삼각형마다 u0,v0,u1,v1,u2,v2).
    /// 비어 있으면 사각형으로 묶는다.
    pub tris: Vec<[f64; 6]>,
}

fn sort_sources(sources: &mut [Source]) {
    // C#: OrderBy(Ordinal).OrderBy(Natural) — 자연 순, 같으면 Ordinal 순
    sources.sort_by(|a, b| natural::natural_cmp(&a.name, &b.name).then_with(|| natural::ordinal_cmp(&a.name, &b.name)));
}

fn need_images_dir(images_dir: &Path) -> SpineResult<()> {
    if images_dir.is_dir() {
        Ok(())
    } else {
        Err(SpineError::Format(format!("이미지 폴더가 없습니다: {}", images_dir.display())))
    }
}

/// Pack source «Attachments»: 스켈레톤의 첨부가 쓰는 이미지만.
pub fn attachment_sources(model: &SkeletonModel, images_dir: &Path) -> SpineResult<Vec<Source>> {
    let (names, mesh) = region_names(model);
    if names.is_empty() {
        return Err(SpineError::Format("묶을 이미지가 없습니다".into()));
    }
    need_images_dir(images_dir)?;
    let mut shapes = mesh_shapes(model);
    Ok(names
        .into_iter()
        .map(|name| Source {
            path: images_dir.join(format!("{name}.png")),
            mesh: mesh.contains(&name),
            tris: shapes.remove(&name).unwrap_or_default(),
            name,
            group: None,
            settings: None,
        })
        .collect())
}

/// Spine `pack.json` 을 base 위에 덮는다: 파일에 있는 키만 바꾸고 나머지는 base 를 물려받는다
/// (Spine 은 폴더 안 `pack.json` 을 가장 가까운 윗폴더의 설정에 덮어 읽는다).
pub fn merge_pack_json(base: &AtlasSettings, json: &str) -> SpineResult<AtlasSettings> {
    let bad = |e: serde_json::Error| SpineError::Format(format!("pack.json 을 읽지 못했습니다: {e}"));
    let over: serde_json::Value = serde_json::from_str(json).map_err(bad)?;
    let Some(over) = over.as_object() else {
        return Err(SpineError::Format("pack.json 은 JSON 객체여야 합니다".into()));
    };
    let mut value = serde_json::to_value(base.clone().for_save()).map_err(bad)?;
    let map = value.as_object_mut().expect("설정은 객체로 바뀐다");
    // 격자는 `grid` 와 `packing` 두 키로 적힌다: 한쪽만 있으면 그쪽을 따른다
    match (over.contains_key("grid"), over.contains_key("packing")) {
        (true, false) if !base.polygons() => {
            map.insert("packing".into(), "rectangles".into());
        }
        (false, true) => {
            map.insert("grid".into(), false.into());
        }
        _ => {}
    }
    for (k, v) in over {
        map.insert(k.clone(), v.clone());
    }
    let merged: AtlasSettings = serde_json::from_value(value).map_err(bad)?;
    Ok(AtlasSettings { legacy_editor38: base.legacy_editor38, ..merged.normalize() })
}

fn read_pack_json(dir: &Path, base: &AtlasSettings) -> SpineResult<Option<AtlasSettings>> {
    let file = dir.join("pack.json");
    if !file.is_file() {
        return Ok(None);
    }
    let text = std::fs::read_to_string(&file)?;
    merge_pack_json(base, &text).map(Some).map_err(|e| SpineError::Format(format!("{}: {e}", file.display())))
}

/// Pack source «Image folders»: 스켈레톤 이미지 폴더의 PNG 를 하위 폴더까지 모두. Spine 처럼 폴더 안 `pack.json` 을 읽는다.
/// - 폴더의 설정은 가장 가까운 윗폴더의 설정(이미지 폴더 자신은 settings)에 그 폴더의 `pack.json` 을 덮은 것이다.
/// - 폴더마다 페이지를 따로 쓴다. Combine subdirectories 가 켜진 폴더는 하위 폴더를 자기 페이지에 함께 묶되,
///   `pack.json` 이 있는 하위 폴더(와 그 아래)는 따로 묶는다.
/// - `"ignore": true` 인 폴더의 이미지는 넣지 않는다(하위 폴더는 자기 `pack.json` 이 풀지 않으면 함께 빠진다).
pub fn folder_sources(model: &SkeletonModel, images_dir: &Path, settings: &AtlasSettings) -> SpineResult<Vec<Source>> {
    need_images_dir(images_dir)?;
    let (_, mesh) = region_names(model);
    let mut shapes = mesh_shapes(model);
    let root = Arc::new(read_pack_json(images_dir, settings)?.unwrap_or_else(|| settings.clone()));
    let mut out = Vec::new();
    // (폴더, 폴더 안 경로, 묶음 경로, 묶음 설정)
    let mut stack = vec![(images_dir.to_path_buf(), String::new(), String::new(), root)];
    while let Some((dir, prefix, group, unit)) = stack.pop() {
        for entry in std::fs::read_dir(&dir)?.filter_map(|e| e.ok()) {
            let path = entry.path();
            // 영역 이름은 NFC 로: macOS 는 한글 파일 이름을 NFD 로 돌려주지만 Spine 은 NFC 로 쓰고, 첨부 이름도 NFC 다
            let Some(file) = path.file_name().and_then(|f| f.to_str()).map(|f| f.nfc().collect::<String>()) else { continue };
            if file.starts_with('.') {
                continue;
            }
            if path.is_dir() {
                let sub = format!("{prefix}{file}/");
                match read_pack_json(&path, &unit)? {
                    Some(own) => stack.push((path, sub.clone(), sub, Arc::new(own))),
                    None if unit.combine_subdirectories && !unit.ignore => stack.push((path, sub, group.clone(), unit.clone())),
                    None => stack.push((path, sub.clone(), sub, unit.clone())),
                }
            } else if unit.ignore {
                continue;
            } else if let Some(stem) = file.strip_suffix(".png").or_else(|| file.strip_suffix(".PNG")) {
                let name = format!("{prefix}{stem}");
                out.push(Source {
                    mesh: mesh.contains(&name),
                    tris: shapes.remove(&name).unwrap_or_default(),
                    name,
                    path,
                    group: Some(group.clone()),
                    settings: Some(unit.clone()),
                });
            }
        }
    }
    if out.is_empty() {
        return Err(SpineError::Format(format!("이미지 폴더에 PNG 가 없습니다: {}", images_dir.display())));
    }
    sort_sources(&mut out);
    Ok(out)
}

/// 여러 스켈레톤의 이미지를 한 atlas 로(Pack target «Single atlas»). 같은 이름은 한 번만 넣는다.
/// 돌려주는 두 번째 값: 이름은 같은데 파일이 달라 먼저 나온 것을 쓴 영역 이름.
pub fn merge_sources(lists: Vec<Vec<Source>>) -> (Vec<Source>, Vec<String>) {
    let mut at: HashMap<String, usize> = HashMap::new();
    let mut out: Vec<Source> = Vec::new();
    let mut clash = Vec::new();
    for s in lists.into_iter().flatten() {
        match at.get(&s.name) {
            Some(&i) => {
                out[i].mesh |= s.mesh;
                // 한쪽이라도 사각형으로 쓰면 사각형, 아니면 모양을 합친다
                if out[i].tris.is_empty() || s.tris.is_empty() {
                    out[i].tris.clear();
                } else {
                    out[i].tris.extend_from_slice(&s.tris);
                }
                if out[i].path != s.path && !clash.contains(&s.name) {
                    clash.push(s.name);
                }
            }
            None => {
                at.insert(s.name.clone(), out.len());
                out.push(s);
            }
        }
    }
    sort_sources(&mut out);
    (out, clash)
}

/// 접미사 suffix 의 atlas 하나를 묶는다. 묶음(폴더)마다 그 설정·배율로 페이지를 만들고 머리말도 그 설정으로 쓴다.
fn pack_at(sources: &[Source], name: &str, root: &AtlasSettings, suffix: &str) -> SpineResult<PackResult> {
    root.check()?;
    // 이 atlas 에 들어갈 이미지와 그 설정·배율: 자기 배율 목록에 이 접미사가 없는 폴더는 뺀다
    let picked: Vec<(&Source, &AtlasSettings, f32)> = sources
        .iter()
        .filter_map(|src| {
            let s = src.settings.as_deref().unwrap_or(root);
            s.scales().into_iter().find(|(_, sfx)| sfx == suffix).map(|(scale, _)| (src, s, scale))
        })
        .collect();
    if picked.is_empty() {
        return Err(SpineError::Format("묶을 이미지가 없습니다".into()));
    }

    // 이미지 읽기·잘라내기는 영역마다 독립이라 병렬로 한다(순서는 이름 순 그대로).
    let loaded: Vec<SpineResult<(Region, bool, bool)>> = picked
        .par_iter()
        .map(|&(src, settings, scale)| {
            let (rn, path) = (&src.name, &src.path);
            if !path.is_file() {
                let (r, _) = build_region(rn.clone(), 1, 1, &[0u8; 4], false, false, None, false, settings);
                return Ok((r, true, false));
            }
            let (w, h, mut rgba) = png_codec::decode_file(path)?;
            let (mut w, mut h) = (w as i32, h as i32);
            if scale != 1.0 {
                let (nw, nh, px) = resize::resize(w as usize, h as usize, &rgba, scale);
                (w, h, rgba) = (nw as i32, nh as i32, px);
            }
            // 메시만 쓰는 이미지는 패킹 방식과 상관없이 메시가 걸친 범위로 자른다
            let shape = polygon::raster_tris(w as usize, h as usize, &src.tris);
            let strip = !src.mesh || shape.is_some();
            let (sx, sy) = (settings.strip_whitespace_x, settings.strip_whitespace_y);
            let (r, blank) = build_region(rn.clone(), w, h, &rgba, strip && sx, strip && sy, shape, settings.polygons(), settings);
            Ok((r, false, blank))
        })
        .collect();
    let mut regions = Vec::with_capacity(loaded.len());
    // 영역마다 (묶음 경로, 설정, 배율)
    let mut owners: Vec<(&str, &AtlasSettings, f32)> = Vec::with_capacity(loaded.len());
    let mut missing = Vec::new();
    let mut ignored = Vec::new();
    for (item, &(src, settings, scale)) in loaded.into_iter().zip(&picked) {
        let (r, miss, blank) = item?;
        if miss {
            missing.push(r.name.clone());
        } else if blank && settings.ignore_blank_images {
            ignored.push(r.name.clone());
            continue;
        }
        regions.push(r);
        owners.push((src.group.as_deref().unwrap_or(""), settings, scale));
    }
    if regions.is_empty() {
        return Err(SpineError::Format("묶을 이미지가 없습니다(모두 투명해서 뺐습니다)".into()));
    }

    // 묶음(폴더)마다 페이지를 따로 쓴다. Spine 처럼 폴더 나무를 깊이 우선으로: 경로 조각을 Ordinal 순으로 견준다.
    let mut by_group: std::collections::BTreeMap<Vec<&str>, Vec<usize>> = Default::default();
    for (i, (group, ..)) in owners.iter().enumerate() {
        by_group.entry(group.split('/').filter(|c| !c.is_empty()).collect()).or_default().push(i);
    }

    // 같은 그림은 한 자리에(묶음 안에서): 이름순 첫 영역을 싣고 나머지는 그 자리를 함께 쓴다
    let mut aliases: HashMap<usize, Vec<usize>> = HashMap::new();
    let mut pages: Vec<(Page, &AtlasSettings, f32)> = Vec::new();
    for ids in by_group.values() {
        let (_, settings, scale) = owners[ids[0]];
        let mut packed: Vec<usize> = Vec::with_capacity(ids.len());
        if settings.alias {
            let mut first: HashMap<&AliasKey, usize> = HashMap::new();
            for &i in ids {
                let rep = *first.entry(regions[i].key.as_ref().unwrap()).or_insert(i);
                if rep == i {
                    packed.push(i);
                } else {
                    aliases.entry(rep).or_default().push(i);
                }
            }
        } else {
            packed.extend(ids);
        }
        let group_pages = pack_group(&mut regions, &packed, settings)?;
        pages.extend(group_pages.into_iter().map(|p| (p, settings, scale)));
    }
    for r in regions.iter_mut() {
        r.key = None;
    }
    for (&rep, list) in &aliases {
        let (x, y, rot, flip) = (regions[rep].x, regions[rep].y, regions[rep].rotated, regions[rep].flip);
        for &i in list {
            regions[i].x = x;
            regions[i].y = y;
            regions[i].rotated = rot;
            regions[i].flip = flip;
        }
    }

    let rgbas: Vec<Vec<u8>> = pages.par_iter().map(|(p, s, _)| blit(p.w, p.h, &p.regions, &regions, s)).collect();
    let mut text = String::new();
    let mut out_pages = Vec::with_capacity(pages.len());
    for (i, ((page, settings, scale), rgba)) in pages.iter().zip(rgbas).enumerate() {
        let (settings, scale) = (*settings, *scale);
        // Spine: 첫 페이지는 이름 그대로, 다음부터 `이름 + separator + 번호`(2부터)
        let file_name = if i == 0 { format!("{name}.png") } else { format!("{name}{}{}.png", root.separator, i + 1) };
        write_page_header(&mut text, &file_name, page, settings, scale, i == 0);
        // Spine 처럼 이름 Ordinal 순(UTF-16, 대소문자 구분: `Line_root` < `bird`, `a10` < `a2`)으로 쓰고,
        // 같은 이름이면 index 오름차순, index 가 없는 것은 그 뒤에 둔다(Spine 4.2.43 으로 확인).
        // 같은 그림을 함께 쓰는 영역(에일리어스)은 Spine 처럼 그 그림을 실은 영역 바로 뒤에 이름 순으로 붙인다.
        let by_name = |list: &[usize]| -> Vec<usize> {
            let keys: Vec<Vec<u16>> = list.iter().map(|&i| regions[i].out_name.encode_utf16().collect()).collect();
            let mut order: Vec<usize> = (0..list.len()).collect();
            let index = |i: usize| regions[list[i]].index.map_or((1, 0), |n| (0, n));
            order.sort_by(|&a, &b| keys[a].cmp(&keys[b]).then(index(a).cmp(&index(b))));
            order.into_iter().map(|o| list[o]).collect()
        };
        let mut ids: Vec<usize> = Vec::with_capacity(page.regions.len());
        for r in by_name(&page.regions) {
            ids.push(r);
            if let Some(list) = aliases.get(&r) {
                ids.extend(by_name(list));
            }
        }
        for id in ids {
            write_region(&mut text, &regions[id], settings.legacy_output);
        }
        out_pages.push(AtlasPage { name: file_name, width: page.w as u32, height: page.h as u32, rgba });
    }

    Ok(PackResult { output: AtlasOutput { atlas_text: text, pages: out_pages }, missing, ignored })
}

/// 묶음 하나(같은 설정)의 영역을 페이지로 나눠 놓는다.
fn pack_group(regions: &mut [Region], ids: &[usize], settings: &AtlasSettings) -> SpineResult<Vec<Page>> {
    if settings.grid {
        return pack_grid(regions, ids, settings);
    }
    // 다각형 패킹: 모양대로 묶은 것과 사각형으로 묶은 것 중 페이지가 적고 좁은 쪽을 쓴다
    let shaped = if settings.polygons() && ids.iter().any(|&i| regions[i].mask.is_some()) {
        polygon::pack_polygons(regions, ids, settings).ok().map(|p| {
            let spots: Vec<(usize, i32, i32, bool, bool)> = ids.iter().map(|&i| (i, regions[i].x, regions[i].y, regions[i].rotated, regions[i].flip)).collect();
            (p, spots)
        })
    } else {
        None
    };
    let rects = pack_pages(regions, ids, settings)?;
    let size = |p: &[Page]| (p.len(), p.iter().map(|p| p.w as i64 * p.h as i64).sum::<i64>());
    match shaped {
        Some((p, spots)) if size(&p) < size(&rects) => {
            for (i, x, y, rotated, flip) in spots {
                (regions[i].x, regions[i].y, regions[i].rotated, regions[i].flip) = (x, y, rotated, flip);
            }
            Ok(p)
        }
        _ => Ok(rects),
    }
}

/// 페이지 머리말. 페이지마다 그 묶음의 설정을 따른다(Spine 도 폴더 설정이 다르면 머리말이 다르다).
fn write_page_header(text: &mut String, file_name: &str, page: &Page, settings: &AtlasSettings, scale: f32, first: bool) {
    let repeat = match (settings.wrap_x != TextureWrap::ClampToEdge, settings.wrap_y != TextureWrap::ClampToEdge) {
        (true, true) => Some("xy"),
        (true, false) => Some("x"),
        (false, true) => Some("y"),
        _ => None,
    };
    if settings.legacy_output {
        // 옛 형식(Spine 의 Legacy output 과 같은 글자): 페이지마다 빈 줄로 시작하고, 머리말 네 줄은 순서·개수가 고정이다
        // (3.8 런타임은 차례대로만 읽는다). pma · scale 줄은 없다.
        let comma = if settings.legacy_editor38 { "," } else { ", " };
        let _ = writeln!(
            text,
            "\n{}\nsize: {}{comma}{}\nformat: {}\nfilter: {}{comma}{}\nrepeat: {}",
            file_name,
            page.w,
            page.h,
            settings.format.as_str(),
            settings.filter_min.as_str(),
            settings.filter_mag.as_str(),
            repeat.unwrap_or("none")
        );
        return;
    }
    if !first {
        text.push('\n'); // 페이지 사이에는 빈 줄이 있어야 런타임이 새 페이지로 읽는다
    }
    let _ = writeln!(text, "{}\nsize:{},{}", file_name, page.w, page.h);
    if settings.format != PixelFormat::RGBA8888 {
        let _ = writeln!(text, "format:{}", settings.format.as_str());
    }
    // Spine 은 기본값(Nearest, Nearest)이면 filter 줄을 쓰지 않는다
    if settings.filter_min != TextureFilter::Nearest || settings.filter_mag != TextureFilter::Nearest {
        let _ = writeln!(text, "filter:{},{}", settings.filter_min.as_str(), settings.filter_mag.as_str());
    }
    if let Some(rep) = repeat {
        let _ = writeln!(text, "repeat:{rep}");
    }
    if settings.pma {
        text.push_str("pma:true\n");
    }
    if scale != 1.0 {
        let _ = writeln!(text, "scale:{scale}"); // Spine 도 배율을 적는다
    }
}

fn write_region(text: &mut String, r: &Region, legacy: bool) {
    let _ = writeln!(text, "{}", r.out_name);
    if legacy {
        // 영역마다 여섯 줄, 순서 고정. 크기는 돌리기 전의 것이고 offset 은 왼쪽·아래 기준이다
        let rotate = match (r.rotated, r.flip) {
            (false, false) => "false",
            (true, false) => "true",
            (false, true) => "180",
            (true, true) => "270",
        };
        let _ = writeln!(
            text,
            "  rotate: {}\n  xy: {}, {}\n  size: {}, {}\n  orig: {}, {}\n  offset: {}, {}\n  index: {}",
            rotate,
            r.x,
            r.y,
            r.w,
            r.h,
            r.orig_w,
            r.orig_h,
            r.left,
            r.bottom_off,
            r.index.unwrap_or(-1)
        );
        return;
    }
    if let Some(index) = r.index {
        let _ = writeln!(text, "index:{index}");
    }
    let _ = writeln!(text, "bounds:{},{},{},{}", r.x, r.y, r.w, r.h);
    if r.stripped() {
        let _ = writeln!(text, "offsets:{},{},{},{}", r.left, r.bottom_off, r.orig_w, r.orig_h);
    }
    match (r.rotated, r.flip) {
        (true, false) => text.push_str("rotate:90\n"),
        (false, true) => text.push_str("rotate:180\n"),
        (true, true) => text.push_str("rotate:270\n"),
        _ => {}
    }
}

#[cfg(test)]
mod tests;
