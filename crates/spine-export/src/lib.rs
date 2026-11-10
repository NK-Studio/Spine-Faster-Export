//! `.spine` → JSON · `.skel` · atlas 내보내기 파사드.
//!
//! 파이썬 PoC `export_poc/spine_export.py` 의 `export_one` 과 같은 순서·규칙을 따른다.
//! - JSON 의 비필수 데이터 기본값은 켬, 바이너리는 끔(Spine CLI 기본과 같다).
//! - 애니메이션 정리는 JSON·바이너리를 쓰기 전에 모델에 적용한다.
//! - 바이너리로 낼 수 없는 기능이 있으면 `Both` 는 건너뛰고 알리며, `Binary` 는 오류로 멈춘다.

use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
pub use spine_atlas::{AtlasSettings, PixelFormat, TextureFilter, TextureWrap};
pub use spine_model::{AtlasOutput, AtlasPage, SkeletonModel, SpineError, SpineResult};
pub use spine_project::SpineProject;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SkeletonFormat {
    Json,
    Binary,
    Both,
}

impl SkeletonFormat {
    pub fn json(self) -> bool {
        matches!(self, SkeletonFormat::Json | SkeletonFormat::Both)
    }

    pub fn binary(self) -> bool {
        matches!(self, SkeletonFormat::Binary | SkeletonFormat::Both)
    }
}

/// 내보내기 설정. 프런트엔드와 직렬화로 주고받는다(camelCase).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct ExportOptions {
    /// 내보낼 런타임 버전(4.0.xx ~ 4.3.xx). 비우면 프로젝트 포맷 버전.
    pub version: String,
    pub format: SkeletonFormat,
    /// None: JSON 은 켬, 바이너리는 끔.
    pub nonessential: Option<bool>,
    pub clean: bool,
    /// 텍스처 아틀라스를 만든다(Texture atlas: Attachments).
    pub atlas: bool,
    /// Texture Packer Settings(Spine `pack.json` 과 같은 키).
    pub pack: AtlasSettings,
    pub skel_ext: String,
    /// 비우면 프로젝트에 적힌 이미지 경로(.spine 기준).
    pub images_dir: Option<PathBuf>,
}

impl Default for ExportOptions {
    fn default() -> Self {
        ExportOptions {
            version: String::new(),
            format: SkeletonFormat::Both,
            nonessential: None,
            clean: false,
            atlas: false,
            pack: AtlasSettings::default(),
            skel_ext: ".skel".into(),
            images_dir: None,
        }
    }
}

impl ExportOptions {
    /// 팀 export 설정(파이썬 `--preset anc`).
    pub fn preset_anc() -> Self {
        ExportOptions {
            format: SkeletonFormat::Binary,
            atlas: true,
            nonessential: Some(true),
            clean: true,
            pack: AtlasSettings {
                pma: false,
                bleed: true,
                alias: true,
                atlas_extension: ".atlas.txt".into(),
                ..AtlasSettings::default()
            }
            .with_max_size(2048),
            skel_ext: ".skel.bytes".into(),
            ..ExportOptions::default()
        }
    }
}

/// 버전 문자열 검사(4.0.xx ~ 4.3.xx).
pub fn check_version(s: &str) -> SpineResult<()> {
    let parts: Vec<&str> = s.split('.').collect();
    let ok = parts.len() == 3
        && parts.iter().all(|p| !p.is_empty() && p.len() < 8 && p.bytes().all(|b| b.is_ascii_digit()))
        && parts[0] == "4"
        && parts[1].parse::<u32>().map(|m| m <= 3).unwrap_or(false);
    if ok {
        Ok(())
    } else {
        Err(SpineError::Usage(format!("내보낼 버전은 4.0.xx ~ 4.3.xx 형식이어야 합니다: {s}")))
    }
}

/// 스켈레톤 하나를 내보낸 결과.
#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ExportReport {
    pub skeleton: String,
    pub files: Vec<PathBuf>,
    /// 실패하지는 않았지만 알릴 것(바이너리 건너뜀, 블리드 미적용 등).
    pub notes: Vec<String>,
}

fn sanitize_file_name(name: &str) -> String {
    // 스켈레톤 이름에 폴더 구분자가 있으면 파일 이름으로 쓸 수 없다.
    name.chars()
        .map(|c| match c {
            '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|' => '_',
            c => c,
        })
        .collect()
}

/// 이미지 폴더: 지정한 것 > 프로젝트에 적힌 경로(.spine 기준).
pub fn images_dir(model: &SkeletonModel, spine_path: &Path, override_dir: Option<&Path>) -> PathBuf {
    if let Some(d) = override_dir {
        return d.to_path_buf();
    }
    let base = spine_path
        .canonicalize()
        .ok()
        .and_then(|p| p.parent().map(Path::to_path_buf))
        .or_else(|| spine_path.parent().map(Path::to_path_buf))
        .unwrap_or_default();
    match model.images.as_deref() {
        Some(rel) if !rel.is_empty() => base.join(rel),
        _ => base,
    }
}

/// 버전을 정한다(비었으면 프로젝트 포맷).
pub fn resolve_version(project: &SpineProject, opts: &ExportOptions) -> SpineResult<String> {
    let v = if opts.version.trim().is_empty() { project.format_version().to_string() } else { opts.version.trim().to_string() };
    check_version(&v)?;
    Ok(v)
}

/// 스켈레톤 하나를 out_dir 에 내보낸다.
pub fn export_skeleton(
    project: &SpineProject,
    index: usize,
    spine_path: &Path,
    out_dir: &Path,
    opts: &ExportOptions,
) -> SpineResult<ExportReport> {
    let version = resolve_version(project, opts)?;
    let mut model = project.read_model(index)?;
    if opts.clean {
        spine_binary::clean_model(&mut model);
    }
    let name = model.name.clone().unwrap_or_else(|| format!("skeleton{index}"));
    let file_name = sanitize_file_name(&name);
    let mut report = ExportReport { skeleton: name.clone(), ..Default::default() };
    fs::create_dir_all(out_dir)?;

    if opts.format.json() {
        let text = spine_json::write_json(&model, &version, opts.nonessential.unwrap_or(true))?;
        let path = out_dir.join(format!("{file_name}.json"));
        fs::write(&path, text)?;
        report.files.push(path);
    }

    if opts.format.binary() {
        match spine_binary::unsupported_binary(&model, &version) {
            Some(reason) if opts.format == SkeletonFormat::Binary => {
                return Err(SpineError::Unsupported(format!(
                    "{name}: .skel 을 만들 수 없습니다(이 버전에 없는 기능: {reason}). JSON 으로 내보내세요."
                )));
            }
            Some(reason) => {
                report.notes.push(format!("{file_name}{} 은 만들지 않았습니다(이 버전에 없는 기능: {reason}).", opts.skel_ext));
            }
            None => {
                let data = spine_binary::write_binary(&model, &version, opts.nonessential.unwrap_or(false))?;
                let path = out_dir.join(format!("{file_name}{}", opts.skel_ext));
                fs::write(&path, data)?;
                report.files.push(path);
            }
        }
    }

    if opts.atlas {
        let pack = &opts.pack;
        if pack.bleed && pack.pma {
            report.notes.push("PMA 가 켜져 있어 블리드는 적용하지 않았습니다.".into());
        }
        let images = images_dir(&model, spine_path, opts.images_dir.as_deref());
        for (suffix, packed) in spine_atlas::pack_scales(&model, &images, &file_name, pack)? {
            let tag = if suffix.is_empty() { String::new() } else { format!("[{suffix}] ") };
            if !packed.missing.is_empty() {
                report.notes.push(format!("{tag}{}", missing_note(&packed.missing, &images)));
            }
            if !packed.ignored.is_empty() {
                report.notes.push(format!("{tag}투명한 이미지 {}개를 넣지 않았습니다(Ignore blank images): {}", packed.ignored.len(), short_list(&packed.ignored)));
            }
            let atlas = packed.output;
            let atlas_path = out_dir.join(format!("{file_name}{suffix}{}", pack.atlas_extension));
            fs::write(&atlas_path, &atlas.atlas_text)?;
            report.files.push(atlas_path);
            for page in &atlas.pages {
                let path = out_dir.join(&page.name);
                fs::write(&path, spine_atlas::encode_png(page))?;
                report.files.push(path);
            }
        }
    }

    Ok(report)
}

/// 스켈레톤 하나를 내보내면 out_dir 에 생길 파일 중 이미 있는 것(덮어쓰기 확인용).
/// 페이지 PNG 는 개수를 미리 알 수 없으므로 `이름.png` 와 `이름<숫자>.png` 를 모두 본다.
pub fn existing_outputs(skeleton_name: &str, out_dir: &Path, opts: &ExportOptions) -> Vec<PathBuf> {
    let file_name = sanitize_file_name(skeleton_name);
    let mut found: Vec<PathBuf> = Vec::new();
    let check = |found: &mut Vec<PathBuf>, name: String| {
        let p = out_dir.join(name);
        if p.is_file() && !found.contains(&p) {
            found.push(p);
        }
    };
    if opts.format.json() {
        check(&mut found, format!("{file_name}.json"));
    }
    if opts.format.binary() {
        check(&mut found, format!("{file_name}{}", opts.skel_ext));
    }
    if opts.atlas {
        let entries: Vec<String> = fs::read_dir(out_dir)
            .map(|rd| rd.filter_map(|e| e.ok()).filter_map(|e| e.file_name().into_string().ok()).collect())
            .unwrap_or_default();
        for (_, suffix) in opts.pack.scales() {
            let base = format!("{file_name}{suffix}");
            check(&mut found, format!("{base}{}", opts.pack.atlas_extension));
            for e in &entries {
                if let Some(rest) = e.strip_prefix(&base).and_then(|r| r.strip_suffix(".png")) {
                    if rest.bytes().all(|b| b.is_ascii_digit()) {
                        check(&mut found, e.clone());
                    }
                }
            }
        }
    }
    found
}

fn short_list(names: &[String]) -> String {
    let shown: Vec<&str> = names.iter().take(5).map(String::as_str).collect();
    let more = if names.len() > 5 { format!(" 외 {}개", names.len() - 5) } else { String::new() };
    format!("{}{more}", shown.join(", "))
}

fn missing_note(missing: &[String], images: &Path) -> String {
    format!("이미지 {}개를 찾지 못해 비워 두었습니다({}): {}", missing.len(), images.display(), short_list(missing))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 프런트엔드(src/main.ts)의 DEFAULT_PACK 이 Rust 기본값과 같은지 본다.
    #[test]
    fn frontend_default_pack_matches() {
        let src = include_str!("../../../src/main.ts");
        let start = src.find("const DEFAULT_PACK: PackSettings = {").expect("DEFAULT_PACK 을 찾을 수 없습니다");
        let body = &src[start..];
        let body = &body[body.find('{').unwrap()..body.find("\n};").unwrap() + 2];
        // TS 객체 리터럴 → JSON: 키에 따옴표, 끝 쉼표 제거, 작은따옴표 없음
        let mut json = String::new();
        for line in body.lines() {
            let t = line.trim();
            if let Some((k, v)) = t.split_once(':') {
                if k.chars().all(|c| c.is_ascii_alphanumeric()) {
                    json.push_str(&format!("\"{k}\":{}\n", v.trim()));
                    continue;
                }
            }
            json.push_str(t);
            json.push('\n');
        }
        let json = json.replace(",\n}", "\n}");
        let ts: AtlasSettings = serde_json::from_str(&json).unwrap_or_else(|e| panic!("{e}\n{json}"));
        assert_eq!(ts, AtlasSettings::default());
    }

    #[test]
    fn existing_outputs_finds_overwrites() {
        let dir = std::env::temp_dir().join(format!("sfe-existing-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        for f in ["hero.skel.bytes", "hero.atlas.txt", "hero.png", "hero2.png", "hero_other.png", "heroes.json", "x.json"] {
            fs::write(dir.join(f), b"x").unwrap();
        }
        let opts = ExportOptions::preset_anc();
        let mut got: Vec<String> = existing_outputs("hero", &dir, &opts).iter().map(|p| p.file_name().unwrap().to_string_lossy().into_owned()).collect();
        got.sort();
        assert_eq!(got, vec!["hero.atlas.txt", "hero.png", "hero.skel.bytes", "hero2.png"]);
        // 아틀라스를 끄면 바이너리만
        let no_atlas = ExportOptions { atlas: false, ..ExportOptions::preset_anc() };
        assert_eq!(existing_outputs("hero", &dir, &no_atlas).len(), 1);
        let _ = fs::remove_dir_all(&dir);
    }

    /// 프런트엔드가 보내는 모양(camelCase, pack 안에 pack.json 키) 그대로 읽힌다.
    #[test]
    fn options_from_frontend_json() {
        let o: ExportOptions = serde_json::from_str(
            r#"{"version":"4.2.43","format":"binary","nonessential":null,"clean":true,"atlas":true,"skelExt":".skel.bytes","imagesDir":null,
                "pack":{"premultiplyAlpha":false,"bleed":true,"maxWidth":2048,"maxHeight":1024,"pot":true,"scale":[1,0.5],"scaleSuffix":["","@0.5x"],"atlasExtension":".atlas.txt"}}"#,
        )
        .unwrap();
        assert!(!o.pack.pma && o.pack.bleed && o.pack.pot && o.pack.max_height == 1024);
        assert_eq!(o.pack.scales(), vec![(1.0, String::new()), (0.5, "@0.5x".to_string())]);
        assert_eq!(o.format, SkeletonFormat::Binary);
    }
}
