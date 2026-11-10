//! 편집기의 이름 정렬(C# `NaturalComparer`)과 .NET `StringComparer.Ordinal`.
//! C# 문자열은 UTF-16 코드 단위로 비교하므로 여기서도 UTF-16 으로 바꿔 비교한다.

use std::cmp::Ordering;

/// .NET `string.CompareOrdinal`: UTF-16 코드 단위 순서.
pub(crate) fn ordinal_cmp(a: &str, b: &str) -> Ordering {
    a.encode_utf16().cmp(b.encode_utf16())
}

/// 유니코드 Nd(십진 숫자) 문자 묶음의 0 자리(BMP). 각 묶음은 0~9 열 글자가 이어진다.
const ND_ZEROS: &[u16] = &[
    0x0030, 0x0660, 0x06F0, 0x07C0, 0x0966, 0x09E6, 0x0A66, 0x0AE6, 0x0B66, 0x0BE6, 0x0C66, 0x0CE6, 0x0D66, 0x0DE6, 0x0E50,
    0x0ED0, 0x0F20, 0x1040, 0x1090, 0x17E0, 0x1810, 0x1946, 0x19D0, 0x1A80, 0x1A90, 0x1B50, 0x1BB0, 0x1C40, 0x1C50, 0xA620,
    0xA8D0, 0xA900, 0xA9D0, 0xA9F0, 0xAA50, 0xABF0, 0xFF10,
];

/// `CharUnicodeInfo.GetDecimalDigitValue` (Nd 가 아니면 None). `char.IsDigit` 은 이것이 Some 일 때.
fn digit_value(c: u16) -> Option<u16> {
    if (0x30..=0x39).contains(&c) {
        return Some(c - 0x30);
    }
    if c < 0x0660 {
        return None;
    }
    ND_ZEROS.iter().find(|&&z| c >= z && c < z + 10).map(|&z| c - z)
}

/// `char.ToLowerInvariant`(단순 대소문자 대응, 한 코드 단위).
fn to_lower(c: u16) -> u16 {
    if c < 0x80 {
        return (c as u8).to_ascii_lowercase() as u16;
    }
    match char::from_u32(c as u32) {
        Some(ch) => {
            let mut lower = ch.to_lowercase();
            match lower.next() {
                // 단순 대응: 전체 대응의 첫 글자(İ → i). BMP 밖으로 가는 경우는 그대로 둔다.
                Some(l) if (l as u32) <= 0xFFFF => l as u16,
                _ => c,
            }
        }
        None => c, // 홀로 선 대리 코드 단위
    }
}

/// 편집기의 이름 정렬: 글자 단위로 비교하되 양쪽이 숫자면 수로 비교(flare2 < flare10, muzzle-glow < muzzle01).
pub(crate) fn natural_cmp(a: &str, b: &str) -> Ordering {
    let a: Vec<u16> = a.encode_utf16().collect();
    let b: Vec<u16> = b.encode_utf16().collect();
    let mut i = 0;
    let mut j = 0;
    while i < a.len() && j < b.len() {
        if digit_value(a[i]).is_some() && digit_value(b[j]).is_some() {
            let mut i2 = i;
            while i2 < a.len() && digit_value(a[i2]).is_some() {
                i2 += 1;
            }
            let mut j2 = j;
            while j2 < b.len() && digit_value(b[j2]).is_some() {
                j2 += 1;
            }
            let order = compare_runs(&a, i, i2, &b, j, j2);
            if order != Ordering::Equal {
                return order;
            }
            i = i2;
            j = j2;
            continue;
        }

        if a[i] != b[j] {
            let ca = to_lower(a[i]);
            let cb = to_lower(b[j]);
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
    while i < i2 && digit_value(a[i]) == Some(0) {
        i += 1;
    }
    while j < j2 && digit_value(b[j]) == Some(0) {
        j += 1;
    }
    if i2 - i != j2 - j {
        return (i2 - i).cmp(&(j2 - j));
    }
    while i < i2 {
        let x = digit_value(a[i]);
        let y = digit_value(b[j]);
        if x != y {
            return x.cmp(&y);
        }
        i += 1;
        j += 1;
    }
    Ordering::Equal
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn natural() {
        let mut v = vec!["flare10", "flare2", "muzzle01", "muzzle-glow", "B", "a", "A", "flare02", "x"];
        v.sort_by(|a, b| natural_cmp(a, b));
        assert_eq!(v, vec!["A", "a", "B", "flare2", "flare02", "flare10", "muzzle-glow", "muzzle01", "x"]);
        assert_eq!(natural_cmp("flare2", "flare02"), Ordering::Equal);
        assert_eq!(natural_cmp("ab", "abc"), Ordering::Less);
        assert_eq!(natural_cmp("a_", "aZ"), Ordering::Less); // '_'(0x5F) < 'z'(0x7A)
        assert_eq!(natural_cmp("a١٢", "a3"), Ordering::Greater); // 아라비아 숫자 12 > 3
    }

    #[test]
    fn ordinal() {
        assert_eq!(ordinal_cmp("B", "a"), Ordering::Less);
        // UTF-16 순서: U+FF5E > U+1F600(대리 쌍 0xD83D...)
        assert_eq!(ordinal_cmp("\u{FF5E}", "\u{1F600}"), Ordering::Greater);
    }
}
