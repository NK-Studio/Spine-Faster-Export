//! 명령줄. `spine-export-cli` 와 앱 실행 파일(`spine-faster-export export …`)이 함께 쓴다.
//!
//!   list   입력.spine [--json]
//!   export 입력.spine [출력폴더] [옵션…]
//!   (예전 형식) 입력.spine 출력폴더 [옵션…]  = export
//!
//! 설정은 아래 순서로 덮인다(뒤가 이긴다):
//!   기본값 → `--preset` → `--project-settings`(.spine 에 저장된 마지막 내보내기 설정) → `--export-settings` 파일 → 나머지 옵션(적은 순서) → `--unity`
//!
//! 종료 코드: 0 성공, 1 일부 실패, 2 사용법·입력 오류, 3 `--no-overwrite` 인데 덮어쓸 파일이 있음.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::sync::Mutex;
use std::time::Instant;

use serde::Serialize;
use serde_json::json;

use crate::{
    export_many, existing_outputs_for, import_spine_settings, options_for, project_export_settings, resolve_output, resolve_version, version_from_spine, AtlasSettings, ExportOptions,
    JsonFormat, PackSource, PackTarget, SkeletonFormat, SpineProject,
};

/// spine-unity 가 가져오는 확장자.
pub const UNITY_SKEL_EXT: &str = ".skel.bytes";
pub const UNITY_ATLAS_EXT: &str = ".atlas.txt";

/// 앱 실행 파일이 창 대신 명령줄로 동작할 첫 인수인가.
pub fn is_cli_command(first: &str) -> bool {
    matches!(first, "export" | "list" | "help" | "--help" | "-h")
}

const HELP: &str = "\
Quick Spine 명령줄 — Spine 없이 .spine 프로젝트를 런타임 파일(JSON · .skel · atlas)로 내보낸다.

명령:
  list   <입력.spine> [--json]           스켈레톤 목록, 프로젝트 버전, 저장된 내보내기 설정
  export <입력.spine> [<출력폴더>] [옵션] 내보내기(스켈레톤마다 병렬)
  help                                   이 도움말

export 옵션:
  --skeleton <이름>        내보낼 스켈레톤(여러 번 쓸 수 있다). 없으면 모두
  --version <x.y.z>        런타임 버전(3.8.xx, 4.0.xx ~ 4.3.xx). 없으면 프로젝트 버전
  --format json|binary|both  기본 both
  --atlas                  텍스처 아틀라스도 만든다
  --pack-settings <pack.json>  Spine Texture Packer Settings(pack.json)
  --pack-source attachments|imagefolders
  --pack-target perskeleton|single
  --images <폴더>          이미지 폴더(없으면 프로젝트에 적힌 경로)
  --unity                  spine-unity 확장자(.skel.bytes · .atlas.txt)
  --project-settings       .spine 에 저장된 마지막 내보내기 설정을 바탕으로(출력폴더를 빼면 그 설정의 출력 폴더)
  --export-settings <파일> Spine 내보내기 대화상자의 Save 로 만든 설정 JSON
  --preset anc             팀 프리셋
  --json-format json|javascript|minimal · --json-ext <확장자> · --pretty · --no-pretty
  --skel-ext <확장자> · --atlas-ext <확장자> · --max-size <픽셀>
  --nonessential · --no-nonessential · --clean · --no-clean · --export-all · --no-warnings
  --pma · --no-pma · --bleed · --no-bleed · --alias · --no-alias
  --dry-run                쓰지 않고, 최종 설정과 덮어쓸 파일만 보여 준다
  --no-overwrite           출력 폴더에 덮어쓸 파일이 있으면 아무것도 쓰지 않고 끝낸다(종료 코드 3)
  --json                   결과를 JSON 한 덩어리로 stdout 에 쓴다(사람용 줄은 쓰지 않는다)

종료 코드: 0 성공 · 1 일부 실패 · 2 사용법·입력 오류 · 3 덮어쓰기 거부
";

/// 값을 받는 옵션.
const VALUE_FLAGS: &[&str] = &[
    "--preset",
    "--export-settings",
    "--json-format",
    "--json-ext",
    "--pack-source",
    "--pack-target",
    "--format",
    "--version",
    "--skeleton",
    "--images",
    "--pack-settings",
    "--max-size",
    "--skel-ext",
    "--atlas-ext",
];

/// 값이 없는 옵션.
const SWITCH_FLAGS: &[&str] = &[
    "--pretty",
    "--no-pretty",
    "--export-all",
    "--no-warnings",
    "--atlas",
    "--nonessential",
    "--no-nonessential",
    "--clean",
    "--no-clean",
    "--pma",
    "--no-pma",
    "--bleed",
    "--no-bleed",
    "--alias",
    "--no-alias",
    "--unity",
    "--project-settings",
    "--dry-run",
    "--no-overwrite",
    "--json",
];

struct Parsed {
    positional: Vec<String>,
    /// (옵션, 값) — 적은 순서 그대로.
    flags: Vec<(String, Option<String>)>,
}

impl Parsed {
    fn has(&self, f: &str) -> bool {
        self.flags.iter().any(|(k, _)| k == f)
    }

    fn value(&self, f: &str) -> Option<&str> {
        self.flags.iter().rev().find(|(k, _)| k == f).and_then(|(_, v)| v.as_deref())
    }

    fn values(&self, f: &str) -> Vec<String> {
        self.flags.iter().filter(|(k, _)| k == f).filter_map(|(_, v)| v.clone()).collect()
    }
}

fn parse(args: &[String]) -> Result<Parsed, String> {
    let mut p = Parsed { positional: Vec::new(), flags: Vec::new() };
    let mut it = args.iter();
    while let Some(a) = it.next() {
        if VALUE_FLAGS.contains(&a.as_str()) {
            let v = it.next().ok_or_else(|| format!("{a} 에 값이 없습니다"))?;
            p.flags.push((a.clone(), Some(v.clone())));
        } else if SWITCH_FLAGS.contains(&a.as_str()) {
            p.flags.push((a.clone(), None));
        } else if a.starts_with("--") {
            return Err(format!("모르는 옵션: {a} (help 로 목록을 볼 수 있습니다)"));
        } else {
            p.positional.push(a.clone());
        }
    }
    Ok(p)
}

/// 명령줄을 실행한다. `args` 는 프로그램 이름을 뺀 인수.
pub fn run(args: Vec<String>) -> ExitCode {
    let (cmd, rest) = match args.first().map(String::as_str) {
        None | Some("help" | "--help" | "-h") => {
            print!("{HELP}");
            return if args.is_empty() { ExitCode::from(2) } else { ExitCode::SUCCESS };
        }
        Some("list") => ("list", &args[1..]),
        Some("export") => ("export", &args[1..]),
        // 예전 형식: 입력.spine 출력폴더 [옵션]
        Some(_) => ("export", &args[..]),
    };
    let parsed = match parse(rest) {
        Ok(p) => p,
        Err(e) => return usage(&e),
    };
    let json_out = parsed.has("--json");
    let out = Output { json: json_out, lock: Mutex::new(()) };
    match cmd {
        "list" => list(&parsed, &out),
        _ => export(&parsed, &out),
    }
}

fn usage(msg: &str) -> ExitCode {
    eprintln!("{msg}\n사용법: export <입력.spine> [<출력폴더>] [옵션] · list <입력.spine> [--json] · help");
    ExitCode::from(2)
}

/// 사람용 줄 / JSON 출력.
struct Output {
    json: bool,
    lock: Mutex<()>,
}

impl Output {
    /// 사람용 줄(--json 이면 쓰지 않는다).
    fn line(&self, s: &str) {
        if !self.json {
            let _g = self.lock.lock().unwrap();
            let mut o = std::io::stdout().lock();
            let _ = writeln!(o, "{s}");
            let _ = o.flush();
        }
    }

    fn error(&self, s: &str) {
        let _g = self.lock.lock().unwrap();
        eprintln!("{s}");
    }

    fn json<T: Serialize>(&self, v: &T) {
        let mut o = std::io::stdout().lock();
        let _ = writeln!(o, "{}", serde_json::to_string_pretty(v).unwrap_or_default());
        let _ = o.flush();
    }

    /// 오류로 끝낸다(--json 이면 {"error": …} 도 stdout 에).
    fn fail(&self, code: u8, msg: &str) -> ExitCode {
        self.error(msg);
        if self.json {
            self.json(&json!({ "ok": false, "error": msg }));
        }
        ExitCode::from(code)
    }
}

fn load(path: &Path) -> Result<SpineProject, String> {
    if path.extension().and_then(|e| e.to_str()).map(|e| e.eq_ignore_ascii_case("spine")) != Some(true) {
        return Err(format!(".spine 파일이 아닙니다: {}", path.display()));
    }
    let bytes = std::fs::read(path).map_err(|e| format!("읽을 수 없습니다({}): {e}", path.display()))?;
    SpineProject::load(&bytes).map_err(|e| format!("프로젝트를 읽을 수 없습니다({}): {e}", path.display()))
}

fn absolute(p: &Path) -> PathBuf {
    std::path::absolute(p).unwrap_or_else(|_| p.to_path_buf())
}

// ---------------------------------------------------------------- list

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct SavedSettings {
    /// "json" | "binary"
    format: SkeletonFormat,
    /// 설정에 적힌 버전을 이 앱의 버전으로 옮긴 것("" = 프로젝트 버전).
    version: Option<String>,
    /// 출력 폴더(이 컴퓨터의 절대 경로). 다른 OS 의 경로였으면 None.
    output: Option<String>,
    atlas: bool,
    notes: Vec<String>,
    options: ExportOptions,
}

fn saved_settings(project: &SpineProject, path: &Path) -> Option<Result<SavedSettings, String>> {
    let got = project_export_settings(project, &ExportOptions::default())?;
    Some(got.map_err(|e| e.to_string()).map(|mut got| {
        let mut output = None;
        if let Some(o) = got.output.take() {
            match resolve_output(path, &o) {
                Some(p) => output = Some(p.display().to_string()),
                None => got.notes.push(format!("설정에 적힌 출력 폴더 «{o}» 는 이 컴퓨터의 경로가 아니라 쓰지 않습니다.")),
            }
        }
        let version = got.version.as_deref().and_then(|v| version_from_spine(v, project.format_version()));
        if let Some(v) = &version {
            got.options.version = v.clone();
        }
        SavedSettings { format: got.options.format, version, output, atlas: got.options.atlas, notes: got.notes, options: got.options }
    }))
}

fn list(p: &Parsed, out: &Output) -> ExitCode {
    let [input] = p.positional.as_slice() else { return usage("list 에는 입력.spine 하나만 줍니다") };
    let path = absolute(Path::new(input));
    let project = match load(&path) {
        Ok(pr) => pr,
        Err(e) => return out.fail(2, &e),
    };
    let names = project.skeleton_names();
    let saved = saved_settings(&project, &path);
    if out.json {
        let saved_json = match &saved {
            None => json!(null),
            Some(Ok(s)) => serde_json::to_value(s).unwrap_or(json!(null)),
            Some(Err(e)) => json!({ "error": e }),
        };
        out.json(&json!({
            "ok": true,
            "path": path.display().to_string(),
            "formatVersion": project.format_version(),
            "skeletons": names,
            "partialError": project.partial_error(),
            "savedSettings": saved_json,
            "knownVersions": crate::KNOWN_VERSIONS,
        }));
        return ExitCode::SUCCESS;
    }
    out.line(&format!("{} (Spine {})", path.display(), project.format_version()));
    if let Some(e) = project.partial_error() {
        out.line(&format!("일부를 읽지 못했습니다: {e}"));
    }
    out.line(&format!("스켈레톤 {}개:", names.len()));
    for n in &names {
        out.line(&format!("  {n}"));
    }
    match saved {
        None => out.line("저장된 내보내기 설정: 없음"),
        Some(Err(e)) => out.line(&format!("저장된 내보내기 설정을 읽을 수 없습니다: {e}")),
        Some(Ok(s)) => {
            let fmt = if s.format == SkeletonFormat::Json { "JSON" } else { "바이너리" };
            let ver = match s.version.as_deref() {
                Some("") | None => "프로젝트 버전".to_string(),
                Some(v) => v.to_string(),
            };
            out.line(&format!(
                "저장된 내보내기 설정: {fmt}, {ver}, atlas {}, 출력 {}  (--project-settings 로 씀)",
                if s.atlas { "켬" } else { "끔" },
                s.output.as_deref().unwrap_or("-")
            ));
            for n in &s.notes {
                out.line(&format!("  참고: {n}"));
            }
        }
    }
    ExitCode::SUCCESS
}

// ---------------------------------------------------------------- export

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ItemResult {
    skeleton: String,
    ok: bool,
    files: Vec<String>,
    notes: Vec<String>,
    error: Option<String>,
    #[serde(skip)]
    order: usize,
}

/// 옵션을 순서대로 적용한다(설정 파일·프리셋은 앞에서 이미 깔았다).
fn apply_flags(p: &Parsed, opts: &mut ExportOptions) -> Result<(), String> {
    for (k, v) in &p.flags {
        let v = v.as_deref().unwrap_or("");
        match k.as_str() {
            "--json-format" => {
                opts.json_format = match v.to_ascii_lowercase().as_str() {
                    "json" => JsonFormat::Json,
                    "javascript" => JsonFormat::JavaScript,
                    "minimal" => JsonFormat::Minimal,
                    _ => return Err(format!("--json-format 은 json · javascript · minimal 중 하나입니다: {v}")),
                }
            }
            "--json-ext" => opts.json_ext = v.to_string(),
            "--pretty" => opts.pretty_print = true,
            "--no-pretty" => opts.pretty_print = false,
            "--export-all" => opts.export_all = true,
            "--no-warnings" => opts.warnings = false,
            "--pack-source" => {
                opts.pack_source = match v.to_ascii_lowercase().as_str() {
                    "attachments" => PackSource::Attachments,
                    "imagefolders" => PackSource::ImageFolders,
                    _ => return Err(format!("--pack-source 는 attachments · imagefolders 중 하나입니다: {v}")),
                }
            }
            "--pack-target" => {
                opts.pack_target = match v.to_ascii_lowercase().as_str() {
                    "perskeleton" => PackTarget::PerSkeleton,
                    "single" => PackTarget::Single,
                    _ => return Err(format!("--pack-target 은 perskeleton · single 중 하나입니다: {v}")),
                }
            }
            "--format" => {
                opts.format = match v.to_ascii_lowercase().as_str() {
                    "json" => SkeletonFormat::Json,
                    "binary" => SkeletonFormat::Binary,
                    "both" => SkeletonFormat::Both,
                    _ => return Err(format!("--format 은 json · binary · both 중 하나입니다: {v}")),
                }
            }
            "--version" => opts.version = v.to_string(),
            "--atlas" => opts.atlas = true,
            "--images" => opts.images_dir = Some(absolute(Path::new(v))),
            "--nonessential" => opts.nonessential = Some(true),
            "--no-nonessential" => opts.nonessential = Some(false),
            "--clean" => opts.clean = true,
            "--no-clean" => opts.clean = false,
            "--pack-settings" => {
                let text = std::fs::read_to_string(v).map_err(|e| format!("팩 설정을 읽을 수 없습니다({v}): {e}"))?;
                let pack: AtlasSettings = serde_json::from_str(&text).map_err(|e| format!("팩 설정(pack.json) 형식이 아닙니다({v}): {e}"))?;
                opts.pack = pack.normalize();
            }
            "--pma" => opts.pack.pma = true,
            "--no-pma" => opts.pack.pma = false,
            "--bleed" => opts.pack.bleed = true,
            "--no-bleed" => opts.pack.bleed = false,
            "--alias" => opts.pack.alias = true,
            "--no-alias" => opts.pack.alias = false,
            "--max-size" => {
                let n: u32 = v.parse().map_err(|_| format!("--max-size 는 픽셀 수입니다: {v}"))?;
                opts.pack = opts.pack.clone().with_max_size(n);
            }
            "--skel-ext" => opts.skel_ext = v.to_string(),
            "--atlas-ext" => opts.pack.atlas_extension = v.to_string(),
            _ => {}
        }
    }
    if p.has("--unity") {
        // 직접 준 확장자가 이긴다
        if !p.has("--skel-ext") {
            opts.skel_ext = UNITY_SKEL_EXT.into();
        }
        if !p.has("--atlas-ext") {
            opts.pack.atlas_extension = UNITY_ATLAS_EXT.into();
        }
    }
    Ok(())
}

fn export(p: &Parsed, out: &Output) -> ExitCode {
    let (input, out_arg) = match p.positional.as_slice() {
        [i] => (i, None),
        [i, o] => (i, Some(o)),
        [] => return usage("입력.spine 이 없습니다"),
        _ => return usage(&format!("인수가 너무 많습니다: {}", p.positional.join(" "))),
    };
    let path = absolute(Path::new(input));
    let project = match load(&path) {
        Ok(pr) => pr,
        Err(e) => return out.fail(2, &e),
    };
    let mut setting_notes: Vec<String> = Vec::new();

    // 기본값 → 프리셋
    let mut opts = match p.value("--preset") {
        None => ExportOptions::default(),
        Some("anc") => ExportOptions::preset_anc(),
        Some(other) => return out.fail(2, &format!("알 수 없는 프리셋: {other} (anc 만 있습니다)")),
    };
    // → 프로젝트에 저장된 설정
    let mut saved_output: Option<String> = None;
    if p.has("--project-settings") {
        match project_export_settings(&project, &opts) {
            None => setting_notes.push("프로젝트에 저장된 내보내기 설정이 없어 기본값을 씁니다.".into()),
            Some(Err(e)) => return out.fail(2, &format!("프로젝트의 내보내기 설정을 읽을 수 없습니다: {e}")),
            Some(Ok(got)) => {
                opts = got.options;
                if let Some(v) = got.version.as_deref().and_then(|v| version_from_spine(v, project.format_version())) {
                    opts.version = v;
                }
                if let Some(o) = got.output {
                    match resolve_output(&path, &o) {
                        Some(d) => saved_output = Some(d.display().to_string()),
                        None => setting_notes.push(format!("설정에 적힌 출력 폴더 «{o}» 는 이 컴퓨터의 경로가 아니라 쓰지 않습니다.")),
                    }
                }
                setting_notes.extend(got.notes);
            }
        }
    }
    // → 설정 파일
    if let Some(f) = p.value("--export-settings") {
        match std::fs::read_to_string(f).map_err(|e| e.to_string()).and_then(|t| import_spine_settings(&t, &opts).map_err(|e| e.to_string())) {
            Ok(got) => {
                opts = got.options;
                if let Some(v) = got.version.as_deref().and_then(|v| version_from_spine(v, project.format_version())) {
                    opts.version = v;
                }
                setting_notes.extend(got.notes);
            }
            Err(e) => return out.fail(2, &format!("내보내기 설정을 읽을 수 없습니다({f}): {e}")),
        }
    }
    // → 나머지 옵션
    if let Err(e) = apply_flags(p, &mut opts) {
        return out.fail(2, &e);
    }
    let version = match resolve_version(&project, &opts) {
        Ok(v) => v,
        Err(e) => return out.fail(2, &e.to_string()),
    };
    let opts = options_for(&project, &opts);

    let out_dir = match out_arg.cloned().or(saved_output) {
        Some(d) => absolute(Path::new(&d)),
        None => return usage("출력폴더가 없습니다(--project-settings 로 저장된 출력 폴더를 쓰려면 프로젝트에 설정이 있어야 합니다)"),
    };

    // 스켈레톤 고르기
    let names = project.skeleton_names();
    let wanted = p.values("--skeleton");
    let indices: Vec<usize> = if wanted.is_empty() {
        (0..names.len()).collect()
    } else {
        let mut v = Vec::new();
        let mut missing = Vec::new();
        for w in &wanted {
            match names.iter().position(|n| n == w) {
                Some(i) if !v.contains(&i) => v.push(i),
                Some(_) => {}
                None => missing.push(w.as_str()),
            }
        }
        if !missing.is_empty() {
            return out.fail(2, &format!("없는 스켈레톤: {} (있는 것: {})", missing.join(", "), names.join(", ")));
        }
        v
    };
    if indices.is_empty() {
        return out.fail(2, "내보낼 스켈레톤이 없습니다");
    }
    if !out_dir.is_dir() {
        setting_notes.push(format!("출력 폴더가 아직 없어 새로 만듭니다: {}", out_dir.display()));
    }
    for n in &setting_notes {
        out.line(&format!("참고: {n}"));
    }

    let existing = existing_outputs_for(&project, &path, &indices, &out_dir, &opts);
    let existing_s: Vec<String> = existing.iter().map(|f| f.display().to_string()).collect();
    let selected: Vec<&String> = indices.iter().map(|&i| &names[i]).collect();

    if p.has("--dry-run") {
        if out.json {
            out.json(&json!({
                "ok": true,
                "dryRun": true,
                "input": path.display().to_string(),
                "outDir": out_dir.display().to_string(),
                "version": version,
                "skeletons": selected,
                "wouldOverwrite": existing_s,
                "notes": setting_notes,
                "options": opts,
            }));
        } else {
            out.line(&format!("입력: {}", path.display()));
            out.line(&format!("출력: {}", out_dir.display()));
            out.line(&format!("버전: {version}"));
            out.line(&format!("스켈레톤: {}", selected.iter().map(|s| s.as_str()).collect::<Vec<_>>().join(", ")));
            out.line(&format!("덮어쓸 파일 {}개{}", existing_s.len(), if existing_s.is_empty() { String::new() } else { format!(": {}", existing_s.join(", ")) }));
            out.line(&format!("설정: {}", serde_json::to_string(&opts).unwrap_or_default()));
        }
        return ExitCode::SUCCESS;
    }
    if p.has("--no-overwrite") && !existing.is_empty() {
        let names: Vec<String> = existing.iter().map(|f| f.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default()).collect();
        let msg = format!("덮어쓸 파일이 {}개 있어 내보내지 않았습니다(--dry-run 으로 목록을 볼 수 있습니다): {}", existing.len(), crate::short_list(&names));
        out.error(&msg);
        if out.json {
            out.json(&json!({ "ok": false, "error": msg, "wouldOverwrite": existing_s }));
        }
        return ExitCode::from(3);
    }

    let start = Instant::now();
    let results: Mutex<Vec<ItemResult>> = Mutex::new(Vec::new());
    let (succeeded, failed) = export_many(&project, &path, &indices, &out_dir, &opts, |item| {
        let order = item.index.unwrap_or(usize::MAX);
        let r = match item.result {
            Ok(rep) => {
                for n in &rep.notes {
                    out.line(&format!("참고: {n}"));
                }
                for f in &rep.files {
                    out.line(&format!("저장: {}", f.display()));
                }
                ItemResult { skeleton: item.label, ok: true, files: rep.files.iter().map(|f| f.display().to_string()).collect(), notes: rep.notes, error: None, order }
            }
            Err(e) => {
                out.error(&format!("{}: 내보낼 수 없습니다: {e}", item.label));
                ItemResult { skeleton: item.label, ok: false, files: vec![], notes: vec![], error: Some(e.to_string()), order }
            }
        };
        results.lock().unwrap().push(r);
    });
    let mut results = results.into_inner().unwrap();
    results.sort_by_key(|r| r.order);
    let elapsed = start.elapsed();
    if out.json {
        out.json(&json!({
            "ok": failed == 0,
            "input": path.display().to_string(),
            "outDir": out_dir.display().to_string(),
            "version": version,
            "succeeded": succeeded,
            "failed": failed,
            "overwritten": existing_s,
            "elapsedMs": elapsed.as_millis() as u64,
            "notes": setting_notes,
            "results": results,
        }));
    } else {
        out.line(&format!("{succeeded}개 성공, {failed}개 실패 · {:.2}초", elapsed.as_secs_f64()));
    }
    if failed > 0 {
        ExitCode::FAILURE
    } else {
        ExitCode::SUCCESS
    }
}
