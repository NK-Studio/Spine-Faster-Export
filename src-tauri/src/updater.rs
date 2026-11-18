//! 앱 업데이트: 새 판을 알아보고(`update.rs`), 내려받아 지금 앱 자리에 바꿔 넣은 뒤 다시 띄운다.
//!
//! - macOS: DMG 를 붙여(hdiutil) 안의 Quick Spine.app 을 지금 앱 옆에 복사하고 서명을 맞춰 본 뒤, 작은 셸 도우미가
//!   이 앱이 끝나기를 기다려 번들을 바꾸고 새 앱을 연다. 앱 폴더에 쓸 수 없으면(다른 사용자 소유·격리 실행) 내려받기 페이지를 연다.
//! - Windows(NSIS 설치판): 작은 PowerShell 도우미가 이 앱이 끝나기를 기다렸다가 새 설치 파일을 조용히(/S) 같은 폴더에 돌리고
//!   Quick Spine 을 다시 띄운다. MSI 로 설치한 판(Program Files, 관리자 권한)은 내려받기 페이지를 연다.
//!
//! 매니페스트 주소는 릴리스 빌드에 QUICK_SPINE_UPDATE_URL 로 묻는다(릴리스 워크플로가 저장소 변수에서 넘긴다). 없으면 업데이트를 확인하지 않는다.

use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};

use serde_json::{Value, json};

use crate::lang::tr;
use crate::update::{self, Manifest};

const CURRENT: &str = env!("CARGO_PKG_VERSION");

/// 마지막으로 읽은 매니페스트(설치할 때 다시 받지 않게).
static LAST: Mutex<Option<Manifest>> = Mutex::new(None);
/// 설치를 한 번에 하나만.
static INSTALLING: AtomicBool = AtomicBool::new(false);

/// 매니페스트 주소. 개발 빌드는 환경 변수 QUICK_SPINE_UPDATE_URL 로 바꿔 시험할 수 있다.
pub fn manifest_url() -> Option<String> {
    #[cfg(debug_assertions)]
    if let Ok(url) = std::env::var("QUICK_SPINE_UPDATE_URL") {
        return Some(url).filter(|u| !u.is_empty());
    }
    option_env!("QUICK_SPINE_UPDATE_URL").map(str::to_string).filter(|u| !u.is_empty())
}

/// 새 판이 있는지 본다. 화면에 넘길 값:
/// `{disabled, current}` · `{available:false, current}` · `{available:true, current, version, date, releases:[{version,date,notes}], page, canInstall}`.
pub fn check() -> Result<Value, String> {
    let Some(url) = manifest_url() else { return Ok(json!({ "disabled": true, "current": CURRENT })) };
    let manifest = update::fetch_manifest(&url)?;
    let newer = manifest.newer_than(CURRENT);
    let value = match newer.first() {
        None => json!({ "available": false, "current": CURRENT }),
        Some(latest) => {
            // 몇 판을 건너뛰었더라도 바뀐 내용은 가장 새 판의 것만 보인다.
            let releases = [json!({ "version": latest.version, "date": latest.date, "notes": latest.notes })];
            let can_install = update::asset_key().is_some_and(|key| latest.assets.contains_key(key)) && install_target().is_ok();
            json!({
                "available": true,
                "current": CURRENT,
                "version": latest.version,
                "date": latest.date,
                "releases": releases,
                "page": manifest.page,
                "canInstall": can_install,
            })
        }
    };
    *LAST.lock().unwrap() = Some(manifest);
    Ok(value)
}

/// `version` 을 `cache` 폴더에 내려받아 설치를 시작한다. Ok 면 도우미(셸·PowerShell)가 이 앱이 끝나기를 기다리고 있다 — 곧바로 앱을 끈다.
/// `progress(받은 바이트, 전체 바이트)`.
pub fn install(version: &str, cache: &Path, progress: impl FnMut(u64, u64)) -> Result<(), String> {
    if INSTALLING.swap(true, Ordering::SeqCst) {
        return Err(tr("이미 업데이트를 받는 중입니다.", "An update is already being downloaded."));
    }
    let result = install_inner(version, cache, progress);
    if result.is_err() {
        INSTALLING.store(false, Ordering::SeqCst);
    }
    result
}

fn install_inner(version: &str, cache: &Path, progress: impl FnMut(u64, u64)) -> Result<(), String> {
    let target = install_target()?;
    let cached = LAST.lock().unwrap().clone().filter(|m| m.releases.iter().any(|r| r.version == version));
    let manifest = match cached {
        Some(m) => m,
        None => update::fetch_manifest(&manifest_url().ok_or_else(|| tr("이 빌드는 업데이트를 지원하지 않습니다.", "This build doesn't support updates."))?)?,
    };
    let release = manifest
        .releases
        .iter()
        .find(|r| r.version == version)
        .ok_or_else(|| tr(&format!("업데이트 정보에 {version} 판이 없습니다."), &format!("The update information has no version {version}.")))?;
    let key = update::asset_key().ok_or_else(unsupported)?;
    let asset = release.assets.get(key).ok_or_else(unsupported)?;

    let dir = cache.join("updates");
    std::fs::create_dir_all(&dir).map_err(|e| tr(&format!("임시 폴더를 만들지 못했습니다: {e}"), &format!("Couldn't create the cache folder: {e}")))?;
    // 지난번에 받다 만 파일·옛 설치 파일을 치운다.
    if let Ok(entries) = std::fs::read_dir(&dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            let _ = if path.is_dir() { std::fs::remove_dir_all(&path) } else { std::fs::remove_file(&path) };
        }
    }
    let name = asset.url.rsplit('/').next().filter(|n| !n.is_empty() && !n.contains(['\\', ':', '?'])).unwrap_or("QuickSpine-update");
    let file = dir.join(name);
    update::download(asset, &file, progress)?;
    launch(&target, &file)
}

fn unsupported() -> String {
    tr(
        "이 운영체제용 업데이트 파일이 없습니다. 내려받기 페이지에서 받아 주세요.",
        "There's no update file for this system. Please download it from the download page.",
    )
}

// ── Windows ─────────────────────────────────────────────

/// NSIS 로 설치한 Quick Spine 의 폴더와 실행 파일.
#[cfg(windows)]
struct Target {
    dir: PathBuf,
    exe: PathBuf,
}

/// 이 실행 파일이 NSIS 설치 파일로 설치한 것인가(옆에 제거 프로그램 uninstall.exe 가 있다). MSI 로 설치한 판은 바꾸지 않는다.
#[cfg(windows)]
fn install_target() -> Result<Target, String> {
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    let dir = exe.parent().map(Path::to_path_buf).ok_or_else(not_setup)?;
    if !dir.join("uninstall.exe").is_file() {
        return Err(not_setup());
    }
    Ok(Target { dir, exe })
}

#[cfg(windows)]
fn not_setup() -> String {
    tr(
        "MSI 로 설치한 판은 앱 안에서 업데이트할 수 없습니다. 내려받기 페이지에서 새 판을 받아 주세요.",
        "The MSI build can't update itself. Please download the new version from the download page.",
    )
}

/// 이 앱이 끝나기를(최대 60초) 기다려 설치 파일을 조용히 돌리고 Quick Spine 을 다시 띄우는 도우미를 띄운다.
#[cfg(windows)]
fn launch(target: &Target, setup: &Path) -> Result<(), String> {
    use std::os::windows::process::CommandExt;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    // PowerShell 작은따옴표 글: ' 는 '' 로.
    let quote = |p: &Path| format!("'{}'", p.display().to_string().replace('\'', "''"));
    // NSIS 의 /D= 는 따옴표 없이 맨 끝에 둔다(경로에 빈칸이 있어도).
    let arguments = format!("/S /D={}", target.dir.display()).replace('\'', "''");
    let script = format!(
        "try {{ Wait-Process -Id {pid} -Timeout 60 -ErrorAction Stop }} catch {{ }}; \
         Start-Process -FilePath {setup} -ArgumentList '{arguments}' -Wait; \
         Start-Process -FilePath {exe}",
        pid = std::process::id(),
        setup = quote(setup),
        exe = quote(&target.exe),
    );
    std::process::Command::new("powershell.exe")
        .args(["-NoProfile", "-NonInteractive", "-ExecutionPolicy", "Bypass", "-WindowStyle", "Hidden", "-Command", &script])
        .creation_flags(CREATE_NO_WINDOW)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .map(|_| ())
        .map_err(|e| tr(&format!("업데이트 도우미를 띄우지 못했습니다: {e}"), &format!("Couldn't start the update helper: {e}")))
}

// ── macOS ─────────────────────────────────────────────

/// 바꿔 넣을 Quick Spine.app 번들.
#[cfg(target_os = "macos")]
struct Target {
    bundle: PathBuf,
}

/// 지금 도는 번들. 그 폴더에 쓸 수 있어야 바꿔 넣는다(격리 실행·다른 사용자 소유 /Applications 면 못 한다).
#[cfg(target_os = "macos")]
fn install_target() -> Result<Target, String> {
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    let bundle = exe.ancestors().find(|p| p.extension().is_some_and(|e| e == "app")).map(Path::to_path_buf).ok_or_else(|| {
        tr("앱 번들 밖에서 실행 중이라 업데이트할 수 없습니다.", "Quick Spine isn't running from an app bundle, so it can't update itself.")
    })?;
    let parent = bundle.parent().ok_or_else(not_writable)?;
    if bundle.to_string_lossy().contains("/AppTranslocation/") {
        return Err(not_writable());
    }
    let probe = parent.join(format!(".quickspine-write-test-{}", std::process::id()));
    std::fs::write(&probe, b"").map_err(|_| not_writable())?;
    let _ = std::fs::remove_file(&probe);
    Ok(Target { bundle })
}

#[cfg(target_os = "macos")]
fn not_writable() -> String {
    tr(
        "Quick Spine 이 있는 폴더에 쓸 수 없어 앱 안에서 업데이트할 수 없습니다. 내려받기 페이지에서 새 판을 받아 주세요.",
        "Quick Spine can't write to the folder it's in, so it can't update itself. Please download the new version from the download page.",
    )
}

#[cfg(target_os = "macos")]
fn run(program: &str, args: &[&std::ffi::OsStr]) -> Result<std::process::Output, String> {
    let out = std::process::Command::new(program).args(args).output().map_err(|e| format!("{program}: {e}"))?;
    if out.status.success() { Ok(out) } else { Err(format!("{program}: {}", String::from_utf8_lossy(&out.stderr).trim())) }
}

/// 서명한 팀(TeamIdentifier). 임시 서명이면 None.
#[cfg(target_os = "macos")]
fn team_id(app: &Path) -> Option<String> {
    let out = std::process::Command::new("/usr/bin/codesign").args(["-dv", "--verbose=2"]).arg(app).output().ok()?;
    let text = String::from_utf8_lossy(&out.stderr);
    text.lines().find_map(|l| l.strip_prefix("TeamIdentifier=")).map(str::trim).filter(|t| !t.is_empty() && *t != "not set").map(str::to_string)
}

/// DMG 안의 앱을 번들 옆(.QuickSpine-update.app)에 꺼내 서명을 맞춰 보고, 이 앱이 끝나면 바꿔 넣고 다시 여는 도우미를 띄운다.
#[cfg(target_os = "macos")]
fn launch(target: &Target, dmg: &Path) -> Result<(), String> {
    use std::ffi::OsStr;
    let failed = |e: String| tr(&format!("업데이트를 준비하지 못했습니다: {e}"), &format!("Couldn't prepare the update: {e}"));
    let mount = dmg.with_extension("mount");
    let _ = std::fs::create_dir_all(&mount);
    run(
        "/usr/bin/hdiutil",
        &[OsStr::new("attach"), OsStr::new("-nobrowse"), OsStr::new("-readonly"), OsStr::new("-noautoopen"), OsStr::new("-mountpoint"), mount.as_os_str(), dmg.as_os_str()],
    )
    .map_err(failed)?;
    let parent = target.bundle.parent().expect("install_target 이 부모를 확인했다");
    let staged = parent.join(".QuickSpine-update.app");
    let copied = (|| -> Result<(), String> {
        let source = std::fs::read_dir(&mount)
            .map_err(|e| e.to_string())?
            .flatten()
            .map(|e| e.path())
            .find(|p| p.extension().is_some_and(|e| e == "app") && !p.is_symlink())
            .ok_or_else(|| tr("DMG 안에 앱이 없습니다.", "There's no app inside the DMG."))?;
        let _ = std::fs::remove_dir_all(&staged);
        run("/usr/bin/ditto", &[source.as_os_str(), staged.as_os_str()])?;
        Ok(())
    })();
    let _ = run("/usr/bin/hdiutil", &[OsStr::new("detach"), OsStr::new("-force"), mount.as_os_str()]);
    let _ = std::fs::remove_dir(&mount);
    copied.map_err(failed)?;

    // 서명이 온전하고, 지금 앱이 Developer ID 로 서명돼 있으면 새 앱도 같은 팀이어야 한다.
    let verified = run("/usr/bin/codesign", &[OsStr::new("--verify"), OsStr::new("--deep"), OsStr::new("--strict"), staged.as_os_str()]).and_then(|_| {
        match (team_id(&target.bundle), team_id(&staged)) {
            (Some(now), new) if new.as_deref() != Some(now.as_str()) => Err(tr("새 앱의 서명자가 다릅니다.", "The new app is signed by someone else.")),
            _ => Ok(()),
        }
    });
    if let Err(e) = verified {
        let _ = std::fs::remove_dir_all(&staged);
        return Err(tr(&format!("새 앱의 서명을 확인하지 못했습니다: {e}"), &format!("Couldn't verify the new app's signature: {e}")));
    }

    // 앱이 끝나기를(최대 60초) 기다려 옛 번들을 치우고 새 번들을 그 자리에 둔 뒤 연다. 바꾸다 실패하면 옛 번들을 되돌린다.
    const SCRIPT: &str = r#"pid="$1"; app="$2"; new="$3"
i=0; while kill -0 "$pid" 2>/dev/null && [ "$i" -lt 600 ]; do sleep 0.1; i=$((i+1)); done
old="$app.old-$$"
if mv "$app" "$old"; then
  if mv "$new" "$app"; then rm -rf "$old"; else mv "$old" "$app"; fi
fi
rm -rf "$new"
open "$app""#;
    std::process::Command::new("/bin/sh")
        .args(["-c", SCRIPT, "quickspine-update", &std::process::id().to_string()])
        .arg(&target.bundle)
        .arg(&staged)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .map(|_| ())
        .map_err(|e| tr(&format!("업데이트 도우미를 띄우지 못했습니다: {e}"), &format!("Couldn't start the update helper: {e}")))
}

// ── 그 밖(리눅스) ─────────────────────────────────────────────

#[cfg(not(any(windows, target_os = "macos")))]
struct Target;

#[cfg(not(any(windows, target_os = "macos")))]
fn install_target() -> Result<Target, String> {
    Err(unsupported())
}

#[cfg(not(any(windows, target_os = "macos")))]
fn launch(_target: &Target, _file: &Path) -> Result<(), String> {
    Err(unsupported())
}
