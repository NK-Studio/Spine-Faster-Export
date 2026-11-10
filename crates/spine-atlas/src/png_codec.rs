//! PNG 읽기·쓰기. C# `SpinePngCodec.cs` 와 같은 RGBA 를 낸다.
//!
//! - 그림에 필요한 청크(IHDR·PLTE·tRNS·IDAT·IEND)만 읽고 나머지는 건너뛴다(포토샵 메타데이터가 수 MB 인 파일이 흔하다).
//! - 8·16비트 그레이/RGB/팔레트(1·2·4·8비트)/알파. 16비트는 위 바이트만 쓴다.
//! - tRNS 는 팔레트에만 적용한다(그레이·RGB 의 색 키 투명은 C# 처럼 무시). 감마·색 공간도 무시.
//! - 압축 해제·필터 복원은 `png` 크레이트에 맡기고(CRC·Adler 검사는 끔, C# 도 보지 않는다), 색 변환은 직접 한다.

use std::fs::File;
use std::io::{BufReader, Cursor, Read};
use std::path::Path;

use spine_model::{SpineError, SpineResult};

const SIGNATURE: [u8; 8] = [0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A];

fn format_err(msg: String) -> SpineError {
    SpineError::Format(msg)
}

/// PNG 파일 → (너비, 높이, straight RGBA, 위쪽 줄부터).
pub fn decode_file(path: &Path) -> SpineResult<(u32, u32, Vec<u8>)> {
    let shown = path.display().to_string();
    let file = File::open(path)?;
    let file_len = file.metadata()?.len();
    let mut file = BufReader::with_capacity(64 * 1024, file);
    let truncated = || format_err(format!("{}: PNG 데이터가 잘렸습니다", shown));

    let mut head = [0u8; 8];
    if read_full(&mut file, &mut head)? != 8 || head != SIGNATURE {
        return Err(format_err(format!("PNG가 아닙니다: {}", shown)));
    }
    let mut needed: Vec<u8> = Vec::with_capacity(file_len.min(64 << 20) as usize);
    needed.extend_from_slice(&head);
    let mut pos: u64 = 8;
    let mut trns: Option<Vec<u8>> = None;
    while read_full(&mut file, &mut head)? == 8 {
        pos += 8;
        let length = i32::from_be_bytes([head[0], head[1], head[2], head[3]]);
        if length < 0 || pos + length as u64 + 4 > file_len {
            return Err(truncated());
        }
        let kind = &head[4..8];
        let end = kind == b"IEND";
        let keep = end || kind == b"IHDR" || kind == b"PLTE" || kind == b"tRNS" || kind == b"IDAT";
        if !keep {
            file.seek_relative(length as i64 + 4)?;
            pos += length as u64 + 4;
            continue;
        }
        let n = length as usize + 4; // 본문 + CRC
        if kind == b"tRNS" {
            // tRNS 는 직접 적용한다(png 크레이트의 길이 검사를 피하고 그레이·RGB 색 키를 무시하려고).
            let mut body = vec![0u8; n];
            if read_full(&mut file, &mut body)? != n {
                return Err(truncated());
            }
            body.truncate(length as usize);
            trns = Some(body);
        } else {
            needed.extend_from_slice(&head);
            let at = needed.len();
            needed.resize(at + n, 0);
            if read_full(&mut file, &mut needed[at..])? != n {
                return Err(truncated());
            }
        }
        pos += n as u64;
        if end {
            break;
        }
    }
    decode_chunks(needed, trns, &shown)
}

fn read_full<R: Read>(r: &mut R, buf: &mut [u8]) -> SpineResult<usize> {
    let mut got = 0;
    while got < buf.len() {
        let n = r.read(&mut buf[got..])?;
        if n == 0 {
            break;
        }
        got += n;
    }
    Ok(got)
}

/// PNG 바이트(tRNS 를 뺀 것) → RGBA.
fn decode_chunks(data: Vec<u8>, trns: Option<Vec<u8>>, name: &str) -> SpineResult<(u32, u32, Vec<u8>)> {
    use png::{BitDepth, ColorType};

    let bad = |e: png::DecodingError| format_err(format!("{}: PNG를 읽을 수 없습니다({})", name, e));
    let mut decoder = png::Decoder::new_with_limits(Cursor::new(data), png::Limits { bytes: usize::MAX });
    decoder.set_transformations(png::Transformations::IDENTITY);
    decoder.ignore_checksums(true);
    decoder.set_ignore_text_chunk(true);
    let mut reader = decoder.read_info().map_err(bad)?;
    let (w, h, depth, ctype, palette) = {
        let info = reader.info();
        (info.width, info.height, info.bit_depth, info.color_type, info.palette.as_ref().map(|p| p.to_vec()))
    };
    let depth_bits = depth as u8 as u32;
    let ok_depth = depth == BitDepth::Eight
        || depth == BitDepth::Sixteen
        || (ctype == ColorType::Indexed && matches!(depth, BitDepth::One | BitDepth::Two | BitDepth::Four));
    if !ok_depth {
        return Err(format_err(format!("{}비트 PNG는 지원하지 않습니다: {}", depth_bits, name)));
    }
    if w == 0 || h == 0 || w > 65536 || h > 65536 || (w as u64) * (h as u64) * 4 > i32::MAX as u64 {
        return Err(format_err(format!("{}: PNG 크기가 잘못되었습니다", name)));
    }
    let size = reader.output_buffer_size().ok_or_else(|| format_err(format!("{}: PNG 크기가 잘못되었습니다", name)))?;
    let mut buf = vec![0u8; size];
    let frame = reader.next_frame(&mut buf).map_err(bad)?;
    let line_size = frame.line_size;
    let (w, h) = (w as usize, h as usize);
    let step = if depth == BitDepth::Sixteen { 2 } else { 1 };
    let mut out = vec![0u8; w * h * 4];

    for y in 0..h {
        let line = &buf[y * line_size..(y + 1) * line_size];
        let row = &mut out[y * w * 4..(y + 1) * w * 4];
        match ctype {
            ColorType::Indexed => {
                let palette = palette.as_deref().unwrap_or(&[]);
                let trns = trns.as_deref();
                let d = depth_bits as usize;
                for x in 0..w {
                    let idx = if d == 8 {
                        line[x] as usize
                    } else {
                        ((line[x * d / 8] >> (8 - d - (x * d) % 8)) & ((1u8 << d) - 1)) as usize
                    };
                    if idx * 3 + 2 >= palette.len() {
                        return Err(format_err(format!("{}: PNG 팔레트 범위를 벗어났습니다", name)));
                    }
                    let o = x * 4;
                    row[o..o + 3].copy_from_slice(&palette[idx * 3..idx * 3 + 3]);
                    row[o + 3] = match trns {
                        Some(t) if idx < t.len() => t[idx],
                        _ => 255,
                    };
                }
            }
            ColorType::Grayscale => {
                for x in 0..w {
                    let g = line[x * step];
                    row[x * 4..x * 4 + 4].copy_from_slice(&[g, g, g, 255]);
                }
            }
            ColorType::Rgb => {
                for x in 0..w {
                    let s = x * 3 * step;
                    row[x * 4..x * 4 + 4].copy_from_slice(&[line[s], line[s + step], line[s + 2 * step], 255]);
                }
            }
            ColorType::GrayscaleAlpha => {
                for x in 0..w {
                    let s = x * 2 * step;
                    let g = line[s];
                    row[x * 4..x * 4 + 4].copy_from_slice(&[g, g, g, line[s + step]]);
                }
            }
            ColorType::Rgba => {
                if step == 1 {
                    row.copy_from_slice(&line[..w * 4]);
                } else {
                    for x in 0..w {
                        let s = x * 8;
                        row[x * 4..x * 4 + 4].copy_from_slice(&[line[s], line[s + 2], line[s + 4], line[s + 6]]);
                    }
                }
            }
        }
    }
    Ok((w as u32, h as u32, out))
}

/// RGBA → 8비트 RGBA PNG. 빠른 압축(fdeflate)을 쓴다. 바이트는 C# 과 다르지만 픽셀은 같다.
pub fn encode(width: u32, height: u32, rgba: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(rgba.len() / 4 + 1024);
    {
        let mut enc = png::Encoder::new(&mut out, width, height);
        enc.set_color(png::ColorType::Rgba);
        enc.set_depth(png::BitDepth::Eight);
        enc.set_compression(png::Compression::Fast);
        let mut writer = enc.write_header().expect("PNG 헤더 쓰기");
        writer.write_image_data(rgba).expect("PNG 데이터 쓰기");
        writer.finish().expect("PNG 마무리");
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip() {
        let rgba: Vec<u8> = (0..(7 * 5 * 4)).map(|i| (i * 37 % 256) as u8).collect();
        let bytes = encode(7, 5, &rgba);
        let mut data = bytes.clone();
        // decode_chunks 는 tRNS 없는 바이트를 받는다
        let (w, h, px) = decode_chunks(std::mem::take(&mut data), None, "t").unwrap();
        assert_eq!((w, h), (7, 5));
        assert_eq!(px, rgba);
    }
}
