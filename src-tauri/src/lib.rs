//! Quick Spine — Tauri 백엔드.
//!
//! 프런트엔드가 부르는 명령:
//! - `open_project(path)`: `.spine` 을 읽어 스켈레톤 목록을 돌려준다.
//! - `export_skeletons(indices, outDir, options)`: 고른 스켈레톤을 병렬로 내보낸다(`export-progress` 이벤트).
//! - `load_pack_settings(path)` / `save_pack_settings(path, settings)`: Spine `pack.json`(Texture Packer Settings).
//! - `load_export_settings(path, options)` / `save_export_settings(path, options, input, output)`: Spine 내보내기 설정 JSON.
//! - `project_export_settings(options)`: 열린 `.spine` 에 저장된 마지막 내보내기 설정(없으면 null).
//! - `take_pending_open()`: 앱을 켤 때 파인더/탐색기에서 넘겨받은 파일.
//! - `set_menu_language(lang)`: 메뉴 막대를 그 언어(`ko`/`en`)로 다시 만든다.
//!
//! 메뉴의 설정(맥: 앱 메뉴 > 설정…, 윈도우: 편집 > 환경설정…)을 누르면 `open-settings` 이벤트를 보낸다.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Instant;

use serde::Serialize;
use spine_export::{AtlasSettings, ExportOptions, ExportReport, SpineProject};
use tauri::menu::{Menu, MenuBuilder, MenuItemBuilder, SubmenuBuilder};
use tauri::{AppHandle, Emitter, Manager, Runtime, State};

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
    let (succeeded, failed) = tauri::async_runtime::spawn_blocking(move || {
        spine_export::export_many(&loaded.project, &loaded.path, &indices, &out, &options, |item| {
            let progress = match item.result {
                Ok(ExportReport { files, notes, .. }) => ExportProgress {
                    index: item.index,
                    skeleton: item.label,
                    ok: true,
                    message: None,
                    files: files.iter().map(|f| display_name(f)).collect(),
                    notes,
                    done: item.done,
                    total: item.total,
                },
                Err(e) => ExportProgress { index: item.index, skeleton: item.label, ok: false, message: Some(e.to_string()), files: vec![], notes: vec![], done: item.done, total: item.total },
            };
            let _ = app.emit("export-progress", progress);
        })
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
    let files = spine_export::existing_outputs_for(&loaded.project, &loaded.path, &indices, &PathBuf::from(&out_dir), &options);
    let mut names: Vec<String> = files.iter().map(|p| display_name(p)).collect();
    names.dedup();
    Ok(names)
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

/// 열린 프로젝트에 저장된 마지막 내보내기 설정(Spine 에서 마지막으로 쓴 JSON · 바이너리 설정)을 지금 설정 위에 덮는다.
/// 저장된 설정이 없으면 None. 출력 폴더는 프로젝트 폴더 기준으로 풀고, 다른 OS 의 경로(예: 맥에서 `E:/…`)면 버린다.
#[tauri::command]
fn project_export_settings(options: ExportOptions, state: State<'_, AppState>) -> Result<Option<spine_export::ImportedSettings>, String> {
    let loaded = current(&state)?;
    let Some(result) = spine_export::project_export_settings(&loaded.project, &options) else { return Ok(None) };
    let mut got = result.map_err(|e| e.to_string())?;
    if let Some(out) = got.output.take() {
        match resolve_output(&loaded.path, &out) {
            Some(p) => got.output = Some(p),
            None => got.notes.push(format!("설정에 적힌 출력 폴더 «{out}» 는 이 컴퓨터의 경로가 아니라 쓰지 않았습니다.")),
        }
    }
    Ok(Some(got))
}

fn resolve_output(project: &Path, out: &str) -> Option<String> {
    spine_export::resolve_output(project, out).map(|p| p.display().to_string())
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

// ───────────────────────────────────────── 메뉴

/// 메뉴 문구(언어별). 앱 화면의 문구는 프런트엔드(`src/i18n.ts`)가 맡는다.
/// 맥 전용 항목(앱·윈도우 메뉴)은 다른 OS 에서 쓰이지 않는다.
#[allow(dead_code)]
struct MenuText {
    edit: &'static str,
    settings: &'static str,
    undo: &'static str,
    redo: &'static str,
    cut: &'static str,
    copy: &'static str,
    paste: &'static str,
    select_all: &'static str,
    window: &'static str,
    minimize: &'static str,
    zoom: &'static str,
    close: &'static str,
    about: &'static str,
    services: &'static str,
    hide: &'static str,
    hide_others: &'static str,
    show_all: &'static str,
    quit: &'static str,
}

const MENU_KO: MenuText = MenuText {
    edit: "편집",
    settings: if cfg!(target_os = "macos") { "설정…" } else { "환경설정…" },
    undo: "실행 취소",
    redo: "다시 실행",
    cut: "오려두기",
    copy: "복사하기",
    paste: "붙여넣기",
    select_all: "모두 선택",
    window: "윈도우",
    minimize: "최소화",
    zoom: "확대/축소",
    close: "윈도우 닫기",
    about: "Quick Spine 정보",
    services: "서비스",
    hide: "Quick Spine 가리기",
    hide_others: "기타 가리기",
    show_all: "모두 보기",
    quit: "Quick Spine 종료",
};

const MENU_EN: MenuText = MenuText {
    edit: "Edit",
    settings: if cfg!(target_os = "macos") { "Settings…" } else { "Preferences…" },
    undo: "Undo",
    redo: "Redo",
    cut: "Cut",
    copy: "Copy",
    paste: "Paste",
    select_all: "Select All",
    window: "Window",
    minimize: "Minimize",
    zoom: "Zoom",
    close: "Close Window",
    about: "About Quick Spine",
    services: "Services",
    hide: "Hide Quick Spine",
    hide_others: "Hide Others",
    show_all: "Show All",
    quit: "Quit Quick Spine",
};

/// 메뉴 막대. 맥은 앱 메뉴에 «설정…»(⌘,), 윈도우·리눅스는 «편집» 메뉴에 «환경설정…»(Ctrl+,).
fn build_menu<R: Runtime, M: Manager<R>>(app: &M, lang: &str) -> tauri::Result<Menu<R>> {
    let t = if lang == "en" { &MENU_EN } else { &MENU_KO };
    let settings = MenuItemBuilder::with_id("settings", t.settings).accelerator("CmdOrCtrl+,").build(app)?;
    #[cfg(target_os = "macos")]
    {
        let app_menu = SubmenuBuilder::new(app, "Quick Spine")
            .about_with_text(t.about, None)
            .separator()
            .item(&settings)
            .separator()
            .services_with_text(t.services)
            .separator()
            .hide_with_text(t.hide)
            .hide_others_with_text(t.hide_others)
            .show_all_with_text(t.show_all)
            .separator()
            .quit_with_text(t.quit)
            .build()?;
        // 기본 메뉴를 대체하므로, 입력칸의 ⌘C·⌘V 등이 계속 되도록 편집 메뉴를 둔다
        let edit = SubmenuBuilder::new(app, t.edit)
            .undo_with_text(t.undo)
            .redo_with_text(t.redo)
            .separator()
            .cut_with_text(t.cut)
            .copy_with_text(t.copy)
            .paste_with_text(t.paste)
            .select_all_with_text(t.select_all)
            .build()?;
        let window = SubmenuBuilder::new(app, t.window)
            .minimize_with_text(t.minimize)
            .maximize_with_text(t.zoom)
            .separator()
            .close_window_with_text(t.close)
            .build()?;
        MenuBuilder::new(app).items(&[&app_menu, &edit, &window]).build()
    }
    #[cfg(not(target_os = "macos"))]
    {
        let edit = SubmenuBuilder::new(app, t.edit)
            .cut_with_text(t.cut)
            .copy_with_text(t.copy)
            .paste_with_text(t.paste)
            .select_all_with_text(t.select_all)
            .separator()
            .item(&settings)
            .build()?;
        MenuBuilder::new(app).items(&[&edit]).build()
    }
}

#[tauri::command]
fn set_menu_language(app: AppHandle, lang: String) -> Result<(), String> {
    let menu = build_menu(&app, &lang).map_err(|e| e.to_string())?;
    app.set_menu(menu).map_err(|e| e.to_string())?;
    Ok(())
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
        .menu(|app| build_menu(app, "ko"))
        .on_menu_event(|app, event| {
            if event.id() == "settings" {
                let _ = app.emit("open-settings", ());
            }
        })
        .setup(|app| {
            // 윈도우: 연결된 파일은 명령줄 인수로 들어온다.
            let initial = args_paths(&std::env::args().collect::<Vec<_>>());
            if !initial.is_empty() {
                forward_open(app.handle(), initial);
            }
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![open_project, close_project, export_skeletons, existing_outputs, load_pack_settings, save_pack_settings, load_export_settings, save_export_settings, project_export_settings, take_pending_open, set_menu_language])
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
