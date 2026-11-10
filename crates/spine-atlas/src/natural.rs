//! 이름 정렬. C# `NaturalComparer`(SpineExportNumbers.cs) 와 `StringComparer.Ordinal` 을 옮겼다.
//! C# 처럼 UTF-16 코드 단위로 비교한다.

use std::cmp::Ordering;

/// 유니코드 Nd(10진 숫자) 블록의 '0' 자리(BMP). C# `char.IsDigit` · `GetDecimalDigitValue` 대응.
const ND_ZEROS: &[u16] = &[
    0x0030, 0x0660, 0x06F0, 0x07C0, 0x0966, 0x09E6, 0x0A66, 0x0AE6, 0x0B66, 0x0BE6, 0x0C66, 0x0CE6, 0x0D66,
    0x0DE6, 0x0E50, 0x0ED0, 0x0F20, 0x1040, 0x1090, 0x17E0, 0x1810, 0x1946, 0x19D0, 0x1A80, 0x1A90, 0x1B50,
    0x1BB0, 0x1C40, 0x1C50, 0xA620, 0xA8D0, 0xA900, 0xA9D0, 0xA9F0, 0xAA50, 0xABF0, 0xFF10,
];

fn digit(c: u16) -> Option<u32> {
    if (0x30..0x3A).contains(&c) {
        return Some((c - 0x30) as u32);
    }
    if c < 0x0660 {
        return None;
    }
    ND_ZEROS.iter().find(|&&z| c >= z && c < z + 10).map(|&z| (c - z) as u32)
}

fn lower(c: u16) -> u16 {
    if (0xD800..0xE000).contains(&c) {
        return c;
    }
    match char::from_u32(c as u32) {
        Some(ch) => {
            let mut it = ch.to_lowercase();
            match (it.next(), it.next()) {
                (Some(l), None) if (l as u32) < 0x10000 => l as u32 as u16,
                _ => c,
            }
        }
        None => c,
    }
}

/// 편집기의 이름 정렬: 글자 단위로 비교하되 양쪽이 숫자면 수로 비교(flare2 < flare10, muzzle-glow < muzzle01).
pub fn natural_cmp(a: &str, b: &str) -> Ordering {
    let a: Vec<u16> = a.encode_utf16().collect();
    let b: Vec<u16> = b.encode_utf16().collect();
    natural_cmp16(&a, &b)
}

pub fn natural_cmp16(a: &[u16], b: &[u16]) -> Ordering {
    let (mut i, mut j) = (0usize, 0usize);
    while i < a.len() && j < b.len() {
        if digit(a[i]).is_some() && digit(b[j]).is_some() {
            let mut i2 = i;
            while i2 < a.len() && digit(a[i2]).is_some() {
                i2 += 1;
            }
            let mut j2 = j;
            while j2 < b.len() && digit(b[j2]).is_some() {
                j2 += 1;
            }
            let order = compare_runs(a, i, i2, b, j, j2);
            if order != Ordering::Equal {
                return order;
            }
            i = i2;
            j = j2;
            continue;
        }
        if a[i] != b[j] {
            let (ca, cb) = (lower(a[i]), lower(b[j]));
            if ca != cb {
                return ca.cmp(&cb);
            }
            return a[i].cmp(&b[j]);
        }
        i += 1;
        j += 1;
    }
    ((a.len() - i) as i64 - (b.len() - j) as i64).cmp(&0)
}

/// 숫자 구간을 값으로 비교한다(길이에 제한이 없게 자릿수로).
fn compare_runs(a: &[u16], mut i: usize, i2: usize, b: &[u16], mut j: usize, j2: usize) -> Ordering {
    while i < i2 && digit(a[i]) == Some(0) {
        i += 1;
    }
    while j < j2 && digit(b[j]) == Some(0) {
        j += 1;
    }
    if i2 - i != j2 - j {
        return (i2 - i).cmp(&(j2 - j));
    }
    while i < i2 {
        let (x, y) = (digit(a[i]), digit(b[j]));
        if x != y {
            return x.cmp(&y);
        }
        i += 1;
        j += 1;
    }
    Ordering::Equal
}

/// C# `StringComparer.Ordinal`: UTF-16 코드 단위 순.
pub fn ordinal_cmp(a: &str, b: &str) -> Ordering {
    a.encode_utf16().cmp(b.encode_utf16())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn natural_order() {
        assert_eq!(natural_cmp("flare2", "flare10"), Ordering::Less);
        assert_eq!(natural_cmp("muzzle-glow", "muzzle01"), Ordering::Less);
        assert_eq!(natural_cmp("a01", "a1"), Ordering::Equal);
        assert_eq!(natural_cmp("A", "b"), Ordering::Less);
        assert_eq!(natural_cmp("B", "b"), Ordering::Less);
        assert_eq!(natural_cmp("ab", "a"), Ordering::Greater);
        assert_eq!(natural_cmp("x/Ｚ１０", "x/Ｚ２"), Ordering::Greater);
    }
}
