//! 페이지 JPEG 인코딩(순수 Rust `jpeg-encoder`). Spine(Java ImageIO = libjpeg)과 같은 머리말·표·4:2:0 으로 쓴다.
//!
//! `jpeg-encoder` 는 4:2:0 에서 2×2 칸의 한 점만 골라 크로마를 줄인다. libjpeg 는 네 점의 평균을 쓰므로
//! (빨강·검정이 맞닿은 가장자리에서 차이가 크다) 크로마를 미리 2×2 평균으로 바꿔 넘긴다.
//! 양자화 표도 Java 처럼 실수 배율로 만든다(IJG 의 정수 배율과 품질 0.5 아래에서 값이 갈린다).

use jpeg_encoder::{Encoder, ImageBuffer, JpegColorType, QuantizationTableType, SamplingFactor};
use spine_model::{SpineError, SpineResult};

/// JPEG 표준 Annex K 표(자연 순서).
const K1_LUMA: [u16; 64] = [
    16, 11, 10, 16, 24, 40, 51, 61, 12, 12, 14, 19, 26, 58, 60, 55, 14, 13, 16, 24, 40, 57, 69, 56, 14, 17, 22, 29, 51, 87, 80, 62, 18, 22, 37, 56, 68, 109, 103,
    77, 24, 35, 55, 64, 81, 104, 113, 92, 49, 64, 78, 87, 103, 121, 120, 101, 72, 92, 95, 98, 112, 100, 103, 99,
];
const K2_CHROMA: [u16; 64] = [
    17, 18, 24, 47, 99, 99, 99, 99, 18, 21, 26, 66, 99, 99, 99, 99, 24, 26, 56, 99, 99, 99, 99, 99, 47, 66, 99, 99, 99, 99, 99, 99, 99, 99, 99, 99, 99, 99, 99, 99,
    99, 99, 99, 99, 99, 99, 99, 99, 99, 99, 99, 99, 99, 99, 99, 99, 99, 99, 99, 99, 99, 99, 99, 99,
];

/// Java `JPEG.convertToLinearQuality` + `JPEGQTable.getScaledInstance(scale, forceBaseline = true)`.
fn java_table(base: &[u16; 64], quality: f32) -> [u16; 64] {
    let q = if quality <= 0.0 { 0.01 } else { quality.min(1.0) };
    let scale: f32 = if q < 0.5 { 0.5 / q } else { 2.0 - q * 2.0 };
    base.map(|v| ((v as f32 * scale + 0.5) as i32).clamp(1, 255) as u16)
}

/// libjpeg `rgb_ycc_convert`(16비트 고정소수). Y 는 ONE_HALF, Cb · Cr 은 ONE_HALF - 1 로 반올림한다.
fn rgb_ycc(r: u8, g: u8, b: u8) -> (u8, u8, u8) {
    let (r, g, b) = (r as i32, g as i32, b as i32);
    const HALF: i32 = 1 << 15;
    const OFFSET: i32 = 128 << 16;
    let y = (19595 * r + 38470 * g + 7471 * b + HALF) >> 16;
    let cb = (-11059 * r - 21709 * g + 32768 * b + OFFSET + HALF - 1) >> 16;
    let cr = (32768 * r - 27439 * g - 5329 * b + OFFSET + HALF - 1) >> 16;
    (y as u8, cb as u8, cr as u8)
}

/// 픽셀마다 (Y, 2×2 평균 Cb, 2×2 평균 Cr). 알파는 버린다.
struct Ycc {
    w: usize,
    h: usize,
    y: Vec<u8>,
    cb: Vec<u8>,
    cr: Vec<u8>,
}

impl Ycc {
    fn new(w: usize, h: usize, rgba: &[u8]) -> Ycc {
        let n = w * h;
        let (mut y, mut cb, mut cr) = (vec![0u8; n], vec![0u8; n], vec![0u8; n]);
        for (i, p) in rgba.chunks_exact(4).enumerate().take(n) {
            (y[i], cb[i], cr[i]) = rgb_ycc(p[0], p[1], p[2]);
        }
        // libjpeg h2v2_downsample: 칸 밖(홀수 폭·높이)은 가장자리 픽셀을 되풀이하고, 반올림 보정은 열마다 1, 2 를 번갈아.
        let avg = |c: &mut Vec<u8>| {
            let src = c.clone();
            for by in (0..h).step_by(2) {
                let y1 = (by + 1).min(h - 1);
                for (k, bx) in (0..w).step_by(2).enumerate() {
                    let x1 = (bx + 1).min(w - 1);
                    let sum = src[by * w + bx] as u32 + src[by * w + x1] as u32 + src[y1 * w + bx] as u32 + src[y1 * w + x1] as u32;
                    let v = ((sum + 1 + (k as u32 & 1)) >> 2) as u8;
                    for yy in [by, y1] {
                        c[yy * w + bx] = v;
                        c[yy * w + x1] = v;
                    }
                }
            }
        };
        avg(&mut cb);
        avg(&mut cr);
        Ycc { w, h, y, cb, cr }
    }
}

impl ImageBuffer for Ycc {
    fn get_jpeg_color_type(&self) -> JpegColorType {
        JpegColorType::Ycbcr
    }

    fn width(&self) -> u16 {
        self.w as u16
    }

    fn height(&self) -> u16 {
        self.h as u16
    }

    fn fill_buffers(&self, y: u16, buffers: &mut [Vec<u8>; 4]) {
        let row = y as usize * self.w..(y as usize + 1) * self.w;
        buffers[0].extend_from_slice(&self.y[row.clone()]);
        buffers[1].extend_from_slice(&self.cb[row.clone()]);
        buffers[2].extend_from_slice(&self.cr[row]);
    }
}

/// RGBA(위쪽 줄부터) → JPEG. quality 는 0~1(Spine `jpegQuality`, Java ImageIO 의 품질과 같은 뜻).
pub fn encode(width: u32, height: u32, rgba: &[u8], quality: f32) -> SpineResult<Vec<u8>> {
    if width == 0 || height == 0 || width > u16::MAX as u32 || height > u16::MAX as u32 {
        return Err(SpineError::Usage(format!("JPEG 로 쓸 수 없는 페이지 크기입니다: {width}×{height}(1~65535).")));
    }
    let mut out = Vec::new();
    // 품질 값은 표를 직접 주므로 쓰이지 않는다(90 이상이면 4:4:4 를 고르는 것만 아래에서 되돌린다)
    let mut enc = Encoder::new(&mut out, 90);
    enc.set_quantization_tables(
        QuantizationTableType::Custom(Box::new(java_table(&K1_LUMA, quality))),
        QuantizationTableType::Custom(Box::new(java_table(&K2_CHROMA, quality))),
    );
    // Spine 은 품질과 상관없이 4:2:0 이다
    enc.set_sampling_factor(SamplingFactor::F_2_2);
    enc.encode_image(Ycc::new(width as usize, height as usize, rgba)).map_err(|e| SpineError::Format(format!("JPEG 인코딩: {e}")))?;
    Ok(out)
}
