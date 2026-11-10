//! Spine Faster Export — Tauri 백엔드.
//!
//! 프런트엔드가 부르는 명령:
//! - `open_project(path)`: `.spine` 을 읽어 스켈레톤 목록을 돌려준다.
//! - `export_skeletons(indices, outDir, options)`: 고른 스켈레톤을 병렬로 내보낸다(`export-progress` 이벤트).
//! - `load_pack_settings(path)` / `save_pack_settings(path, settings)`: Spine `pack.json`(Texture Packer Settings).
//! - `load_export_settings(path, options)` / `save_export_settings(path, options, input, output)`: Spine 내보내기 설정 JSON.
//! - `take_pending_open()`: 앱을 켤 때 파인더/탐색기에서 넘겨받은 파일.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Instant;

use serde::Serialize;
use spine_export::{export_skeleton, AtlasSettings, ExportOptions, ExportReport, SpineProject};
use tauri::{AppHandle, Emitter, Manager, State};

struct LoadedProject {
    path: PathBuf,
    project: SpineProject,
}

#[derive(Default)]
struct AppState {
    project: Mutex<Option<Arc<LoadedProject>>>,
    /// 프런트엔드가 준비되기 전에 들어온 열기 요청.
    pending_open: Mutex<Vec<String>>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ProjectInfo {
    path: String,
    file_name: String,
    format_version: String,
    skeletons: Vec<String>,
    load_ms: u128,
    partial_error: Option<String>,
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
struct ExportProgress {
    /// 스켈레톤 번호. 여러 스켈레톤을 묶은 아틀라스(Single atlas)는 None.
    index: Option<usize>,
    skeleton: String,
    ok: bool,
    message: Option<String>,
    files: Vec<String>,
    notes: Vec<String>,
    done: usize,
    total: usize,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ExportSummary {
    succeeded: usize,
    failed: usize,
    out_dir: String,
    elapsed_ms: u128,
}

fn current(state: &State<'_, AppState>) -> Result<Arc<LoadedProject>, String> {
    state.project.lock().unwrap().clone().ok_or_else(|| "열린 프로젝트가 없습니다.".to_string())
}

#[tauri::command]
async fn open_project(path: String, state: State<'_, AppState>) -> Result<ProjectInfo, String> {
    let p = PathBuf::from(&path);
    if p.extension().and_then(|e| e.to_str()).map(|e| e.eq_ignore_ascii_case("spine")) != Some(true) {
        return Err(format!(".spine 파일이 아닙니다: {path}"));
    }
    let start = Instant::now();
    let loaded = tauri::async_runtime::spawn_blocking(move || -> Result<LoadedProject, String> {
        let bytes = std::fs::read(&p).map_err(|e| format!("파일을 읽을 수 없습니다: {e}"))?;
        let project = SpineProject::load(&bytes).map_err(|e| e.to_string())?;
        Ok(LoadedProject { path: p, project })
    })
    .await
    .map_err(|e| e.to_string())??;
    let info = ProjectInfo {
        path: loaded.path.display().to_string(),
        file_name: loaded.path.file_name().map(|f| f.to_string_lossy().into_owned()).unwrap_or_default(),
        format_version: loaded.project.format_version().to_string(),
        skeletons: loaded.project.skeleton_names(),
        load_ms: start.elapsed().as_millis(),
        partial_error: loaded.project.partial_error().map(str::to_string),
    };
    *state.project.lock().unwrap() = Some(Arc::new(loaded));
    Ok(info)
}

#[tauri::command]
fn close_project(state: State<'_, AppState>) {
    *state.project.lock().unwrap() = None;
}

#[tauri::command]
async fn export_skeletons(
    app: AppHandle,
    indices: Vec<usize>,
    out_dir: String,
    options: ExportOptions,
    state: State<'_, AppState>,
) -> Result<ExportSummary, String> {
    let loaded = current(&state)?;
    if indices.is_empty() {
        return Err("내보낼 스켈레톤을 고르세요.".into());
    }
    if !options.version.trim().is_empty() {
        spine_export::check_version(options.version.trim()).map_err(|e| e.to_string())?;
    }
    let out = PathBuf::from(&out_dir);
    let start = Instant::now();
    let names = loaded.project.skeleton_names();
    let single = spine_export::wants_single_atlas(&options);
    let all_indices = indices.clone();
    let total = indices.len() + usize::from(single);
    let (succeeded, failed) = tauri::async_runtime::spawn_blocking(move || {
        let done = std::sync::atomic::AtomicUsize::new(0);
        let results: Mutex<(usize, usize)> = Mutex::new((0, 0));
        let threads = std::thread::available_parallelism().map(|n| n.get()).unwrap_or(4).min(total).max(1);
        let report = |index: Option<usize>, skeleton: String, result: spine_export::SpineResult<ExportReport>| {
            let n = done.fetch_add(1, std::sync::atomic::Ordering::SeqCst) + 1;
            let progress = match result {
                Ok(ExportReport { files, notes, .. }) => {
                    results.lock().unwrap().0 += 1;
                    ExportProgress { index, skeleton, ok: true, message: None, files: files.iter().map(|f| display_name(f)).collect(), notes, done: n, total }
                }
                Err(e) => {
                    results.lock().unwrap().1 += 1;
                    ExportProgress { index, skeleton, ok: false, message: Some(e.to_string()), files: vec![], notes: vec![], done: n, total }
                }
            };
            let _ = app.emit("export-progress", progress);
        };
        let queue = Mutex::new(indices.into_iter());
        std::thread::scope(|scope| {
            for _ in 0..threads {
                scope.spawn(|| loop {
                    let Some(index) = queue.lock().unwrap().next() else { break };
                    let result = export_skeleton(&loaded.project, index, &loaded.path, &out, &options);
                    report(Some(index), names.get(index).cloned().unwrap_or_default(), result);
                });
            }
            if single {
                // 모든 스켈레톤의 이미지를 atlas 하나로(프로젝트 파일 이름)
                scope.spawn(|| {
                    let result = spine_export::export_single_atlas(&loaded.project, &all_indices, &loaded.path, &out, &options);
                    let label = format!("{}{}", spine_export::single_atlas_name(&loaded.path), options.pack.atlas_extension);
                    report(None, label, result);
                });
            }
        });
        results.into_inner().unwrap()
    })
    .await
    .map_err(|e| e.to_string())?;
    Ok(ExportSummary { succeeded, failed, out_dir, elapsed_ms: start.elapsed().as_millis() })
}

fn display_name(p: &Path) -> String {
    p.file_name().map(|f| f.to_string_lossy().into_owned()).unwrap_or_else(|| p.display().to_string())
}

/// 내보내면 덮어쓸 파일 이름들(출력 폴더에 이미 있는 것).
#[tauri::command]
fn existing_outputs(indices: Vec<usize>, out_dir: String, options: ExportOptions, state: State<'_, AppState>) -> Result<Vec<String>, String> {
    let loaded = current(&state)?;
    let names = loaded.project.skeleton_names();
    let out = PathBuf::from(&out_dir);
    if !out.is_dir() {
        return Ok(Vec::new());
    }
    let mut files: Vec<String> = indices
        .iter()
        .filter_map(|&i| names.get(i))
        .flat_map(|n| spine_export::existing_outputs(n, &out, &options))
        .chain(if spine_export::wants_single_atlas(&options) && !indices.is_empty() {
            spine_export::existing_atlas_outputs(&spine_export::single_atlas_name(&loaded.path), &out, &options)
        } else {
            Vec::new()
        })
        .map(|p| display_name(&p))
        .collect();
    files.sort();
    files.dedup();
    Ok(files)
}

#[tauri::command]
fn load_pack_settings(path: String) -> Result<AtlasSettings, String> {
    let text = std::fs::read_to_string(&path).map_err(|e| format!("파일을 읽을 수 없습니다: {e}"))?;
    serde_json::from_str::<AtlasSettings>(&text).map(AtlasSettings::normalize).map_err(|e| format!("팩 설정(pack.json) 형식이 아닙니다: {e}"))
}

#[tauri::command]
fn save_pack_settings(path: String, settings: AtlasSettings) -> Result<(), String> {
    let text = serde_json::to_string_pretty(&settings.for_save()).map_err(|e| e.to_string())?;
    std::fs::write(&path, text).map_err(|e| format!("저장할 수 없습니다: {e}"))
}

/// Spine 의 내보내기 설정 JSON(Export 대화상자의 Save)을 읽어 지금 설정 위에 덮는다.
#[tauri::command]
fn load_export_settings(path: String, options: ExportOptions) -> Result<spine_export::ImportedSettings, String> {
    let text = std::fs::read_to_string(&path).map_err(|e| format!("파일을 읽을 수 없습니다: {e}"))?;
    spine_export::import_spine_settings(&text, &options).map_err(|e| e.to_string())
}

/// 지금 설정을 Spine 의 내보내기 설정 JSON 으로 저장한다(Spine 의 Load · CLI `--export` 로 읽을 수 있다).
#[tauri::command]
fn save_export_settings(path: String, options: ExportOptions, input: Option<String>, output: Option<String>) -> Result<(), String> {
    let text = spine_export::export_spine_settings(&options, input.as_deref(), output.as_deref()).map_err(|e| e.to_string())?;
    std::fs::write(&path, text).map_err(|e| format!("저장할 수 없습니다: {e}"))
}

#[tauri::command]
fn take_pending_open(state: State<'_, AppState>) -> Vec<String> {
    std::mem::take(&mut *state.pending_open.lock().unwrap())
}

/// 바깥(파인더 «다음으로 열기», 탐색기 연결, 두 번째 실행)에서 들어온 경로를 프런트엔드에 넘긴다.
fn forward_open(app: &AppHandle, paths: Vec<String>) {
    let paths: Vec<String> = paths.into_iter().filter(|p| p.to_lowercase().ends_with(".spine")).collect();
    if paths.is_empty() {
        return;
    }
    if let Some(state) = app.try_state::<AppState>() {
        state.pending_open.lock().unwrap().extend(paths.iter().cloned());
    }
    let _ = app.emit("open-files", paths);
    if let Some(w) = app.get_webview_window("main") {
        let _ = w.unminimize();
        let _ = w.set_focus();
    }
}

fn args_paths(args: &[String]) -> Vec<String> {
    args.iter().skip(1).filter(|a| !a.starts_with('-')).cloned().collect()
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    #[allow(unused_mut)]
    let mut builder = tauri::Builder::default();
    #[cfg(any(windows, target_os = "linux"))]
    {
        builder = builder.plugin(tauri_plugin_single_instance::init(|app, args, _cwd| {
            forward_open(app, args_paths(&args));
        }));
    }
    builder
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        .manage(AppState::default())
        .setup(|app| {
            // 윈도우: 연결된 파일은 명령줄 인수로 들어온다.
            let initial = args_paths(&std::env::args().collect::<Vec<_>>());
            if !initial.is_empty() {
                forward_open(app.handle(), initial);
            }
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![open_project, close_project, export_skeletons, existing_outputs, load_pack_settings, save_pack_settings, load_export_settings, save_export_settings, take_pending_open])
        .build(tauri::generate_context!())
        .expect("Tauri 앱을 만들 수 없습니다")
        .run(|_app, _event| {
            // 맥: 파인더에서 «다음으로 열기»·더블클릭한 파일은 Opened 이벤트로 들어온다.
            #[cfg(target_os = "macos")]
            if let tauri::RunEvent::Opened { urls } = &_event {
                let paths = urls.iter().filter_map(|u| u.to_file_path().ok()).map(|p| p.display().to_string()).collect();
                forward_open(_app, paths);
            }
        });
}
