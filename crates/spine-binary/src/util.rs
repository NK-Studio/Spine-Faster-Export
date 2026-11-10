//! 파이썬 `json_writer.py` 에서 가져온 작은 도우미들(spine-json 에 기대지 않도록 여기에 옮겼다).

use std::cmp::Ordering;

use spine_model::{ModelConstraint, ModelFromProperty, ModelKey, ModelToProperty};

/// 곡선 손잡이가 정해지지 않았다는 표시(0x4f000000).
pub const UNSET: f64 = 2147483648.0;

/// 파이썬 `f32(x)`: float32 로 반올림한 값을 다시 double 로.
#[inline]
pub fn f32r(x: f64) -> f64 {
    x as f32 as f64
}

/// 편집기의 이름 정렬(파이썬 `_nat_cmp`): 글자 단위로 비교하되 양쪽이 숫자면 수로 비교.
pub fn nat_cmp(a: &str, b: &str) -> Ordering {
    let a: Vec<char> = a.chars().collect();
    let b: Vec<char> = b.chars().collect();
    let (mut i, mut j) = (0usize, 0usize);
    while i < a.len() && j < b.len() {
        if a[i].is_ascii_digit() && b[j].is_ascii_digit() {
            let (mut i2, mut j2) = (i, j);
            while i2 < a.len() && a[i2].is_ascii_digit() {
                i2 += 1;
            }
            while j2 < b.len() && b[j2].is_ascii_digit() {
                j2 += 1;
            }
            let o = cmp_digits(&a[i..i2], &b[j..j2]);
            if o != Ordering::Equal {
                return o;
            }
            i = i2;
            j = j2;
            continue;
        }
        if a[i] != b[j] {
            let ca: String = a[i].to_lowercase().collect();
            let cb: String = b[j].to_lowercase().collect();
            if ca != cb {
                return ca.cmp(&cb);
            }
            return a[i].cmp(&b[j]);
        }
        i += 1;
        j += 1;
    }
    let ra = (a.len() - i) as i64;
    let rb = (b.len() - j) as i64;
    (ra - rb).cmp(&0)
}

/// 숫자 문자열을 정수로 비교(자릿수 제한 없음).
fn cmp_digits(x: &[char], y: &[char]) -> Ordering {
    let strip = |s: &[char]| -> Vec<char> {
        let k = s.iter().position(|c| *c != '0').unwrap_or(s.len());
        s[k..].to_vec()
    };
    let (x, y) = (strip(x), strip(y));
    x.len().cmp(&y.len()).then_with(|| x.cmp(&y))
}

/// float32 표현 사이의 간격 수(파이썬 `_ulps`).
pub fn ulps(a: f64, b: f64) -> i64 {
    fn key(x: f64) -> i64 {
        let n = (x as f32).to_bits() as i32 as i64;
        if n >= 0 {
            n
        } else {
            -2147483648 - n
        }
    }
    (key(a) - key(b)).abs()
}

/// 파이썬 `int(round(x * 255))` — round 는 짝수 반올림.
#[inline]
pub fn byte255(x: f64) -> i64 {
    (x * 255.0).round_ties_even() as i64
}

/// `Math.round(x * 10^d) / 10^d` — 값은 float32, 곱셈은 double(파이썬 `round_half_up`).
pub fn round_half_up(x: f64, digits: i32) -> f64 {
    let m = 10f64.powi(digits);
    (f32r(x) * m + 0.5).floor() / m
}

pub fn vals(k: &ModelKey) -> &[f64] {
    k.values.as_deref().unwrap_or(&[])
}

pub fn hds(k: &ModelKey) -> &[[f64; 4]] {
    k.handles.as_deref().unwrap_or(&[])
}

pub fn curve_str(k: &ModelKey) -> &str {
    k.curve.as_deref().unwrap_or("linear")
}

/// 파이썬 `same_key_stepped`: 값이 같은 두 키 사이를 계단으로 낼지.
pub fn same_key_stepped(k: &ModelKey, nxt: &ModelKey, nch: usize) -> bool {
    let (a, b) = (vals(k), vals(nxt));
    if a.is_empty() || a.len() != b.len() {
        return false;
    }
    let ik = a.len() == 5;
    if nch == 4 || nch == 7 {
        // 색은 8비트 값으로 비교한다
        if a.iter()
            .map(|x| byte255(*x))
            .ne(b.iter().map(|x| byte255(*x)))
        {
            return false;
        }
    } else {
        let n = if ik { 2.min(a.len()) } else { a.len() };
        if !a[..n]
            .iter()
            .zip(&b[..n])
            .all(|(x, y)| (x - y).abs() <= 1e-6 || ulps(*x, *y) <= 1)
        {
            return false;
        }
    }
    if curve_str(k) == "linear" {
        return true;
    }
    if ik {
        return false;
    }
    let (hk, hn) = (hds(k), hds(nxt));
    (0..nch).all(|c| {
        c < hk.len()
            && c < hn.len()
            && (hk[c][3] - a[c]).abs() <= 1e-5
            && (hn[c][1] - b[c]).abs() <= 1e-5
    })
}

/// 파이썬 `legacy_properties`: 4.2 이하 변형 제약을 4.3 속성 목록으로.
pub fn legacy_properties(c: &ModelConstraint) -> Vec<ModelFromProperty> {
    const NAMES: [&str; 6] = ["rotate", "x", "y", "scaleX", "scaleY", "shearY"];
    let mut out = Vec::new();
    for (i, k) in NAMES.iter().enumerate() {
        if c.mixes[i] != 0.0 {
            let max = if *k == "scaleX" || *k == "scaleY" {
                1.0
            } else {
                100.0
            };
            out.push(ModelFromProperty {
                name: Some(k.to_string()),
                offset: 0.0,
                to: vec![ModelToProperty {
                    name: Some(k.to_string()),
                    offset: 0.0,
                    max,
                    scale: 1.0,
                }],
            });
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn natural_order() {
        assert_eq!(nat_cmp("flare2", "flare10"), Ordering::Less);
        assert_eq!(nat_cmp("muzzle-glow", "muzzle01"), Ordering::Less);
        assert_eq!(nat_cmp("a", "B"), Ordering::Less);
        assert_eq!(nat_cmp("A", "a"), Ordering::Less);
        assert_eq!(nat_cmp("a01", "a1"), Ordering::Equal);
        assert_eq!(nat_cmp("ab", "a"), Ordering::Greater);
    }

    #[test]
    fn ulps_and_round() {
        assert_eq!(ulps(1.0, 1.0), 0);
        assert_eq!(ulps(1.0, f32::from_bits(1.0f32.to_bits() + 1) as f64), 1);
        assert_eq!(ulps(-0.0, 0.0), 0);
        assert_eq!(
            round_half_up(0.123455, 5),
            (0.123455f32 as f64 * 1e5 + 0.5).floor() / 1e5
        );
        assert_eq!(byte255(0.5), 128); // 127.5 → 짝수 반올림 128
    }
}
