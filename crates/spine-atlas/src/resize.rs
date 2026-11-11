//! 배율(Scale) 적용: RGBA 이미지를 늘리거나 줄인다.
//!
//! 미리 곱한 알파로 바꿔 가로·세로로 나눠 거르고(줄일 때는 넓이 평균, 늘릴 때는 삼각 필터) 다시 되돌린다.
//! 투명 픽셀의 색이 가장자리로 번지지 않는다.

/// (새 폭, 새 높이, RGBA). 새 크기는 반올림, 최소 1.
pub fn resize(w: usize, h: usize, rgba: &[u8], scale: f32) -> (usize, usize, Vec<u8>) {
    let nw = ((w as f64 * scale as f64).round() as usize).max(1);
    let nh = ((h as f64 * scale as f64).round() as usize).max(1);
    if nw == w && nh == h {
        return (w, h, rgba.to_vec());
    }
    let mut pm: Vec<f32> = Vec::with_capacity(w * h * 4);
    for p in rgba.chunks_exact(4) {
        let a = p[3] as f32 / 255.0;
        pm.extend_from_slice(&[p[0] as f32 * a, p[1] as f32 * a, p[2] as f32 * a, p[3] as f32]);
    }
    // 가로
    let xw = weights(w, nw);
    let mut tmp = vec![0f32; nw * h * 4];
    for y in 0..h {
        for (x, (start, ws)) in xw.iter().enumerate() {
            let mut acc = [0f32; 4];
            for (k, wt) in ws.iter().enumerate() {
                let s = (y * w + start + k) * 4;
                for c in 0..4 {
                    acc[c] += pm[s + c] * wt;
                }
            }
            tmp[(y * nw + x) * 4..(y * nw + x) * 4 + 4].copy_from_slice(&acc);
        }
    }
    // 세로
    let yw = weights(h, nh);
    let mut out = vec![0u8; nw * nh * 4];
    for (y, (start, ws)) in yw.iter().enumerate() {
        for x in 0..nw {
            let mut acc = [0f32; 4];
            for (k, wt) in ws.iter().enumerate() {
                let s = ((start + k) * nw + x) * 4;
                for c in 0..4 {
                    acc[c] += tmp[s + c] * wt;
                }
            }
            let a = acc[3].clamp(0.0, 255.0);
            let o = (y * nw + x) * 4;
            if a > 0.0 {
                let inv = 255.0 / a;
                for c in 0..3 {
                    out[o + c] = (acc[c] * inv).round().clamp(0.0, 255.0) as u8;
                }
            }
            out[o + 3] = a.round() as u8;
        }
    }
    (nw, nh, out)
}

/// 출력 칸마다 (원본 시작 칸, 정규화한 무게들).
fn weights(src: usize, dst: usize) -> Vec<(usize, Vec<f32>)> {
    let ratio = src as f64 / dst as f64;
    if ratio > 1.0 {
        // 줄일 때: 출력 칸이 덮는 원본 구간의 넓이 평균(Spine 의 결과와 같다)
        return (0..dst)
            .map(|i| {
                let (a, b) = (i as f64 * ratio, ((i + 1) as f64 * ratio).min(src as f64));
                let lo = (a.floor() as usize).min(src - 1);
                let hi = ((b.ceil() as usize).max(lo + 1) - 1).min(src - 1);
                let ws: Vec<f32> = (lo..=hi).map(|j| (((j + 1) as f64).min(b) - (j as f64).max(a)).max(0.0) as f32).collect();
                let sum: f32 = ws.iter().sum();
                if sum > 0.0 {
                    (lo, ws.iter().map(|w| w / sum).collect())
                } else {
                    (lo, vec![1.0])
                }
            })
            .collect();
    }
    let support = ratio.max(1.0);
    (0..dst)
        .map(|i| {
            let center = (i as f64 + 0.5) * ratio - 0.5;
            let lo = ((center - support).floor() as isize).max(0) as usize;
            let hi = ((center + support).ceil() as isize).min(src as isize - 1).max(0) as usize;
            let mut ws: Vec<f32> = (lo..=hi).map(|j| (1.0 - ((j as f64 - center).abs() / support)).max(0.0) as f32).collect();
            let sum: f32 = ws.iter().sum();
            if sum > 0.0 {
                ws.iter_mut().for_each(|w| *w /= sum);
            } else {
                // 가장 가까운 칸
                let near = (center.round().max(0.0) as usize).min(src - 1);
                return (near, vec![1.0]);
            }
            (lo, ws)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn halves_and_keeps_color() {
        let px: Vec<u8> = [[200u8, 100, 50, 255]; 16].concat();
        let (w, h, out) = resize(4, 4, &px, 0.5);
        assert_eq!((w, h), (2, 2));
        for p in out.chunks_exact(4) {
            assert_eq!(p, &[200, 100, 50, 255]);
        }
    }

    #[test]
    fn transparent_color_does_not_bleed() {
        // 왼쪽 투명(빨강 RGB), 오른쪽 불투명 파랑
        let mut px = Vec::new();
        for x in 0..4 {
            px.extend_from_slice(if x < 2 { &[255, 0, 0, 0] } else { &[0, 0, 255, 255] });
        }
        let (_, _, out) = resize(4, 1, &px, 0.5);
        // 섞여도 색은 파랑 그대로(투명 픽셀의 빨강이 섞이지 않음), 알파만 줄어든다
        assert_eq!(out[4..7], [0, 0, 255]);
        assert!(out[7] > 128);
    }
}
