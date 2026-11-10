//! 명령줄 내보내기(파이썬 `spine_export.py` 와 같은 옵션). 결과를 PoC 와 비교하는 데 쓴다.
//!
//!   spine-export-cli 입력.spine 출력폴더 --version 4.2.43 [--format json|binary|both] [--atlas] ...

use std::path::PathBuf;
use std::process::ExitCode;
use std::time::Instant;

use spine_export::{export_single_atlas, export_skeleton, import_spine_settings, wants_single_atlas, ExportOptions, JsonFormat, PackSource, PackTarget, SkeletonFormat, SpineProject};

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let mut positional = Vec::new();
    let mut opts = ExportOptions::default();
    let mut skeleton: Option<String> = None;
    // 직접 준 값 > 프리셋 > 기본값: 프리셋을 먼저 찾아 바탕으로 깐다.
    if let Some(i) = args.iter().position(|a| a == "--preset") {
        match args.get(i + 1).map(String::as_str) {
            Some("anc") => opts = ExportOptions::preset_anc(),
            other => {
                eprintln!("알 수 없는 프리셋: {other:?}");
                return ExitCode::from(2);
            }
        }
    }
    // Spine 내보내기 설정 JSON(Export 대화상자의 Save)도 바탕으로 깐다.
    if let Some(i) = args.iter().position(|a| a == "--export-settings") {
        let path = args.get(i + 1).cloned().unwrap_or_default();
        match std::fs::read_to_string(&path).map_err(|e| e.to_string()).and_then(|t| import_spine_settings(&t, &opts).map_err(|e| e.to_string())) {
            Ok(got) => {
                for n in &got.notes {
                    println!("참고: {n}");
                }
                opts = got.options;
            }
            Err(e) => {
                eprintln!("내보내기 설정을 읽을 수 없습니다({path}): {e}");
                return ExitCode::from(2);
            }
        }
    }
    let mut i = 0;
    while i < args.len() {
        let a = args[i].as_str();
        let mut value = || {
            i += 1;
            args.get(i).cloned().unwrap_or_default()
        };
        match a {
            "--preset" | "--export-settings" => {
                value();
            }
            "--json-format" => {
                opts.json_format = match value().to_ascii_lowercase().as_str() {
                    "javascript" => JsonFormat::JavaScript,
                    "minimal" => JsonFormat::Minimal,
                    _ => JsonFormat::Json,
                }
            }
            "--json-ext" => opts.json_ext = value(),
            "--pretty" => opts.pretty_print = true,
            "--no-pretty" => opts.pretty_print = false,
            "--export-all" => opts.export_all = true,
            "--no-warnings" => opts.warnings = false,
            "--pack-source" => opts.pack_source = if value() == "imagefolders" { PackSource::ImageFolders } else { PackSource::Attachments },
            "--pack-target" => opts.pack_target = if value() == "single" { PackTarget::Single } else { PackTarget::PerSkeleton },
            "--format" => {
                opts.format = match value().as_str() {
                    "json" => SkeletonFormat::Json,
                    "binary" => SkeletonFormat::Binary,
                    _ => SkeletonFormat::Both,
                }
            }
            "--version" => opts.version = value(),
            "--skeleton" => skeleton = Some(value()),
            "--atlas" => opts.atlas = true,
            "--images" => opts.images_dir = Some(PathBuf::from(value())),
            "--nonessential" => opts.nonessential = Some(true),
            "--no-nonessential" => opts.nonessential = Some(false),
            "--clean" => opts.clean = true,
            "--no-clean" => opts.clean = false,
            "--pack-settings" => {
                // Spine 이 저장한 pack.json(Texture Packer Settings)
                let path = value();
                match std::fs::read_to_string(&path).map_err(|e| e.to_string()).and_then(|t| serde_json::from_str(&t).map_err(|e| e.to_string())) {
                    Ok(p) => opts.pack = spine_export::AtlasSettings::normalize(p),
                    Err(e) => {
                        eprintln!("팩 설정을 읽을 수 없습니다({path}): {e}");
                        return ExitCode::from(2);
                    }
                }
            }
            "--pma" => opts.pack.pma = true,
            "--no-pma" => opts.pack.pma = false,
            "--bleed" => opts.pack.bleed = true,
            "--no-bleed" => opts.pack.bleed = false,
            "--alias" => opts.pack.alias = true,
            "--no-alias" => opts.pack.alias = false,
            "--max-size" => opts.pack = opts.pack.clone().with_max_size(value().parse().unwrap_or(4096)),
            "--skel-ext" => opts.skel_ext = value(),
            "--atlas-ext" => opts.pack.atlas_extension = value(),
            _ => positional.push(args[i].clone()),
        }
        i += 1;
    }
    if positional.len() != 2 {
        eprintln!("사용법: spine-export-cli 입력.spine 출력폴더 [--version 4.2.43] [--format json|binary|both] [--atlas] [--pack-settings pack.json] [--export-settings spine-export.json] [--json-format json|javascript|minimal] [--pretty] [--export-all] [--pack-source attachments|imagefolders] [--pack-target perskeleton|single] [--preset anc] ...");
        return ExitCode::from(2);
    }
    let input = PathBuf::from(&positional[0]);
    let out = PathBuf::from(&positional[1]);
    let start = Instant::now();
    let bytes = match std::fs::read(&input) {
        Ok(b) => b,
        Err(e) => {
            eprintln!("읽을 수 없습니다: {e}");
            return ExitCode::FAILURE;
        }
    };
    let project = match SpineProject::load(&bytes) {
        Ok(p) => p,
        Err(e) => {
            eprintln!("읽을 수 없습니다: {e}");
            return ExitCode::FAILURE;
        }
    };
    let names = project.skeleton_names();
    let indices: Vec<usize> = match &skeleton {
        Some(s) => match names.iter().position(|n| n == s) {
            Some(i) => vec![i],
            None => {
                eprintln!("스켈레톤 '{s}'이 없습니다. 있는 것: {}", names.join(", "));
                return ExitCode::FAILURE;
            }
        },
        None => (0..names.len()).collect(),
    };
    let mut failed = 0;
    if wants_single_atlas(&opts) {
        match export_single_atlas(&project, &indices, &input, &out, &opts) {
            Ok(report) => {
                for n in &report.notes {
                    println!("참고: {n}");
                }
                for f in &report.files {
                    println!("저장: {}", f.display());
                }
            }
            Err(e) => {
                failed += 1;
                eprintln!("아틀라스를 만들 수 없습니다: {e}");
            }
        }
    }
    for i in indices {
        match export_skeleton(&project, i, &input, &out, &opts) {
            Ok(report) => {
                for n in &report.notes {
                    println!("참고: {n}");
                }
                for f in &report.files {
                    println!("저장: {}", f.display());
                }
            }
            Err(e) => {
                failed += 1;
                eprintln!("{}: 내보낼 수 없습니다: {e}", names[i]);
            }
        }
    }
    println!("{:.2}초", start.elapsed().as_secs_f64());
    if failed > 0 {
        ExitCode::FAILURE
    } else {
        ExitCode::SUCCESS
    }
}
