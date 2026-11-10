//! JSON 숫자 표기. C# `SpineExportNumbers.cs`(SpineNumbers)를 그대로 옮겼다.
//! 파이썬 PoC(json_writer.py)의 규칙을 정수 산술(큰 정수)로 계산한다.

use std::cmp::Ordering;
use std::sync::OnceLock;

// ------------------------------------------------------------ 작은 큰 정수(부호 없음)

/// 부호 없는 큰 정수. 32비트 자리 묶음, 낮은 자리부터. 끝의 0 자리는 두지 않는다.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Big(Vec<u32>);

impl Big {
    pub fn from_u64(v: u64) -> Big {
        let mut b = Big(vec![v as u32, (v >> 32) as u32]);
        b.trim();
        b
    }

    fn trim(&mut self) {
        while let Some(&0) = self.0.last() {
            self.0.pop();
        }
    }

    pub fn is_zero(&self) -> bool {
        self.0.is_empty()
    }

    pub fn shl(&self, bits: u32) -> Big {
        if self.is_zero() {
            return Big(Vec::new());
        }
        let words = (bits / 32) as usize;
        let rest = bits % 32;
        let mut out = vec![0u32; words];
        if rest == 0 {
            out.extend_from_slice(&self.0);
        } else {
            let mut carry = 0u32;
            for &w in &self.0 {
                out.push((w << rest) | carry);
                carry = w >> (32 - rest);
            }
            if carry != 0 {
                out.push(carry);
            }
        }
        Big(out)
    }

    pub fn one_shl(bits: u32) -> Big {
        Big::from_u64(1).shl(bits)
    }

    pub fn mul_small(&self, m: u32) -> Big {
        let mut out = Vec::with_capacity(self.0.len() + 1);
        let mut carry = 0u64;
        for &w in &self.0 {
            let t = w as u64 * m as u64 + carry;
            out.push(t as u32);
            carry = t >> 32;
        }
        if carry != 0 {
            out.push(carry as u32);
        }
        let mut b = Big(out);
        b.trim();
        b
    }

    pub fn mul(&self, o: &Big) -> Big {
        if self.is_zero() || o.is_zero() {
            return Big(Vec::new());
        }
        if o.0.len() == 1 {
            return self.mul_small(o.0[0]);
        }
        if self.0.len() == 1 {
            return o.mul_small(self.0[0]);
        }
        let mut out = vec![0u32; self.0.len() + o.0.len()];
        for (i, &a) in self.0.iter().enumerate() {
            let mut carry = 0u64;
            for (j, &b) in o.0.iter().enumerate() {
                let t = a as u64 * b as u64 + out[i + j] as u64 + carry;
                out[i + j] = t as u32;
                carry = t >> 32;
            }
            let mut k = i + o.0.len();
            while carry != 0 {
                let t = out[k] as u64 + carry;
                out[k] = t as u32;
                carry = t >> 32;
                k += 1;
            }
        }
        let mut b = Big(out);
        b.trim();
        b
    }

    pub fn add(&self, o: &Big) -> Big {
        let n = self.0.len().max(o.0.len());
        let mut out = Vec::with_capacity(n + 1);
        let mut carry = 0u64;
        for i in 0..n {
            let t = *self.0.get(i).unwrap_or(&0) as u64 + *o.0.get(i).unwrap_or(&0) as u64 + carry;
            out.push(t as u32);
            carry = t >> 32;
        }
        if carry != 0 {
            out.push(carry as u32);
        }
        Big(out)
    }

    /// self - o (self >= o 여야 한다).
    pub fn sub(&self, o: &Big) -> Big {
        debug_assert!(self.cmp_big(o) != Ordering::Less);
        let mut out = Vec::with_capacity(self.0.len());
        let mut borrow = 0i64;
        for i in 0..self.0.len() {
            let mut t = self.0[i] as i64 - *o.0.get(i).unwrap_or(&0) as i64 - borrow;
            if t < 0 {
                t += 1 << 32;
                borrow = 1;
            } else {
                borrow = 0;
            }
            out.push(t as u32);
        }
        let mut b = Big(out);
        b.trim();
        b
    }

    pub fn cmp_big(&self, o: &Big) -> Ordering {
        if self.0.len() != o.0.len() {
            return self.0.len().cmp(&o.0.len());
        }
        for i in (0..self.0.len()).rev() {
            if self.0[i] != o.0[i] {
                return self.0[i].cmp(&o.0[i]);
            }
        }
        Ordering::Equal
    }

    /// 몫이 작을 때(자리 하나 정도)의 나눗셈: (몫, 나머지).
    pub fn divrem_small(&self, d: &Big) -> (u32, Big) {
        let mut q = 0u32;
        let mut r = self.clone();
        while r.cmp_big(d) != Ordering::Less {
            r = r.sub(d);
            q += 1;
        }
        (q, r)
    }
}

impl PartialOrd for Big {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for Big {
    fn cmp(&self, other: &Self) -> Ordering {
        self.cmp_big(other)
    }
}

fn powers() -> &'static Vec<Big> {
    static P: OnceLock<Vec<Big>> = OnceLock::new();
    P.get_or_init(|| {
        let mut table = Vec::with_capacity(400);
        table.push(Big::from_u64(1));
        for i in 1..400 {
            let next = table[i - 1].mul_small(10);
            table.push(next);
        }
        table
    })
}

fn pow10(n: i32) -> Big {
    let n = n as usize;
    let p = powers();
    if n < p.len() {
        return p[n].clone();
    }
    let mut b = p[p.len() - 1].clone();
    for _ in p.len() - 1..n {
        b = b.mul_small(10);
    }
    b
}

/// num/den 과 10^k 비교.
fn compare(num: &Big, den: &Big, k: i32) -> Ordering {
    if k >= 0 {
        num.cmp_big(&den.mul(&pow10(k)))
    } else {
        num.mul(&pow10(-k)).cmp_big(den)
    }
}

// ------------------------------------------------------------ SpineNumbers

pub fn f32r(x: f64) -> f64 {
    x as f32 as f64
}

fn scale(digits: i32) -> f64 {
    match digits {
        2 => 100.0,
        3 => 1000.0,
        4 => 10000.0,
        5 => 100000.0,
        _ => 10f64.powi(digits),
    }
}

/// Math.round(x * 10^d) / 10^d — 값은 float32, 곱셈은 double.
pub fn round_half_up(x: f64, digits: i32) -> f64 {
    let m = scale(digits);
    (f32r(x) * m + 0.5).floor() / m
}

pub fn fmt_round(x: f64, digits: i32) -> String {
    java_double(round_half_up(x, digits))
}

fn int(x: f64) -> String {
    (x as i64).to_string()
}

/// Java Double.toString 과 같은 표기(정수면 '.0' 을 뗀 형태).
pub fn java_double(v: f64) -> String {
    if v == 0.0 {
        return "0".to_string();
    }
    if v == v.trunc() && v.abs() < 1e7 {
        return int(v);
    }
    let a = v.abs();
    let sign = if v < 0.0 { "-" } else { "" };
    let (digits, point) = shortest_double(a);
    if (1e-3..1e7).contains(&a) {
        if point <= 0 {
            return format!("{}0.{}{}", sign, "0".repeat((-point) as usize), digits);
        }
        let p = point as usize;
        return if p >= digits.len() {
            format!("{}{}{}.0", sign, digits, "0".repeat(p - digits.len()))
        } else {
            format!("{}{}.{}", sign, &digits[..p], &digits[p..])
        };
    }
    let mantissa = format!("{}.{}", &digits[..1], if digits.len() > 1 { &digits[1..] } else { "0" });
    format!("{}{}E{}", sign, mantissa, scientific_exponent(a))
}

fn decompose(a: f64) -> (u64, i32) {
    let bits = a.to_bits();
    let biased = ((bits >> 52) & 0x7FF) as i32;
    let frac = bits & 0xF_FFFF_FFFF_FFFF;
    if biased == 0 {
        (frac, -1074)
    } else {
        (frac | (1u64 << 52), biased - 1075)
    }
}

/// a 를 유효숫자 18자리로 반올림했을 때의 십진 지수(파이썬 '%.17e' 의 지수).
fn scientific_exponent(a: f64) -> i32 {
    let (m, e) = decompose(a);
    let mut num = Big::from_u64(m);
    let mut den = Big::from_u64(1);
    if e >= 0 {
        num = num.shl(e as u32);
    } else {
        den = den.shl((-e) as u32);
    }
    let mut k = a.log10().floor() as i32;
    while compare(&num, &den, k) == Ordering::Less {
        k -= 1;
    }
    while compare(&num, &den, k + 1) != Ordering::Less {
        k += 1;
    }
    // n = a / 10^(k-17) 를 절반은 짝수로 반올림: 10^18 이 되면 지수가 하나 오른다.
    // n/d < 10^18 이므로 반올림 결과가 10^18 이 되는 것은 2n >= (2·10^18 - 1)·d 일 때뿐이다
    // (10^18 - 1 은 홀수라 정확히 절반이면 올림).
    let shift = 17 - k;
    let (n, d) = if shift >= 0 { (num.mul(&pow10(shift)), den) } else { (num, den.mul(&pow10(-shift))) };
    let limit = Big::from_u64(2_000_000_000_000_000_000 - 1);
    if n.mul_small(2).cmp_big(&limit.mul(&d)) != Ordering::Less {
        k + 1
    } else {
        k
    }
}

/// 왕복 가능한 가장 짧은 십진수(파이썬 repr 과 같은 값). 값 = 0.digits × 10^point.
fn shortest_double(a: f64) -> (String, i32) {
    let (m, e) = decompose(a);
    let even = (m & 1) == 0;
    let lower_gap = m == (1u64 << 52) && e > -1074;
    let (mut r, mut s, mut mp, mut mm);
    if e >= 0 {
        let be = Big::one_shl(e as u32);
        if lower_gap {
            r = Big::from_u64(m).mul(&be).mul_small(4);
            s = Big::from_u64(4);
            mp = be.mul_small(2);
            mm = be;
        } else {
            r = Big::from_u64(m).mul(&be).mul_small(2);
            s = Big::from_u64(2);
            mp = be.clone();
            mm = be;
        }
    } else if lower_gap {
        r = Big::from_u64(m).mul_small(4);
        s = Big::one_shl((-e + 2) as u32);
        mp = Big::from_u64(2);
        mm = Big::from_u64(1);
    } else {
        r = Big::from_u64(m).mul_small(2);
        s = Big::one_shl((-e + 1) as u32);
        mp = Big::from_u64(1);
        mm = Big::from_u64(1);
    }

    let mut k = (a.log10() - 1e-10).ceil() as i32;
    if k >= 0 {
        s = s.mul(&pow10(k));
    } else {
        let sc = pow10(-k);
        r = r.mul(&sc);
        mp = mp.mul(&sc);
        mm = mm.mul(&sc);
    }

    let ge = |x: &Big, y: &Big| if even { x >= y } else { x > y };

    // 추정한 k 맞추기: (r + m+)/s 가 1 미만(경계 포함 여부는 짝수 규칙)이어야 한다
    while ge(&r.add(&mp), &s) {
        s = s.mul_small(10);
        k += 1;
    }

    loop {
        let r10 = r.mul_small(10);
        let mp10 = mp.mul_small(10);
        if ge(&r10.add(&mp10), &s) {
            break;
        }
        r = r10;
        mp = mp10;
        mm = mm.mul_small(10);
        k -= 1;
    }

    let mut sb: Vec<u8> = Vec::with_capacity(18);
    loop {
        r = r.mul_small(10);
        mp = mp.mul_small(10);
        mm = mm.mul_small(10);
        let (q, rem) = r.divrem_small(&s);
        r = rem;
        let mut d = q as i32;
        let low = if even { r <= mm } else { r < mm };
        let high = ge(&r.add(&mp), &s);
        if !low && !high {
            sb.push(b'0' + d as u8);
            continue;
        }
        if low && high {
            let cmp = r.mul_small(2).cmp_big(&s);
            if cmp == Ordering::Greater || (cmp == Ordering::Equal && (d & 1) != 0) {
                d += 1;
            }
        } else if high {
            d += 1;
        }
        append_with_carry(&mut sb, d, &mut k);
        break;
    }

    let mut digits = String::from_utf8(sb).unwrap();
    while digits.ends_with('0') {
        digits.pop();
    }
    (digits, k)
}

fn append_with_carry(sb: &mut Vec<u8>, d: i32, k: &mut i32) {
    if d < 10 {
        sb.push(b'0' + d as u8);
        return;
    }
    let mut i = sb.len() as isize - 1;
    sb.push(b'0');
    while i >= 0 && sb[i as usize] == b'9' {
        sb[i as usize] = b'0';
        i -= 1;
    }
    if i >= 0 {
        sb[i as usize] += 1;
    } else {
        sb.insert(0, b'1');
        *k += 1;
    }
}

/// 4.3 편집기 숫자 표기에 가장 가까운 규칙: float32 이웃 간격의 3/4 안에서 가장 짧은 십진수(Dragon4 자유 형식).
fn dragon(x: f64) -> String {
    if x == 0.0 {
        return "0".to_string();
    }
    let negative = x < 0.0;
    let x = x.abs();
    let bits = (x as f32).to_bits() as i32;
    let biased = (bits >> 23) & 0xFF;
    let frac = bits & 0x7FFFFF;
    let e = if biased == 0 { -149 } else { biased - 150 };
    let mantissa: u64 = if biased == 0 { frac as u64 } else { (frac | 0x800000) as u64 };

    // 단위 2^(e-2) 로 모두 정수가 된다: 값 = M·4, 위 간격·3/2 = 6, 아래 간격·3/2 = 6(지수 경계면 3)
    let up = if e > 2 { Big::one_shl((e - 2) as u32) } else { Big::from_u64(1) };
    let mut s = if e < 2 { Big::one_shl((2 - e) as u32) } else { Big::from_u64(1) };
    let mut r = Big::from_u64(mantissa * 4).mul(&up);
    let mut mm = up.mul_small(if frac == 0 && biased > 1 { 3 } else { 6 });
    let mut mp = up.mul_small(6);

    // 10^(k-1) <= 값 < 10^k
    let mut k = x.log10().ceil() as i32;
    while compare(&r, &s, k) != Ordering::Less {
        k += 1;
    }
    while compare(&r, &s, k - 1) == Ordering::Less {
        k -= 1;
    }

    if k >= 0 {
        s = s.mul(&pow10(k));
    } else {
        let sc = pow10(-k);
        r = r.mul(&sc);
        mm = mm.mul(&sc);
        mp = mp.mul(&sc);
    }

    let mut digits: Vec<i32> = Vec::with_capacity(14);
    let mut d;
    let mut low;
    let mut high;
    loop {
        r = r.mul_small(10);
        mm = mm.mul_small(10);
        mp = mp.mul_small(10);
        let (q, rem) = r.divrem_small(&s);
        d = q as i32;
        r = rem;
        low = r < mm;
        high = r.add(&mp) > s; // r > s - mp
        if low || high || digits.len() > 12 {
            break;
        }
        digits.push(d);
    }

    if high && (!low || r.mul_small(2) >= s) {
        d += 1;
    }
    digits.push(d);
    let mut i = digits.len() - 1;
    while digits[i] == 10 {
        digits[i] = 0;
        if i == 0 {
            digits.insert(0, 1);
            k += 1;
            break;
        }
        digits[i - 1] += 1;
        i -= 1;
    }

    let mut t: String = digits.iter().map(|&g| (b'0' + g as u8) as char).collect();
    while t.ends_with('0') {
        t.pop();
    }
    if t.is_empty() {
        t = "0".to_string();
    }

    let output = if k <= 0 {
        format!("0.{}{}", "0".repeat((-k) as usize), t)
    } else if k as usize >= t.len() {
        format!("{}{}", t, "0".repeat(k as usize - t.len()))
    } else {
        format!("{}.{}", &t[..k as usize], &t[k as usize..])
    };
    if negative {
        format!("-{}", output)
    } else {
        output
    }
}

/// Java Float.toString 과 같은 가장 짧은 표기. int_ok 면 정수는 '.0' 없이.
pub fn fmt_f32(value: f64, int_ok: bool) -> String {
    let x = f32r(value);
    if x == x.trunc() && x.abs() < 1e7 {
        return if int_ok { int(x) } else { int(x) + ".0" };
    }

    let mut s = dragon(x);
    if !s.contains('.') && !int_ok {
        s.push_str(".0");
    }

    let a = x.abs();
    if a < 1e-3 || a >= 1e7 {
        // Java식 과학 표기. 지수는 십진 표기에서 직접 센다(0.0001f 의 log10 은 -4 보다 살짝 작아 한 자리 어긋난다)
        let plain = s.trim_start_matches('-');
        let joined = plain.replace('.', "");
        let mut digits = joined.trim_start_matches('0').trim_end_matches('0').to_string();
        if digits.is_empty() {
            digits = "0".to_string();
        }
        let dot = plain.find('.');
        let exponent: i32 = if a < 1e-3 {
            let fraction = &plain[dot.map_or(0, |p| p + 1)..];
            -((fraction.len() - fraction.trim_start_matches('0').len()) as i32 + 1)
        } else {
            dot.unwrap_or(plain.len()) as i32 - 1
        };
        s = format!(
            "{}{}.{}E{}",
            if x < 0.0 { "-" } else { "" },
            &digits[..1],
            if digits.len() > 1 { &digits[1..] } else { "0" },
            exponent
        );
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn java_double_cases() {
        assert_eq!(java_double(0.0), "0");
        assert_eq!(java_double(-0.0), "0");
        assert_eq!(java_double(100.0), "100");
        assert_eq!(java_double(-3.0), "-3");
        assert_eq!(java_double(0.5), "0.5");
        assert_eq!(java_double(-0.25), "-0.25");
        assert_eq!(java_double(1e-5), "1.0E-5");
        assert_eq!(java_double(1.5e-5), "1.5E-5");
        assert_eq!(java_double(0.001), "0.001");
        assert_eq!(java_double(0.0001), "1.0E-4");
        assert_eq!(java_double(12345678.0), "1.2345678E7");
        assert_eq!(java_double(1e7), "1.0E7");
        assert_eq!(java_double(9999999.5), "9999999.5");
        assert_eq!(java_double(1234.5678), "1234.5678");
        assert_eq!(java_double(0.1), "0.1");
        assert_eq!(java_double(0.3), "0.3");
        assert_eq!(java_double(0.1 + 0.2), "0.30000000000000004");
        assert_eq!(java_double(1e21), "1.0E21");
        assert_eq!(java_double(1.7976931348623157e308), "1.7976931348623157E308");
        assert_eq!(java_double(5e-324), "5.0E-324"); // 파이썬 repr: 5e-324
        assert_eq!(java_double(123456789012.0), "1.23456789012E11");
        assert_eq!(java_double(2f64.powi(60)), "1.152921504606847E18");
    }

    #[test]
    fn shortest_matches_rust() {
        // Rust 의 가장 짧은 표기(최근접)와 숫자열이 같아야 한다
        let mut x: u64 = 0x9E37_79B9_7F4A_7C15;
        for _ in 0..20000 {
            x ^= x << 13;
            x ^= x >> 7;
            x ^= x << 17;
            let v = f64::from_bits(x >> 2); // 양수, 다양한 지수
            if !v.is_finite() || v == 0.0 {
                continue;
            }
            let (digits, point) = shortest_double(v);
            let r = format!("{:e}", v);
            let (mant, exp) = r.split_once('e').unwrap();
            let rd: String = mant.chars().filter(|c| *c != '.').collect();
            let re: i32 = exp.parse().unwrap();
            assert_eq!(digits, rd, "{:e}", v);
            assert_eq!(point, re + 1, "{:e}", v);
        }
    }

    #[test]
    fn scientific_exponent_cases() {
        assert_eq!(scientific_exponent(1e-5), -5);
        assert_eq!(scientific_exponent(9.99e-5), -5);
        assert_eq!(scientific_exponent(1e7), 7);
        assert_eq!(scientific_exponent(0.0001), -4);
        assert_eq!(scientific_exponent(1e23), 22); // 1e23 은 9.999...e22
        assert_eq!(scientific_exponent(1e-300), -300);
    }

    #[test]
    fn round_cases() {
        assert_eq!(fmt_round(1.005, 2), "1"); // float32 1.005 = 1.00499999...
        assert_eq!(fmt_round(0.125, 2), "0.13");
        assert_eq!(fmt_round(-0.125, 2), "-0.12");
        assert_eq!(fmt_round(0.000004, 5), "0");
        assert_eq!(fmt_round(0.00001, 5), "1.0E-5");
        assert_eq!(fmt_round(0.33333, 4), "0.3333");
        assert_eq!(fmt_round(12.3456, 2), "12.35");
        assert_eq!(fmt_round(100.0, 2), "100");
        assert_eq!(fmt_round(-0.001, 2), "0");
        assert_eq!(fmt_round(-0.004, 2), "0");
        assert_eq!(fmt_round(-0.006, 2), "-0.01");
    }

    #[test]
    fn f32_cases() {
        // 기대값은 C# SpineNumbers.FmtF32 결과(4.3 은 float32 간격의 3/4 규칙이라 Java 보다 짧을 수 있다)
        assert_eq!(fmt_f32(0.0, true), "0");
        assert_eq!(fmt_f32(0.0, false), "0.0");
        assert_eq!(fmt_f32(1.0, false), "1.0");
        assert_eq!(fmt_f32(100.0, true), "100");
        assert_eq!(fmt_f32(1e-5, true), "1.0E-5");
        assert_eq!(fmt_f32(1e7, true), "1.0E7");
        assert_eq!(fmt_f32(12345678.0, true), "1.2345678E7");
        assert_eq!(fmt_f32(9999999.5, true), "1.0E7");
        assert_eq!(fmt_f32(0.001, true), "0.001");
        assert_eq!(fmt_f32(1e-4, true), "1.0E-4");
        assert_eq!(fmt_f32(0.1, true), "0.1");
        assert_eq!(fmt_f32(1.0 / 3.0, true), "0.3333333");
        assert_eq!(fmt_f32(2.0 / 3.0, true), "0.6666667");
        assert_eq!(fmt_f32(1.0 / 30.0, true), "0.03333334");
        assert_eq!(fmt_f32(16777217.0, true), "1.6777216E7");
        assert_eq!(fmt_f32(1e21, true), "1.0E21");
        assert_eq!(fmt_f32(1.17549435e-38, true), "1.1754944E-38");
        assert_eq!(fmt_f32(3.4028234663852886e38, true), "3.4028235E38");
    }

    #[test]
    fn big_basic() {
        let a = Big::from_u64(u64::MAX);
        let b = a.mul(&a);
        let c = b.add(&a).sub(&a);
        assert_eq!(b, c);
        assert_eq!(pow10(20).cmp_big(&Big::from_u64(10_000_000_000_000_000_000)), Ordering::Greater);
        assert_eq!(pow10(19), Big::from_u64(10_000_000_000_000_000_000));
        assert_eq!(Big::one_shl(64).sub(&Big::from_u64(1)), Big::from_u64(u64::MAX));
    }
}
