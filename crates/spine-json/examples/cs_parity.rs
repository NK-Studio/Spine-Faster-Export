//! C# 출력기와 바이트 단위 비교(개발용).
//!
//! C# 오라클(패키지의 SkeletonModelReader + SkeletonJsonWriter 를 그대로 컴파일한 콘솔 앱)이 만든 폴더를 읽는다:
//!   <n>.model.json            — Newtonsoft 로 직렬화한 C# SkeletonModel(double 은 "d<비트>" 문자열)
//!   <n>.<버전>.<ne|es>.json   — C# SkeletonJsonWriter.Write(model, 버전, nonessential) 결과
//!   <n>.<버전>.<ne|es>.err    — C# 가 예외를 던진 경우
//!
//!   cargo run -p spine-json --example cs_parity -- <폴더>

use std::fs;
use std::path::{Path, PathBuf};

#[path = "../tests/support/cs_model.rs"]
mod cs_model;

fn walk(dir: &Path, out: &mut Vec<PathBuf>) {
    for e in fs::read_dir(dir).unwrap() {
        let p = e.unwrap().path();
        if p.is_dir() {
            walk(&p, out);
        } else if p.to_string_lossy().ends_with(".model.json") {
            out.push(p);
        }
    }
}

fn first_diff(a: &str, b: &str) -> String {
    let at = a.bytes().zip(b.bytes()).position(|(x, y)| x != y).unwrap_or(a.len().min(b.len()));
    let from = at.saturating_sub(80);
    let snip = |s: &str| s.get(from..(at + 80).min(s.len())).unwrap_or("").to_string();
    format!("@{}\n  C#  : {}\n  Rust: {}", at, snip(a), snip(b))
}

fn main() {
    let root = PathBuf::from(std::env::args().nth(1).expect("폴더"));
    let mut models = Vec::new();
    walk(&root, &mut models);
    models.sort();
    let (mut ok, mut bad) = (0, 0);
    for mp in &models {
        let stem = mp.to_string_lossy().trim_end_matches(".model.json").to_string();
        let m = cs_model::model(&serde_json::from_str(&fs::read_to_string(mp).unwrap()).unwrap());
        for ver in ["4.0.64", "4.1.24", "4.2.43", "4.3.26"] {
            for (ne, tag) in [(true, "ne"), (false, "es")] {
                let base = format!("{stem}.{ver}.{tag}");
                let got = spine_json::write_json(&m, ver, ne);
                let json = Path::new(&format!("{base}.json")).to_path_buf();
                if json.exists() {
                    let want = fs::read_to_string(&json).unwrap();
                    match got {
                        Ok(g) if g == want => ok += 1,
                        Ok(g) => {
                            bad += 1;
                            println!("DIFF {base}: {}", first_diff(&want, &g));
                        }
                        Err(e) => {
                            bad += 1;
                            println!("ERR  {base}: Rust 오류 {e}");
                        }
                    }
                } else {
                    let want = fs::read_to_string(format!("{base}.err")).unwrap_or_default();
                    match got {
                        Err(_) => ok += 1,
                        Ok(_) => {
                            bad += 1;
                            println!("ERR  {base}: C# 는 오류({want}), Rust 는 성공");
                        }
                    }
                }
            }
        }
    }
    println!("모델 {} 개, 일치 {ok}, 불일치 {bad}", models.len());
}
