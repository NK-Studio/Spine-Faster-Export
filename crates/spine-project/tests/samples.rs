//! 예제 프로젝트(Spine 3.8 ~ 4.2 공식 예제)를 모두 읽고 모든 스켈레톤 모델을 만든다.
//! 표본 폴더는 환경 변수 SPINE_SAMPLES 로 준다(없으면 건너뛴다).
//!   SPINE_SAMPLES=/path/to/Spine-Custom-Editor/data cargo test -p spine-project

use std::path::{Path, PathBuf};

fn collect(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else { return };
    for e in entries.flatten() {
        let p = e.path();
        if p.is_dir() {
            collect(&p, out);
        } else if p.extension().is_some_and(|x| x == "spine") {
            out.push(p);
        }
    }
}

#[test]
fn load_all_samples() {
    let Ok(root) = std::env::var("SPINE_SAMPLES") else {
        eprintln!("SPINE_SAMPLES 가 없어 건너뜁니다");
        return;
    };
    let mut files = Vec::new();
    collect(Path::new(&root), &mut files);
    files.sort();
    if files.is_empty() {
        eprintln!("표본이 없어 건너뜁니다: {root}");
        return;
    }

    let mut failures = Vec::new();
    let mut loaded = 0;
    for f in &files {
        let bytes = std::fs::read(f).unwrap();
        let t = std::time::Instant::now();
        match spine_project::SpineProject::load(&bytes) {
            Ok(p) => {
                let names = p.skeleton_names();
                assert!(!names.is_empty());
                for i in 0..names.len() {
                    let m = p.read_model(i).unwrap_or_else(|e| panic!("{}: 모델 {i}: {e}", f.display()));
                    assert!(!m.bones.is_empty(), "{}: 뼈가 없습니다", f.display());
                }
                // 저장된 내보내기 설정은 JSON · 바이너리 설정 모양이어야 한다
                if let Some(s) = p.last_export_settings() {
                    let class = s["class"].as_str().unwrap_or("");
                    assert!(class == "export-json" || class == "export-binary", "{}: 내보내기 설정 class {class}", f.display());
                    assert!(s["extension"].is_string(), "{}: 내보내기 설정에 확장자가 없습니다", f.display());
                }
                loaded += 1;
                eprintln!("{:>8.2} ms  {}  {}", t.elapsed().as_secs_f64() * 1000.0, p.format_version(), f.display());
            }
            Err(e) => failures.push(format!("{}: {e}", f.display())),
        }
    }

    // 지원하지 않는 베타 포맷(4.2.32-beta, 4.2.61-beta)만 실패해야 한다(C# · 파이썬 PoC 와 같다).
    for f in &failures {
        eprintln!("실패: {f}");
        assert!(f.contains("지원하지 않는 프로젝트 포맷 버전"), "예상하지 못한 실패: {f}");
    }
    eprintln!("읽음 {loaded} / {}", files.len());
}
