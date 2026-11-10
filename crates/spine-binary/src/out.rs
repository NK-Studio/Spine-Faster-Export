//! 바이너리 출력 버퍼(파이썬 `binary_writer.Out`). 숫자는 빅엔디언.

#[derive(Default)]
pub struct Out {
    pub b: Vec<u8>,
}

impl Out {
    pub fn byte(&mut self, v: i64) {
        self.b.push((v & 0xFF) as u8);
    }

    pub fn boolean(&mut self, v: bool) {
        self.byte(if v { 1 } else { 0 });
    }

    /// 부호 없는 varint(32비트로 자른 뒤 7비트씩).
    pub fn varint(&mut self, v: i64) {
        let mut v = (v as u64) & 0xFFFF_FFFF;
        loop {
            let x = (v & 0x7F) as u8;
            v >>= 7;
            if v != 0 {
                self.b.push(x | 0x80);
            } else {
                self.b.push(x);
                return;
            }
        }
    }

    /// readInt(false): zigzag.
    pub fn svarint(&mut self, v: i64) {
        self.varint((v << 1) ^ (v >> 31));
    }

    pub fn int(&mut self, v: i64) {
        self.b
            .extend_from_slice(&((v as u64 & 0xFFFF_FFFF) as u32).to_be_bytes());
    }

    /// float32 빅엔디언(`struct.pack('>f', v)`).
    pub fn float(&mut self, v: f64) {
        self.b.extend_from_slice(&(v as f32).to_be_bytes());
    }

    /// 2바이트 부호 있는 정수(`struct.pack('>h', v)`).
    pub fn short(&mut self, v: i64) {
        self.b.extend_from_slice(&(v as i16).to_be_bytes());
    }

    /// 길이(글자 수 + 1, null 이면 0) 뒤에 UTF-8 바이트.
    pub fn string(&mut self, s: Option<&str>) {
        match s {
            None => self.varint(0),
            Some(s) => {
                self.varint(s.chars().count() as i64 + 1);
                self.b.extend_from_slice(s.as_bytes());
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn varint_bytes() {
        let mut o = Out::default();
        o.varint(0);
        o.varint(127);
        o.varint(128);
        o.varint(300);
        o.varint(-1);
        assert_eq!(
            o.b,
            vec![0, 127, 0x80, 1, 0xAC, 2, 0xFF, 0xFF, 0xFF, 0xFF, 0x0F]
        );
    }

    #[test]
    fn svarint_zigzag() {
        let mut o = Out::default();
        o.svarint(0);
        o.svarint(-1);
        o.svarint(1);
        o.svarint(-64);
        assert_eq!(o.b, vec![0, 1, 2, 127]);
    }

    #[test]
    fn strings_and_numbers() {
        let mut o = Out::default();
        o.string(None);
        o.string(Some(""));
        o.string(Some("가a"));
        assert_eq!(o.b, vec![0, 1, 3, 0xEA, 0xB0, 0x80, b'a']);
        let mut o = Out::default();
        o.float(1.0);
        o.int(0xFFFF_FFFF);
        o.short(-2);
        o.byte(0x1FF);
        assert_eq!(
            o.b,
            vec![0x3F, 0x80, 0, 0, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFE, 0xFF]
        );
    }
}
