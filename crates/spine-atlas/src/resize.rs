//! 배율(Scale) 적용: RGBA(곱하지 않은 알파) 이미지를 늘리거나 줄인다.
//!
//! Spine 텍스처 패커의 `scaleResampling` 과 같은 결과를 낸다(Spine 4.3 명령줄 패커와 픽셀 단위로 대조했다).
//! Spine 은 Java AWT/Java2D 로 배율을 적용한다:
//! - `auto` · `bicubic`: AWT 의 넓이 평균(`Image.SCALE_AREA_AVERAGING`). 늘릴 때도 같다(정수 배율이면 픽셀 복제).
//! - `bilinear` · `bilinearProgressive`: Java2D 의 BILINEAR 보간. 줄일 때는 목표 크기의 2ⁿ 배로 먼저 줄이고 반씩 줄여 간다.
//! - `nearest`: 가장 가까운 픽셀을 그대로 복사. 줄일 때는 bilinear 와 같은 순서로 여러 번.

/// 배율을 적용하는 방식.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Resampling {
    /// 넓이 평균(Spine `auto` · `bicubic`).
    Area,
    /// Java2D BILINEAR(Spine `bilinear` · `bilinearProgressive`).
    Bilinear,
    /// 가장 가까운 픽셀(Spine `nearest`).
    Nearest,
}

impl Resampling {
    /// Spine `pack.json` 의 이름. 모르는 이름은 None.
    pub fn parse(name: &str) -> Option<Resampling> {
        match name.to_ascii_lowercase().as_str() {
            "auto" | "bicubic" => Some(Resampling::Area),
            "bilinear" | "bilinearprogressive" => Some(Resampling::Bilinear),
            "nearest" => Some(Resampling::Nearest),
            _ => None,
        }
    }
}

/// (새 폭, 새 높이, RGBA). 새 크기는 반올림, 최소 1.
pub fn resize(w: usize, h: usize, rgba: &[u8], scale: f32, method: Resampling) -> (usize, usize, Vec<u8>) {
    let nw = ((w as f64 * scale as f64).round() as usize).max(1);
    let nh = ((h as f64 * scale as f64).round() as usize).max(1);
    if nw == w && nh == h {
        return (w, h, rgba.to_vec());
    }
    let out = match method {
        Resampling::Area => progressive(w, h, rgba, nw, nh, area_average),
        Resampling::Bilinear => progressive(w, h, rgba, nw, nh, bilinear),
        Resampling::Nearest => progressive(w, h, rgba, nw, nh, nearest),
    };
    (nw, nh, out)
}

type Step = fn(usize, usize, &[u8], usize, usize) -> Vec<u8>;

/// 목표 크기의 2ⁿ 배(원본보다 작은 가장 큰 것)로 한 번 바꾼 뒤 반씩 줄여 목표에 닿는다. 늘릴 때는 한 번에.
fn progressive(w: usize, h: usize, rgba: &[u8], nw: usize, nh: usize, step: Step) -> Vec<u8> {
    let (mut cw, mut ch) = (nw, nh);
    while cw * 2 < w && ch * 2 < h {
        cw *= 2;
        ch *= 2;
    }
    let mut cur = step(w, h, rgba, cw, ch);
    while cw > nw {
        let (pw, ph) = (cw, ch);
        cw /= 2;
        ch /= 2;
        cur = step(pw, ph, &cur, cw, ch);
    }
    cur
}

// ---------------------------------------------------------------- 넓이 평균

/// `java.awt.image.AreaAveragingScaleFilter` 를 그대로 옮겼다(f32 누적, 같은 순서).
fn area_average(sw: usize, sh: usize, src: &[u8], dw: usize, dh: usize) -> Vec<u8> {
    let mut out = vec![0u8; dw * dh * 4];
    let mut acc = vec![[0f32; 4]; dw]; // a, r, g, b
    let (mut sy, mut syrem, mut dy, mut dyrem) = (0usize, dh, 0usize, 0usize);
    while sy < sh {
        if dyrem == 0 {
            acc.iter_mut().for_each(|c| *c = [0.0; 4]);
            dyrem = sh;
        }
        let amty = syrem.min(dyrem);
        let row = &src[sy * sw * 4..(sy + 1) * sw * 4];
        let (mut sx, mut dx, mut sxrem, mut dxrem) = (0usize, 0usize, 0usize, sw);
        let mut px = [0f32; 4];
        while sx < sw {
            if sxrem == 0 {
                sxrem = dw;
                let p = &row[sx * 4..sx * 4 + 4];
                let a = p[3] as f32;
                let (mut r, mut g, mut b) = (p[0] as f32, p[1] as f32, p[2] as f32);
                if a != 255.0 {
                    let s = a / 255.0;
                    r *= s;
                    g *= s;
                    b *= s;
                }
                px = [a, r, g, b];
            }
            let amtx = sxrem.min(dxrem);
            let mult = amtx as f32 * amty as f32;
            for c in 0..4 {
                acc[dx][c] += mult * px[c];
            }
            sxrem -= amtx;
            if sxrem == 0 {
                sx += 1;
            }
            dxrem -= amtx;
            if dxrem == 0 {
                dx += 1;
                dxrem = sw;
            }
        }
        dyrem -= amty;
        if dyrem == 0 {
            let line = area_row(&acc, sw, sh);
            loop {
                out[dy * dw * 4..(dy + 1) * dw * 4].copy_from_slice(&line);
                dy += 1;
                syrem -= amty;
                if !(syrem >= amty && amty == sh) {
                    break;
                }
            }
        } else {
            syrem -= amty;
        }
        if syrem == 0 {
            syrem = dh;
            sy += 1;
        }
    }
    out
}

/// `AreaAveragingScaleFilter.calcRow`.
fn area_row(acc: &[[f32; 4]], sw: usize, sh: usize) -> Vec<u8> {
    let origmult = sw as f32 * sh as f32;
    let mut line = Vec::with_capacity(acc.len() * 4);
    for &[a, r, g, b] in acc {
        let mut mult = origmult;
        let mut ai = java_round(a / mult);
        if ai <= 0 {
            ai = 0;
        } else if ai >= 255 {
            ai = 255;
        } else {
            mult = a / 255.0;
        }
        let c = |v: f32| java_round(v / mult).clamp(0, 255) as u8;
        line.extend_from_slice(&[c(r), c(g), c(b), ai as u8]);
    }
    line
}

/// Java `Math.round(float)`: floor(x + 1/2).
fn java_round(x: f32) -> i64 {
    if x.is_nan() {
        return 0;
    }
    (x as f64 + 0.5).floor() as i64
}

// ---------------------------------------------------------------- Java2D

/// Java2D 의 `mul8table`(AlphaMath.c): 대략 round(a·b/255).
fn mul8(a: u32, b: u32) -> u32 {
    (a * b * 0x10101 + (1 << 23)) >> 24
}

/// Java2D 의 `div8table`: 대략 round(b·255/a), a ≤ b 면 255.
fn div8(a: u32, b: u32) -> u32 {
    if b >= a {
        return 255;
    }
    let inc = ((0xffu32 << 24) + a / 2) / a;
    ((1u32 << 23) + b * inc) >> 24
}

/// 곱하지 않은 RGBA → Java2D IntArgbPre 의 성분(r, g, b, a).
fn to_pre(p: &[u8]) -> [u32; 4] {
    let a = p[3] as u32;
    if a == 0 {
        [0; 4]
    } else if a < 255 {
        [mul8(a, p[0] as u32), mul8(a, p[1] as u32), mul8(a, p[2] as u32), a]
    } else {
        [p[0] as u32, p[1] as u32, p[2] as u32, 255]
    }
}

/// IntArgbPre 를 빈(투명) IntArgb 위에 SrcOver 로 그린 결과.
fn from_pre(c: [u32; 4]) -> [u8; 4] {
    let a = c[3];
    if a == 0 {
        return [0; 4];
    }
    if a < 255 {
        [div8(a, c[0]) as u8, div8(a, c[1]) as u8, div8(a, c[2]) as u8, a as u8]
    } else {
        [c[0] as u8, c[1] as u8, c[2] as u8, 255]
    }
}

const ONE: f64 = 4294967296.0; // 2³², Java2D 의 32.32 고정소수점
const HALF: i64 = 1 << 31;

/// 출력 칸마다 원본 좌표(32.32 고정소수점, 칸 가운데 기준). Java2D TransformHelper 처럼 줄 처음만 실수로 구하고
/// 나머지는 고정소수점 증분을 더한다.
fn fixed_coords(src: usize, dst: usize) -> Vec<i64> {
    let inv = 1.0 / (dst as f64 / src as f64);
    let step = (inv * ONE) as i64;
    let base = (0.5 * inv * ONE) as i64;
    (0..dst as i64).map(|i| base + i * step).collect()
}

/// Java2D 의 BILINEAR 보간(TransformHelper.c 의 `BilinearInterp`, 가장자리는 늘려 쓴다).
fn bilinear(sw: usize, sh: usize, src: &[u8], dw: usize, dh: usize) -> Vec<u8> {
    let pre: Vec<[u32; 4]> = src.chunks_exact(4).map(to_pre).collect();
    let xs: Vec<(usize, usize, u32)> = fixed_coords(sw, dw).into_iter().map(|x| split(x - HALF, sw)).collect();
    let ys: Vec<(usize, usize, u32)> = fixed_coords(sh, dh).into_iter().map(|y| split(y - HALF, sh)).collect();
    let mut out = vec![0u8; dw * dh * 4];
    for (dy, &(y0, y1, fy)) in ys.iter().enumerate() {
        for (dx, &(x0, x1, fx)) in xs.iter().enumerate() {
            let (p00, p01, p10, p11) = (pre[y0 * sw + x0], pre[y0 * sw + x1], pre[y1 * sw + x0], pre[y1 * sw + x1]);
            let mut c = [0u32; 4];
            for k in 0..4 {
                let lerp = |v1: i64, v2: i64, f: u32| (v1 << 8) + (v2 - v1) * f as i64;
                let top = lerp(p00[k] as i64, p01[k] as i64, fx);
                let bot = lerp(p10[k] as i64, p11[k] as i64, fx);
                let v = lerp(top, bot, fy);
                c[k] = (((v + (1 << 15)) >> 16) & 0xff) as u32;
            }
            out[(dy * dw + dx) * 4..(dy * dw + dx) * 4 + 4].copy_from_slice(&from_pre(c));
        }
    }
    out
}

/// 고정소수점 좌표 → (왼쪽 칸, 오른쪽 칸, 8비트 비율). 가장자리 밖은 가장자리 칸 하나로.
fn split(v: i64, n: usize) -> (usize, usize, u32) {
    let whole = (v >> 32) as i64;
    let fract = ((v as u64 & 0xffff_ffff) >> 24) as u32;
    if whole < 0 {
        (0, 0, fract)
    } else if whole as usize + 1 >= n {
        let i = (whole as usize).min(n - 1);
        (i, i, fract)
    } else {
        (whole as usize, whole as usize + 1, fract)
    }
}

/// 가장 가까운 픽셀을 그대로 복사한다(값은 바꾸지 않는다). Java2D ScaledBlit 의 고정소수점 계산을 따른다:
/// 정밀도(shift)는 원본 폭·높이가 32비트 정수에 들어가는 만큼, 증분은 잘라내고, 시작은 출력 칸 가운데에서 올림.
/// (Spine 이 고른 원본 칸과 폭 1 ~ 160 · 배율 7가지에서 모두 같았다.)
fn nearest(sw: usize, sh: usize, src: &[u8], dw: usize, dh: usize) -> Vec<u8> {
    let mut tmp = (sw | sh) as u32;
    let mut shift = 0;
    loop {
        tmp <<= 1;
        if tmp == 0 || tmp & 0x8000_0000 != 0 {
            break;
        }
        shift += 1;
    }
    let pick = |s: usize, d: usize| -> Vec<usize> {
        let scale = (1u64 << shift) as f64 * s as f64 / d as f64;
        let inc = scale as i64;
        let loc = (0.5 * scale - 0.5).ceil() as i64;
        (0..d as i64).map(|i| (((loc + i * inc) >> shift).max(0) as usize).min(s - 1)).collect()
    };
    let (xs, ys) = (pick(sw, dw), pick(sh, dh));
    let mut out = Vec::with_capacity(dw * dh * 4);
    for &y in &ys {
        for &x in &xs {
            out.extend_from_slice(&src[(y * sw + x) * 4..(y * sw + x) * 4 + 4]);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn halves_and_keeps_color() {
        let px: Vec<u8> = [[200u8, 100, 50, 255]; 16].concat();
        for m in [Resampling::Area, Resampling::Bilinear, Resampling::Nearest] {
            let (w, h, out) = resize(4, 4, &px, 0.5, m);
            assert_eq!((w, h), (2, 2));
            for p in out.chunks_exact(4) {
                assert_eq!(p, &[200, 100, 50, 255], "{m:?}");
            }
        }
    }

    #[test]
    fn transparent_color_does_not_bleed() {
        // 왼쪽 투명(빨강 RGB), 오른쪽 불투명 파랑
        let mut px = Vec::new();
        for x in 0..4 {
            px.extend_from_slice(if x < 2 { &[255, 0, 0, 0] } else { &[0, 0, 255, 255] });
        }
        let (_, _, out) = resize(4, 1, &px, 0.5, Resampling::Area);
        // 섞여도 색은 파랑 그대로(투명 픽셀의 빨강이 섞이지 않음), 알파만 줄어든다
        assert_eq!(out[4..7], [0, 0, 255]);
        assert!(out[7] > 128);
    }

    #[test]
    fn integer_upscale_area_replicates() {
        let px = [10u8, 20, 30, 40, 50, 60, 70, 80];
        let (w, h, out) = resize(2, 1, &px, 2.0, Resampling::Area);
        assert_eq!((w, h), (4, 2));
        assert_eq!(&out[..16], &[10, 20, 30, 40, 10, 20, 30, 40, 50, 60, 70, 80, 50, 60, 70, 80]);
    }

    #[test]
    fn nearest_keeps_exact_values() {
        // 반투명 픽셀 값이 그대로 남아야 한다(premultiply 를 거치지 않는다)
        let px = [135u8, 19, 73, 157];
        let (_, _, out) = resize(1, 1, &px, 3.0, Resampling::Nearest);
        assert!(out.chunks_exact(4).all(|p| p == px));
    }

    /// Spine 이 고른 원본 칸(같은 비율이라도 원본 크기에 따라 정밀도가 달라 결과가 다르다).
    #[test]
    fn nearest_matches_spine_indices() {
        let idx = |s: usize, d: usize| {
            let px: Vec<u8> = (0..s).flat_map(|x| [x as u8, 0, 0, 255]).collect();
            nearest(s, 1, &px, d, 1).chunks_exact(4).map(|p| p[0] as usize).collect::<Vec<_>>()
        };
        assert_eq!(idx(4, 3), [0, 2, 3]);
        assert_eq!(idx(8, 6), [0, 1, 3, 4, 5, 7]);
        assert_eq!(idx(16, 12), [0, 2, 3, 4, 5, 7, 8, 9, 11, 12, 13, 15]);
        assert_eq!(idx(10, 6), [0, 2, 4, 5, 7, 9]);
        assert_eq!(idx(5, 7), [0, 1, 1, 2, 3, 3, 4]);
    }

    /// Java2D 는 보간할 때 미리 곱한 알파를 거친다: 같은 칸만 섞여도 반투명 색이 조금 바뀐다(Spine 과 같다).
    #[test]
    fn bilinear_premultiply_round_trip() {
        let px = [134u8, 20, 73, 157];
        let (_, _, out) = resize(1, 1, &px, 2.0, Resampling::Bilinear);
        assert_eq!(&out[..4], &[135, 19, 73, 157]);
    }

    #[test]
    fn names() {
        assert_eq!(Resampling::parse("auto"), Some(Resampling::Area));
        assert_eq!(Resampling::parse("bicubic"), Some(Resampling::Area));
        assert_eq!(Resampling::parse("bilinearProgressive"), Some(Resampling::Bilinear));
        assert_eq!(Resampling::parse("nearest"), Some(Resampling::Nearest));
        assert_eq!(Resampling::parse("lanczos"), None);
    }
}
