//! 프로젝트에 저장된 마지막 데이터 내보내기 설정을 찍는다.
//!   cargo run -p spine-project --example dump_export_settings -- 프로젝트.spine

fn main() {
    let path = std::env::args().nth(1).expect("프로젝트 경로");
    let project = spine_project::SpineProject::load(&std::fs::read(&path).unwrap()).unwrap();
    match project.last_export_settings() {
        Some(s) => println!("{s}"),
        None => println!("저장된 내보내기 설정이 없습니다"),
    }
}
