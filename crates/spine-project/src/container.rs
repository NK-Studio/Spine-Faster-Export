//! .spine 컨테이너 = zlib 헤더 없는 raw deflate 스트림 하나(C# `SpineProjectContainer`).
//! 4.3.17부터 뒤에 20바이트 꼬리가 붙는다: 00 00 00 00 | 압축 길이(BE 4) | 8바이트 | 02 0b 02 0b.

use std::io::Read;

use spine_model::{SpineError, SpineResult};

pub const TRAILER_LENGTH: usize = 20;

/// 압축을 푼 본문.
pub fn decompress(file: &[u8]) -> SpineResult<Vec<u8>> {
    let deflate_length = if looks_like_trailer(file) { file.len() - TRAILER_LENGTH } else { file.len() };
    let mut out = Vec::with_capacity(file.len() * 8);
    let mut dec = flate2::read::DeflateDecoder::new(&file[..deflate_length]);
    dec.read_to_end(&mut out)
        .map_err(|e| SpineError::Format(format!("raw deflate 스트림이 아닙니다: {e}")))?;
    Ok(out)
}

fn looks_like_trailer(file: &[u8]) -> bool {
    if file.len() <= TRAILER_LENGTH {
        return false;
    }
    let t = file.len() - TRAILER_LENGTH;
    if file[t] != 0 || file[t + 1] != 0 || file[t + 2] != 0 || file[t + 3] != 0 {
        return false;
    }
    let length = u32::from_be_bytes([file[t + 4], file[t + 5], file[t + 6], file[t + 7]]) as usize;
    length == t && file[t + 16] == 0x02 && file[t + 17] == 0x0b && file[t + 18] == 0x02 && file[t + 19] == 0x0b
}
