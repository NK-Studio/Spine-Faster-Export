//! 이미지 폴더 → atlas 텍스트 + 페이지 RGBA. C# `SpineAtlasPacker.cs`·`SpinePngCodec.cs` + 파이썬 `atlas_packer.py` 의 PMA/블리드/에일리어스 옵션.
//!
//! 설정은 Spine 의 Texture Packer Settings 와 같은 항목·JSON 키(`pack.json`)를 쓴다.
//! 기본값([`AtlasSettings::default`])의 결과는 C# 과 같고(여백 2·가장자리 여백·투명 테두리 잘라내기(알파 3 초과)·회전·PMA),
//! PMA/블리드/에일리어스/최대 크기 옵션은 파이썬 PoC 와 같다.
//! 페이지 안 배치는 Spine 과 같지 않을 수 있다(런타임은 atlas 좌표를 쓰므로 결과에는 영향이 없다).
//!
//! 공개 API(계약): [`AtlasSettings`], [`pack`], [`pack_scales`], [`encode_png`]. 빠진 이미지 목록이 필요하면 [`pack_detailed`].

mod natural;
mod png_codec;
mod resize;

use std::collections::{HashMap, HashSet};
use std::fmt::Write as _;
use std::path::{Path, PathBuf};

use rayon::prelude::*;
use serde::{Deserialize, Serialize};
use spine_model::{AtlasOutput, AtlasPage, SkeletonModel, SpineError, SpineResult};

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
    /// "polygons" 는 지원하지 않아 직사각형으로 묶는다.
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
    /// 이 앱은 스켈레톤 첨부로 묶으므로 쓰지 않는다(Spine `pack.json` 호환을 위해 둔다).
    pub combine_subdirectories: bool,
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
            flatten_paths: false,
            use_indexes: false,
            fast: false,
            debug: false,
            separator: "_".into(),
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
    x: i32,
    y: i32,
    rotated: bool,
}

impl Region {
    fn stripped(&self) -> bool {
        self.w != self.orig_w || self.h != self.orig_h
    }
}

/// (left, top, w, h, 원본 w, 원본 h, 완전 투명 픽셀 RGB 를 지운 straight 픽셀).
#[derive(PartialEq, Eq, Hash)]
struct AliasKey(i32, i32, i32, i32, i32, i32, Vec<u8>);

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

/// 투명 테두리 잘라내기(알파가 문턱값 이하인 줄·칸). 메시 이미지는 UV 가 전체 이미지 기준이라 자르지 않는다.
/// 돌려주는 bool: 문턱값을 넘는 픽셀이 하나도 없는 이미지.
fn build_region(name: String, w: i32, h: i32, rgba: &[u8], strip_x: bool, strip_y: bool, settings: &AtlasSettings) -> (Region, bool) {
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
    // 다 투명한 이미지는 자르는 축을 1픽셀로 둔다(C# 과 같음)
    let (left, right) = if !strip_x { (0, w - 1) } else if blank { (0, 0) } else { (left, right) };
    let (top, bottom) = if !strip_y { (0, h - 1) } else if blank { (0, 0) } else { (top, bottom) };
    let rw = (right - left + 1) as usize;
    let rh = (bottom - top + 1) as usize;
    let mut straight = Vec::with_capacity(rw * rh * 4);
    for y in top as usize..=bottom as usize {
        let s = (y * wu + left as usize) * 4;
        straight.extend_from_slice(&rgba[s..s + rw * 4]);
    }

    let key = if settings.alias {
        let mut k = straight.clone();
        for p in k.chunks_exact_mut(4) {
            if p[3] == 0 {
                p[0] = 0;
                p[1] = 0;
                p[2] = 0;
            }
        }
        Some(AliasKey(left, top, rw as i32, rh as i32, w, h, k))
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
    } else if settings.bleed {
        // 블리드는 PMA 를 끌 때만 의미가 있다(Spine 도 PMA 켜짐에서는 결과가 같다)
        bleed_rgb(rw, rh, straight, settings.bleed_iterations as usize)
    } else {
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
        x: 0,
        y: 0,
        rotated: false,
    };
    (region, blank)
}

/// 완전 투명 픽셀의 RGB 를 주변 8칸 평균(정수 나눗셈)으로 채운다(알파는 그대로). 잘라낸 뒤의 이미지에 2회 적용한다.
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
            let non_empty = |s: &Option<String>| s.as_deref().filter(|s| !s.is_empty()).map(str::to_owned);
            let n = non_empty(&a.path)
                .or_else(|| non_empty(&a.name))
                .or_else(|| row.key.clone())
                .unwrap_or_default();
            let mut add = |frame: String| {
                if t != "region" {
                    mesh.insert(frame.clone());
                }
                names.insert(frame);
            };
            match &a.sequence {
                None => add(n),
                Some(q) => {
                    for i in 0..q.count.max(0) {
                        add(format!("{}{}", n, zfill(q.start + i, q.digits)));
                    }
                }
            }
        }
    }
    let mut sorted: Vec<String> = names.into_iter().collect();
    // C#: OrderBy(Ordinal).OrderBy(Natural) — 자연 순, 같으면 Ordinal 순
    sorted.sort_by(|a, b| natural::natural_cmp(a, b).then_with(|| natural::ordinal_cmp(a, b)));
    (sorted, mesh)
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
        placed.push(Placed { idx, x, y, rotated });
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
        if !r.rotated {
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
    if s.duplicate_padding {
        let (ax, ay) = ((s.padding_x / 2) as usize, (s.padding_y / 2) as usize);
        for &i in ids {
            duplicate_edges(&mut out, pw, ph, page_rect(&regions[i]), ax, ay);
        }
    }
    if s.debug {
        for &i in ids {
            draw_rect(&mut out, pw, ph, page_rect(&regions[i]));
        }
        draw_rect(&mut out, pw, ph, (0, 0, pw, ph));
    }
    out
}

/// Duplicate padding: 영역 가장자리 픽셀을 여백 절반만큼 바깥으로 늘린다(모서리 포함).
fn duplicate_edges(out: &mut [u8], pw: usize, ph: usize, (x, y, w, h): (usize, usize, usize, usize), ax: usize, ay: usize) {
    if w == 0 || h == 0 {
        return;
    }
    let copy = |out: &mut [u8], from: (usize, usize), to: (usize, usize)| {
        let (f, t) = ((from.1 * pw + from.0) * 4, (to.1 * pw + to.0) * 4);
        out.copy_within(f..f + 4, t);
    };
    for row in y..(y + h).min(ph) {
        for k in 1..=ax {
            if x >= k {
                copy(out, (x, row), (x - k, row));
            }
            if x + w - 1 + k < pw {
                copy(out, (x + w - 1, row), (x + w - 1 + k, row));
            }
        }
    }
    let (x0, x1) = (x.saturating_sub(ax), (x + w - 1 + ax).min(pw - 1));
    for col in x0..=x1 {
        for k in 1..=ay {
            if y >= k {
                copy(out, (col, y), (col, y - k));
            }
            if y + h - 1 + k < ph {
                copy(out, (col, y + h - 1), (col, y + h - 1 + k));
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
    let (scale, suffix) = settings.scales().remove(0);
    let sources = attachment_sources(model, images_dir)?;
    pack_at(&sources, &format!("{name}{suffix}"), settings, scale)
}

/// 배율마다 하나씩: (접미사, 결과). 파일 이름은 `name + 접미사`.
pub fn pack_scales(model: &SkeletonModel, images_dir: &Path, name: &str, settings: &AtlasSettings) -> SpineResult<Vec<(String, PackResult)>> {
    pack_sources_scales(&attachment_sources(model, images_dir)?, name, settings)
}

/// [`pack_scales`] 와 같지만 묶을 이미지를 직접 준다(이미지 폴더째 묶기, 여러 스켈레톤을 한 atlas 로).
pub fn pack_sources_scales(sources: &[Source], name: &str, settings: &AtlasSettings) -> SpineResult<Vec<(String, PackResult)>> {
    settings.check()?;
    settings
        .scales()
        .into_iter()
        .map(|(scale, suffix)| pack_at(sources, &format!("{name}{suffix}"), settings, scale).map(|r| (suffix, r)))
        .collect()
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
    /// 페이지를 따로 쓰는 묶음(이미지 폴더의 하위 폴더). None 이면 모두 한데 묶는다.
    pub group: Option<String>,
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
    Ok(names
        .into_iter()
        .map(|name| Source { path: images_dir.join(format!("{name}.png")), mesh: mesh.contains(&name), name, group: None })
        .collect())
}

/// Pack source «Image folders»: 스켈레톤 이미지 폴더의 PNG 를 하위 폴더까지 모두.
/// combine 이 꺼져 있으면 폴더마다 페이지를 따로 쓴다(Combine subdirectories).
pub fn folder_sources(model: &SkeletonModel, images_dir: &Path, combine: bool) -> SpineResult<Vec<Source>> {
    need_images_dir(images_dir)?;
    let (_, mesh) = region_names(model);
    let mut out = Vec::new();
    let mut stack = vec![(images_dir.to_path_buf(), String::new())];
    while let Some((dir, prefix)) = stack.pop() {
        for entry in std::fs::read_dir(&dir)?.filter_map(|e| e.ok()) {
            let path = entry.path();
            let Some(file) = path.file_name().and_then(|f| f.to_str()).map(str::to_owned) else { continue };
            if file.starts_with('.') {
                continue;
            }
            if path.is_dir() {
                stack.push((path, format!("{prefix}{file}/")));
            } else if let Some(stem) = file.strip_suffix(".png").or_else(|| file.strip_suffix(".PNG")) {
                let name = format!("{prefix}{stem}");
                let group = (!combine).then(|| prefix.clone());
                out.push(Source { mesh: mesh.contains(&name), name, path, group });
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

fn pack_at(sources: &[Source], name: &str, settings: &AtlasSettings, scale: f32) -> SpineResult<PackResult> {
    settings.check()?;
    if sources.is_empty() {
        return Err(SpineError::Format("묶을 이미지가 없습니다".into()));
    }

    // 이미지 읽기·잘라내기는 영역마다 독립이라 병렬로 한다(순서는 이름 순 그대로).
    let (sx, sy) = (settings.strip_whitespace_x, settings.strip_whitespace_y);
    let loaded: Vec<SpineResult<(Region, bool, bool)>> = sources
        .par_iter()
        .map(|src| {
            let (rn, path) = (&src.name, &src.path);
            if !path.is_file() {
                let (r, _) = build_region(rn.clone(), 1, 1, &[0u8; 4], false, false, settings);
                return Ok((r, true, false));
            }
            let (w, h, mut rgba) = png_codec::decode_file(path)?;
            let (mut w, mut h) = (w as i32, h as i32);
            if scale != 1.0 {
                let (nw, nh, px) = resize::resize(w as usize, h as usize, &rgba, scale);
                (w, h, rgba) = (nw as i32, nh as i32, px);
            }
            let strip = !src.mesh;
            let (r, blank) = build_region(rn.clone(), w, h, &rgba, strip && sx, strip && sy, settings);
            Ok((r, false, blank))
        })
        .collect();
    let mut regions = Vec::with_capacity(loaded.len());
    let mut groups: Vec<&Option<String>> = Vec::with_capacity(loaded.len());
    let mut missing = Vec::new();
    let mut ignored = Vec::new();
    for (item, src) in loaded.into_iter().zip(sources) {
        let (r, miss, blank) = item?;
        if miss {
            missing.push(r.name.clone());
        } else if blank && settings.ignore_blank_images {
            ignored.push(r.name.clone());
            continue;
        }
        regions.push(r);
        groups.push(&src.group);
    }
    if regions.is_empty() {
        return Err(SpineError::Format("묶을 이미지가 없습니다(모두 투명해서 뺐습니다)".into()));
    }

    // 같은 그림은 한 자리에: 이름순 첫 영역을 싣고 나머지는 그 자리를 함께 쓴다
    let mut packed: Vec<usize> = Vec::with_capacity(regions.len());
    let mut aliases: HashMap<usize, Vec<usize>> = HashMap::new();
    if settings.alias {
        let mut first: HashMap<&AliasKey, usize> = HashMap::new();
        for (i, r) in regions.iter().enumerate() {
            let rep = *first.entry(r.key.as_ref().unwrap()).or_insert(i);
            if rep == i {
                packed.push(i);
            } else {
                aliases.entry(rep).or_default().push(i);
            }
        }
    } else {
        packed.extend(0..regions.len());
    }
    for r in regions.iter_mut() {
        r.key = None;
    }

    // 묶음(하위 폴더)마다 페이지를 따로 쓴다. 묶음이 없으면 한 번에.
    let mut by_group: std::collections::BTreeMap<&Option<String>, Vec<usize>> = Default::default();
    for &i in &packed {
        by_group.entry(groups[i]).or_default().push(i);
    }
    let mut pages = Vec::new();
    for ids in by_group.values() {
        pages.extend(if settings.grid { pack_grid(&mut regions, ids, settings)? } else { pack_pages(&mut regions, ids, settings)? });
    }
    for (&rep, list) in &aliases {
        let (x, y, rot) = (regions[rep].x, regions[rep].y, regions[rep].rotated);
        for &i in list {
            regions[i].x = x;
            regions[i].y = y;
            regions[i].rotated = rot;
        }
    }

    let rgbas: Vec<Vec<u8>> = pages.par_iter().map(|p| blit(p.w, p.h, &p.regions, &regions, settings)).collect();
    let repeat = match (settings.wrap_x != TextureWrap::ClampToEdge, settings.wrap_y != TextureWrap::ClampToEdge) {
        (true, true) => Some("xy"),
        (true, false) => Some("x"),
        (false, true) => Some("y"),
        _ => None,
    };
    let mut text = String::new();
    let mut out_pages = Vec::with_capacity(pages.len());
    for (i, (page, rgba)) in pages.iter().zip(rgbas).enumerate() {
        // Spine: 첫 페이지는 이름 그대로, 다음부터 `이름 + separator + 번호`(2부터)
        let file_name = if i == 0 { format!("{name}.png") } else { format!("{name}{}{}.png", settings.separator, i + 1) };
        if i > 0 {
            text.push('\n'); // 페이지 사이에는 빈 줄이 있어야 런타임이 새 페이지로 읽는다
        }
        let _ = writeln!(text, "{}\nsize:{},{}", file_name, page.w, page.h);
        if settings.format != PixelFormat::RGBA8888 {
            let _ = writeln!(text, "format:{}", settings.format.as_str());
        }
        let _ = writeln!(text, "filter:{},{}", settings.filter_min.as_str(), settings.filter_mag.as_str());
        if let Some(rep) = repeat {
            let _ = writeln!(text, "repeat:{rep}");
        }
        if settings.pma {
            text.push_str("pma:true\n");
        }
        let mut ids: Vec<usize> = page.regions.clone();
        for &r in &page.regions {
            if let Some(list) = aliases.get(&r) {
                ids.extend_from_slice(list);
            }
        }
        // 안정 정렬(C# OrderBy · 파이썬 sorted 와 같음)
        let keys: Vec<Vec<u16>> = ids.iter().map(|&i| regions[i].out_name.encode_utf16().collect()).collect();
        let mut order: Vec<usize> = (0..ids.len()).collect();
        order.sort_by(|&a, &b| natural::natural_cmp16(&keys[a], &keys[b]).then(regions[ids[a]].index.cmp(&regions[ids[b]].index)));
        for o in order {
            let r = &regions[ids[o]];
            let _ = writeln!(text, "{}", r.out_name);
            if let Some(index) = r.index {
                let _ = writeln!(text, "index:{index}");
            }
            let _ = writeln!(text, "bounds:{},{},{},{}", r.x, r.y, r.w, r.h);
            if r.stripped() {
                let _ = writeln!(text, "offsets:{},{},{},{}", r.left, r.bottom_off, r.orig_w, r.orig_h);
            }
            if r.rotated {
                text.push_str("rotate:90\n");
            }
        }
        out_pages.push(AtlasPage { name: file_name, width: page.w as u32, height: page.h as u32, rgba });
    }

    Ok(PackResult { output: AtlasOutput { atlas_text: text, pages: out_pages }, missing, ignored })
}

#[cfg(test)]
mod tests;
