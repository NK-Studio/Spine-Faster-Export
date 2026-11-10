//! C# SpineNumbers 로 만든 기대값(tests/data/numbers_cs.txt)과 비교한다.
//! 줄 형식: "<종류> <double 비트(i64)> <C# 결과>". 종류: jd(JavaDouble), r<자릿수>(FmtRound), f/fn(FmtF32 intOk=true/false).

use spine_json::number::{fmt_f32, fmt_round, java_double};

#[test]
fn numbers_match_csharp() {
    let text = include_str!("data/numbers_cs.txt");
    let mut failures = Vec::new();
    let mut count = 0;
    for line in text.lines() {
        let mut parts = line.split(' ');
        let kind = parts.next().unwrap();
        let bits: i64 = parts.next().unwrap().parse().unwrap();
        let expected = parts.next().unwrap();
        let v = f64::from_bits(bits as u64);
        let got = match kind {
            "jd" => java_double(v),
            "f" => fmt_f32(v, true),
            "fn" => fmt_f32(v, false),
            r => fmt_round(v, r[1..].parse().unwrap()),
        };
        count += 1;
        if got != expected {
            failures.push(format!("{} {:e}: C# {} / Rust {}", kind, v, expected, got));
        }
    }
    assert!(count > 7000);
    assert!(failures.is_empty(), "{} 개 불일치:\n{}", failures.len(), failures[..failures.len().min(30)].join("\n"));
}
