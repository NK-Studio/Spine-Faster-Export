//! `.spine` → `.skel` (spine-project 의 모델 사용). 파이썬 `spine_export.py --format binary` 와 비교용.
//!
//! 사용법: `project_skel <in.spine> <out.skel> <버전> [--clean] [--nonessential]`
//! 스켈레톤이 여럿이면 `<out>` 이름 뒤에 `.<번호>` 를 붙여 모두 쓴다.

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.len() < 4 {
        eprintln!("사용법: project_skel <in.spine> <out.skel> <버전> [--clean] [--nonessential]");
        std::process::exit(2);
    }
    let clean = args.iter().any(|a| a == "--clean");
    let ness = args.iter().any(|a| a == "--nonessential");
    let file = std::fs::read(&args[1]).expect("입력을 읽을 수 없습니다");
    let project = spine_project::SpineProject::load(&file).expect("프로젝트를 읽을 수 없습니다");
    let names = project.skeleton_names();
    for i in 0..names.len() {
        let mut model = project.read_model(i).expect("모델을 만들 수 없습니다");
        if clean {
            spine_binary::clean_model(&mut model);
        }
        let out = if names.len() == 1 {
            args[2].clone()
        } else {
            format!("{}.{i}", args[2])
        };
        match spine_binary::write_binary(&model, &args[3], ness) {
            Ok(data) => std::fs::write(&out, data).expect("쓸 수 없습니다"),
            Err(e) => eprintln!("{}: {e}", names[i]),
        }
    }
}
