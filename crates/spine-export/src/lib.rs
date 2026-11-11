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
use spine_atlas::Source;
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

/// JSON 의 Format(Spine 내보내기 대화상자).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum JsonFormat {
    #[default]
    Json,
    JavaScript,
    Minimal,
}

impl JsonFormat {
    fn style(self) -> spine_json::JsonFormat {
        match self {
            JsonFormat::Json => spine_json::JsonFormat::Json,
            JsonFormat::JavaScript => spine_json::JsonFormat::JavaScript,
            JsonFormat::Minimal => spine_json::JsonFormat::Minimal,
        }
    }

    /// Spine 설정 파일에 쓰는 이름.
    fn spine_name(self) -> &'static str {
        match self {
            JsonFormat::Json => "JSON",
            JsonFormat::JavaScript => "JavaScript",
            JsonFormat::Minimal => "Minimal",
        }
    }

    fn parse(s: &str) -> Option<JsonFormat> {
        match s.to_ascii_lowercase().as_str() {
            "json" => Some(JsonFormat::Json),
            "javascript" => Some(JsonFormat::JavaScript),
            "minimal" => Some(JsonFormat::Minimal),
            _ => None,
        }
    }
}

/// 텍스처 아틀라스에 넣을 이미지(Spine `packSource`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PackSource {
    /// 첨부가 쓰는 이미지만.
    #[default]
    Attachments,
    /// 스켈레톤 이미지 폴더의 이미지 모두.
    ImageFolders,
}

/// 아틀라스를 나누는 단위(Spine `packTarget`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PackTarget {
    /// 스켈레톤마다 하나(스켈레톤 이름).
    #[default]
    PerSkeleton,
    /// 모든 스켈레톤을 하나로(프로젝트 파일 이름).
    Single,
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
    /// JSON 파일 확장자.
    pub json_ext: String,
    pub json_format: JsonFormat,
    /// JSON 을 읽기 좋게 줄을 나눠 쓴다.
    pub pretty_print: bool,
    /// Export 체크가 꺼진 첨부도 내보낸다(Export all).
    pub export_all: bool,
    /// 내보낸 뒤 알림(경고)을 보여 준다.
    pub warnings: bool,
    pub pack_source: PackSource,
    pub pack_target: PackTarget,
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
            json_ext: ".json".into(),
            json_format: JsonFormat::Json,
            pretty_print: false,
            export_all: false,
            warnings: true,
            pack_source: PackSource::Attachments,
            pack_target: PackTarget::PerSkeleton,
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

/// 버전 문자열 검사(3.8.xx, 4.0.xx ~ 4.3.xx).
pub fn check_version(s: &str) -> SpineResult<()> {
    if spine_json::supported_version(s) {
        Ok(())
    } else {
        Err(SpineError::Usage(format!("내보낼 버전은 3.8.xx 또는 4.0.xx ~ 4.3.xx 형식이어야 합니다: {s}")))
    }
}

/// 바이너리(.skel)로 쓸 수 있는 버전인지(3.8.xx, 4.0.xx ~ 4.3.xx).
pub fn binary_version(s: &str) -> bool {
    spine_json::supported_version(s)
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

/// Spine 3.8 편집기가 하는 내보내기인가: 3.8 프로젝트를 3.8 로 내보낼 때. atlas 머리말의 글자를 고르는 데 쓴다.
/// (4.x 프로젝트를 3.8 로 내보내는 것은 Spine 4.x 의 «Version» 내보내기라 4.x 의 글자를 따른다.)
fn editor_38(project_format: &str, version: &str) -> bool {
    project_format.starts_with("3.") && version.starts_with("3.")
}

/// 프로젝트와 내보낼 버전에 맞춘 설정을 돌려준다. 내보내기 전에 한 번 거친다.
///
/// Spine 3.8 의 내보내기에는 Pack target 이 없다(atlas 는 늘 스켈레톤마다 하나). 3.8 로 내보낼 때는 프로젝트가
/// 어느 버전이든 «Single atlas» 가 골라져 있어도 스켈레톤별로 만든다(화면에서도 그 선택을 숨긴다).
pub fn options_for(project: &SpineProject, opts: &ExportOptions) -> ExportOptions {
    let mut o = opts.clone();
    let version = if o.version.trim().is_empty() { project.format_version() } else { o.version.trim() };
    if per_skeleton_only(version) {
        o.pack_target = PackTarget::PerSkeleton;
    }
    o
}

/// 이 버전으로 내보낼 때는 atlas 를 스켈레톤별로만 만드는가(3.8).
fn per_skeleton_only(version: &str) -> bool {
    version.starts_with("3.")
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
    let mut model = project.read_model_for(index, opts.export_all, Some(&version))?;
    if opts.clean {
        spine_binary::clean_model(&mut model);
    }
    let name = model.name.clone().unwrap_or_else(|| format!("skeleton{index}"));
    let file_name = sanitize_file_name(&name);
    let mut report = ExportReport { skeleton: name.clone(), ..Default::default() };
    fs::create_dir_all(out_dir)?;

    if opts.format.json() {
        let (text, notes) =
            spine_json::write_json_with_notes(&model, &version, opts.nonessential.unwrap_or(true), opts.json_format.style(), opts.pretty_print)?;
        let path = out_dir.join(format!("{file_name}{}", opts.json_ext));
        fs::write(&path, text)?;
        report.files.push(path);
        report.notes.extend(notes.into_iter().map(|n| format!("{file_name}{}: {n}", opts.json_ext)));
    }

    if opts.format.binary() && !binary_version(&version) {
        // 바이너리를 쓸 수 없는 버전: 바이너리만 고르면 오류, 둘 다면 .skel 만 건너뛴다
        let msg = format!("{version} 은 바이너리(.skel)로 내보낼 수 없습니다. JSON 으로 내보내세요.");
        if opts.format == SkeletonFormat::Binary {
            return Err(SpineError::Usage(msg));
        }
        report.notes.push(format!("{file_name}{} 은 만들지 않았습니다: {msg}", opts.skel_ext));
    } else if opts.format.binary() {
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
                // 3.8: JSON 의 3.8 내보내기와 같은 변환을 거친 모델로 쓴다(X·Y 따로 이동 합치기 등)
                let for_38;
                let source = if version.starts_with("3.") {
                    let (m, notes) = spine_json::model_for_38(&model)?;
                    if !opts.format.json() {
                        // JSON 도 함께 내보내면 같은 알림이 이미 있다
                        report.notes.extend(notes.into_iter().map(|n| format!("{file_name}{}: {n}", opts.skel_ext)));
                    }
                    for_38 = m;
                    &for_38
                } else {
                    &model
                };
                let data = spine_binary::write_binary(source, &version, opts.nonessential.unwrap_or(false))?;
                let path = out_dir.join(format!("{file_name}{}", opts.skel_ext));
                fs::write(&path, data)?;
                report.files.push(path);
            }
        }
    }

    if opts.atlas && opts.pack_target == PackTarget::PerSkeleton {
        let images = images_dir(&model, spine_path, opts.images_dir.as_deref());
        let sources = sources_for(&model, &images, opts)?;
        write_atlas(&sources, &images, &file_name, out_dir, &opts.pack, &version, project.format_version(), &mut report)?;
    }

    if !opts.warnings {
        report.notes.clear();
    }
    Ok(report)
}

/// 팩 소스에 따라 스켈레톤 하나의 이미지 목록을 만든다.
fn sources_for(model: &SkeletonModel, images: &Path, opts: &ExportOptions) -> SpineResult<Vec<Source>> {
    match opts.pack_source {
        PackSource::Attachments => spine_atlas::attachment_sources(model, images),
        PackSource::ImageFolders => spine_atlas::folder_sources(model, images, &opts.pack),
    }
}

/// 이미지 목록을 묶어 `base + 접미사 + 확장자` 와 페이지 PNG 를 쓴다.
/// `project_format` 은 프로젝트를 저장한 Spine 버전: 3.8 프로젝트를 3.8 로 내보내면 머리말을 Spine 3.8 편집기의 글자로 쓴다.
#[allow(clippy::too_many_arguments)]
fn write_atlas(
    sources: &[Source],
    images: &Path,
    base: &str,
    out_dir: &Path,
    pack: &AtlasSettings,
    runtime: &str,
    project_format: &str,
    report: &mut ExportReport,
) -> SpineResult<()> {
    // 3.8 런타임의 atlas 파서는 옛 형식만 차례대로 읽는다(새 형식의 `bounds:` 줄을 모른다).
    // Spine 도 3.8 로 내보내면 Legacy output 을 꺼 두어도 옛 형식으로 쓴다.
    let legacy = AtlasSettings { legacy_output: true, legacy_editor38: editor_38(project_format, runtime), ..pack.clone() };
    let pack = if runtime.starts_with("3.") { &legacy } else { pack };
    // 폴더 안 pack.json 으로 읽은 설정도 3.8 이면 옛 형식으로
    let forced: Vec<Source>;
    let sources = if runtime.starts_with("3.") && sources.iter().any(|s| s.settings.is_some()) {
        forced = sources
            .iter()
            .map(|s| Source {
                settings: s.settings.as_ref().map(|f| std::sync::Arc::new(AtlasSettings { legacy_output: true, legacy_editor38: legacy.legacy_editor38, ..(**f).clone() })),
                ..s.clone()
            })
            .collect();
        &forced[..]
    } else {
        sources
    };
    if pack.bleed && pack.pma {
        report.notes.push("PMA 가 켜져 있어 블리드는 적용하지 않았습니다.".into());
    }
    for (suffix, packed) in spine_atlas::pack_sources_scales(sources, base, pack)? {
        let tag = if suffix.is_empty() { String::new() } else { format!("[{suffix}] ") };
        if !packed.missing.is_empty() {
            report.notes.push(format!("{tag}{}", missing_note(&packed.missing, images)));
        }
        if !packed.ignored.is_empty() {
            report.notes.push(format!("{tag}투명한 이미지 {}개를 넣지 않았습니다(Ignore blank images): {}", packed.ignored.len(), short_list(&packed.ignored)));
        }
        let atlas = packed.output;
        let atlas_path = out_dir.join(format!("{base}{suffix}{}", pack.atlas_extension));
        fs::write(&atlas_path, &atlas.atlas_text)?;
        report.files.push(atlas_path);
        for page in &atlas.pages {
            let path = out_dir.join(&page.name);
            fs::write(&path, spine_atlas::encode_png(page))?;
            report.files.push(path);
        }
    }
    Ok(())
}

/// «Single atlas» 의 이름: 프로젝트 파일 이름에서 확장자를 뺀 것.
pub fn single_atlas_name(spine_path: &Path) -> String {
    sanitize_file_name(&spine_path.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_else(|| "atlas".into()))
}

/// 설정이 «Single atlas» 로 묶기인가(스켈레톤별 내보내기 뒤에 [`export_single_atlas`] 를 한 번 부른다).
pub fn wants_single_atlas(opts: &ExportOptions) -> bool {
    opts.atlas && opts.pack_target == PackTarget::Single
}

/// 고른 스켈레톤의 이미지를 atlas 하나로 묶어 out_dir 에 쓴다(Pack target «Single atlas»).
pub fn export_single_atlas(project: &SpineProject, indices: &[usize], spine_path: &Path, out_dir: &Path, opts: &ExportOptions) -> SpineResult<ExportReport> {
    let name = single_atlas_name(spine_path);
    let mut report = ExportReport { skeleton: format!("{name}{}", opts.pack.atlas_extension), ..Default::default() };
    fs::create_dir_all(out_dir)?;
    let mut lists = Vec::with_capacity(indices.len());
    let mut first_images: Option<PathBuf> = None;
    for &index in indices {
        let model = project.read_model_with(index, opts.export_all)?;
        let images = images_dir(&model, spine_path, opts.images_dir.as_deref());
        match sources_for(&model, &images, opts) {
            Ok(list) => lists.push(list),
            // 이미지가 없는 스켈레톤은 건너뛴다(다른 스켈레톤의 이미지는 묶는다)
            Err(SpineError::Format(m)) => report.notes.push(format!("{}: {m}", model.name.clone().unwrap_or_else(|| format!("skeleton{index}")))),
            Err(e) => return Err(e),
        }
        first_images.get_or_insert(images);
    }
    let (sources, clash) = spine_atlas::merge_sources(lists);
    if sources.is_empty() {
        return Err(SpineError::Format("묶을 이미지가 없습니다".into()));
    }
    if !clash.is_empty() {
        report.notes.push(format!("이름이 같은 다른 이미지 {}개는 먼저 나온 것을 썼습니다: {}", clash.len(), short_list(&clash)));
    }
    let version = resolve_version(project, opts)?;
    write_atlas(&sources, &first_images.unwrap_or_default(), &name, out_dir, &opts.pack, &version, project.format_version(), &mut report)?;
    if !opts.warnings {
        report.notes.clear();
    }
    Ok(report)
}

/// 스켈레톤 하나를 내보내면 out_dir 에 생길 파일 중 이미 있는 것(덮어쓰기 확인용).
/// 페이지 PNG 는 개수를 미리 알 수 없으므로 `이름.png` 와 `이름_<숫자>.png` 를 모두 본다.
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
        check(&mut found, format!("{file_name}{}", opts.json_ext));
    }
    if opts.format.binary() {
        check(&mut found, format!("{file_name}{}", opts.skel_ext));
    }
    if opts.atlas && opts.pack_target == PackTarget::PerSkeleton {
        found.extend(existing_atlas_outputs(&file_name, out_dir, opts));
    }
    found
}

/// `base` 라는 이름으로 atlas 를 쓰면 덮어쓸 파일(atlas 와 페이지 PNG).
pub fn existing_atlas_outputs(base_name: &str, out_dir: &Path, opts: &ExportOptions) -> Vec<PathBuf> {
    let mut found: Vec<PathBuf> = Vec::new();
    let mut check = |name: String| {
        let p = out_dir.join(name);
        if p.is_file() && !found.contains(&p) {
            found.push(p);
        }
    };
    let entries: Vec<String> = fs::read_dir(out_dir)
        .map(|rd| rd.filter_map(|e| e.ok()).filter_map(|e| e.file_name().into_string().ok()).collect())
        .unwrap_or_default();
    for (_, suffix) in opts.pack.scales() {
        let base = format!("{base_name}{suffix}");
        check(format!("{base}{}", opts.pack.atlas_extension));
        for e in &entries {
            if let Some(rest) = e.strip_prefix(&base).and_then(|r| r.strip_suffix(".png")) {
                // 이름.png, 이름_2.png …(separator 가 비었으면 이름2.png)
                let digits = rest.strip_prefix(opts.pack.separator.as_str()).filter(|d| !d.is_empty());
                if rest.is_empty() || digits.is_some_and(|d| d.bytes().all(|b| b.is_ascii_digit())) {
                    check(e.clone());
                }
            }
        }
    }
    found
}

// ---------------------------------------------------------------- Spine 내보내기 설정 파일

/// Spine 의 내보내기 설정 JSON(Export 대화상자의 Save)에서 읽은 것.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportedSettings {
    pub options: ExportOptions,
    /// 설정에 적힌 출력 폴더(없거나 비었으면 None).
    pub output: Option<String>,
    /// JSON 내보내기의 Version(예: "4.1"). 없으면 None.
    pub version: Option<String>,
    /// 옮기지 못한 항목 등 알릴 것.
    pub notes: Vec<String>,
}

/// Spine 내보내기 설정 JSON(`"class": "export-json" | "export-binary"`)을 읽어 base 위에 덮는다.
/// 설정에 없는 항목(이미지 폴더 등)은 base 의 값을 그대로 둔다.
pub fn import_spine_settings(text: &str, base: &ExportOptions) -> SpineResult<ImportedSettings> {
    use serde_json::Value;
    let bad = |m: String| SpineError::Format(format!("Spine 내보내기 설정을 읽을 수 없습니다: {m}"));
    let v: Value = serde_json::from_str(text.trim_start_matches('\u{feff}')).map_err(|e| bad(e.to_string()))?;
    let obj = v.as_object().ok_or_else(|| bad("JSON 객체가 아닙니다".into()))?;
    let class = obj.get("class").and_then(Value::as_str).unwrap_or("");
    let mut o = base.clone();
    let mut notes = Vec::new();
    let string = |k: &str| obj.get(k).and_then(Value::as_str).filter(|s| !s.is_empty()).map(str::to_owned);
    let flag = |k: &str| obj.get(k).and_then(Value::as_bool);
    match class.rsplit(['.', '$']).next().unwrap_or("").to_ascii_lowercase().as_str() {
        "export-json" | "exportjson" => {
            o.format = SkeletonFormat::Json;
            if let Some(e) = string("extension") {
                o.json_ext = e;
            }
            if let Some(f) = string("format") {
                match JsonFormat::parse(&f) {
                    Some(f) => o.json_format = f,
                    None => notes.push(format!("모르는 JSON 형식 «{f}» 은 건너뛰었습니다.")),
                }
            }
            if let Some(b) = flag("prettyPrint") {
                o.pretty_print = b;
            }
            if let Some(b) = ["all", "exportAll", "forceAll"].iter().find_map(|k| flag(k)) {
                o.export_all = b;
            }
        }
        "export-binary" | "exportbinary" => {
            o.format = SkeletonFormat::Binary;
            if let Some(e) = string("extension") {
                o.skel_ext = e;
            }
        }
        "" => return Err(bad("\"class\" 가 없습니다(Spine 의 내보내기 대화상자에서 저장한 파일인지 확인하세요)".into())),
        other => return Err(bad(format!("데이터 내보내기 설정이 아닙니다(class: {other}). JSON · 바이너리 내보내기 설정만 읽습니다"))),
    }
    if let Some(b) = flag("nonessential") {
        o.nonessential = Some(b);
    }
    if let Some(b) = flag("cleanUp") {
        o.clean = b;
    }
    if let Some(b) = flag("warnings") {
        o.warnings = b;
    }
    match obj.get("packAtlas") {
        Some(Value::Object(_)) => {
            let pack: AtlasSettings = serde_json::from_value(obj["packAtlas"].clone()).map_err(|e| bad(format!("packAtlas: {e}")))?;
            o.pack = pack.normalize();
            o.atlas = true;
            let p = &obj["packAtlas"];
            if !o.pack.output_format.eq_ignore_ascii_case("png") {
                notes.push(format!("출력 형식 {} 은 지원하지 않아 PNG 로 바꿨습니다.", o.pack.output_format));
                o.pack.output_format = "png".into();
            }
            if p.get("autoScale").and_then(Value::as_bool) == Some(true) {
                notes.push("Auto scale 은 지원하지 않습니다.".into());
            }
        }
        Some(Value::Null) | None => o.atlas = false,
        Some(_) => return Err(bad("packAtlas 가 객체가 아닙니다".into())),
    }
    match string("packSource").map(|s| s.to_ascii_lowercase()).as_deref() {
        Some("attachments") => o.pack_source = PackSource::Attachments,
        Some("imagefolders") => o.pack_source = PackSource::ImageFolders,
        Some(other) => notes.push(format!("모르는 packSource «{other}» 는 건너뛰었습니다.")),
        None => {}
    }
    match string("packTarget").map(|s| s.to_ascii_lowercase()).as_deref() {
        Some("perskeleton") => o.pack_target = PackTarget::PerSkeleton,
        Some("single") => o.pack_target = PackTarget::Single,
        Some(other) => notes.push(format!("packTarget «{other}» 는 지원하지 않아 건너뛰었습니다.")),
        None => {}
    }
    Ok(ImportedSettings { options: o, output: string("output"), version: string("version"), notes })
}

/// 프로젝트에 저장된 마지막 데이터 내보내기 설정(Spine Export 대화상자에서 마지막으로 쓴 JSON · 바이너리 설정)을 base 위에 덮는다.
/// 저장된 설정이 없으면 None. 출력 폴더는 Spine 이 적은 그대로다(상대 경로면 프로젝트 폴더 기준).
pub fn project_export_settings(project: &SpineProject, base: &ExportOptions) -> Option<SpineResult<ImportedSettings>> {
    let settings = project.last_export_settings()?;
    Some(import_spine_settings(&settings.to_string(), base))
}

/// 지금 설정을 Spine 내보내기 설정 JSON 으로. 형식이 JSON 이면 `export-json`, 아니면 `export-binary`.
/// Spine 이 저장하는 것과 같은 키만 쓴다(이 앱에만 있는 설정은 빠진다).
pub fn export_spine_settings(opts: &ExportOptions, input: Option<&str>, output: Option<&str>) -> SpineResult<String> {
    use serde_json::{json, Map, Value};
    let json_class = opts.format == SkeletonFormat::Json;
    let mut m = Map::new();
    m.insert("class".into(), json!(if json_class { "export-json" } else { "export-binary" }));
    if json_class {
        m.insert("extension".into(), json!(opts.json_ext));
        m.insert("format".into(), json!(opts.json_format.spine_name()));
        m.insert("prettyPrint".into(), json!(opts.pretty_print));
    } else {
        m.insert("extension".into(), json!(opts.skel_ext));
    }
    m.insert("nonessential".into(), json!(opts.nonessential.unwrap_or(json_class)));
    m.insert("cleanUp".into(), json!(opts.clean));
    let pack = if opts.atlas {
        let mut p = serde_json::to_value(opts.pack.clone().for_save()).map_err(|e| SpineError::Format(e.to_string()))?;
        if let Some(p) = p.as_object_mut() {
            p.remove("grid"); // Spine 4.x 는 "packing" 으로 적는다
        }
        p
    } else {
        Value::Null
    };
    m.insert("packAtlas".into(), pack);
    m.insert("packSource".into(), json!(if opts.pack_source == PackSource::ImageFolders { "imagefolders" } else { "attachments" }));
    m.insert("packTarget".into(), json!(if opts.pack_target == PackTarget::Single { "single" } else { "perskeleton" }));
    m.insert("warnings".into(), json!(opts.warnings));
    m.insert("output".into(), json!(output.unwrap_or("")));
    m.insert("input".into(), json!(input.unwrap_or("")));
    m.insert("open".into(), json!(false));
    serde_json::to_string_pretty(&Value::Object(m)).map_err(|e| SpineError::Format(e.to_string()))
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

    /// atlas 머리말은 3.8 프로젝트를 3.8 로 내보낼 때만 Spine 3.8 편집기의 글자를 따른다.
    #[test]
    fn editor_38_is_38_project_to_38() {
        assert!(editor_38("3.8.75", "3.8.99"));
        assert!(!editor_38("4.2.43", "3.8.99")); // Spine 4.x 의 «Version» 내보내기
        assert!(!editor_38("3.8.75", "4.2.43")); // Spine 4.x 로 열어 내보내기
        assert!(!editor_38("4.2.43", "4.2.43"));
    }

    /// 3.8 로 내보낼 때는 프로젝트 버전과 상관없이 스켈레톤별 atlas 만 만든다.
    #[test]
    fn target_38_packs_per_skeleton_only() {
        assert!(per_skeleton_only("3.8.99"));
        assert!(per_skeleton_only("3.8.55"));
        assert!(!per_skeleton_only("4.0.64"));
        assert!(!per_skeleton_only("4.2.43"));
    }

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
        for f in ["hero.skel.bytes", "hero.atlas.txt", "hero.png", "hero_2.png", "hero2.png", "hero_other.png", "heroes.json", "x.json"] {
            fs::write(dir.join(f), b"x").unwrap();
        }
        let opts = ExportOptions::preset_anc();
        let mut got: Vec<String> = existing_outputs("hero", &dir, &opts).iter().map(|p| p.file_name().unwrap().to_string_lossy().into_owned()).collect();
        got.sort();
        assert_eq!(got, vec!["hero.atlas.txt", "hero.png", "hero.skel.bytes", "hero_2.png"]);
        // 아틀라스를 끄면 바이너리만
        let no_atlas = ExportOptions { atlas: false, ..ExportOptions::preset_anc() };
        assert_eq!(existing_outputs("hero", &dir, &no_atlas).len(), 1);
        let _ = fs::remove_dir_all(&dir);
    }

    /// Spine 이 저장한 내보내기 설정을 읽고, 다시 써서 읽어도 같다.
    #[test]
    fn spine_settings_round_trip() {
        let text = r#"{"class":"export-binary","extension":".skel.bytes","nonessential":true,"cleanUp":true,
            "packAtlas":{"alias":true,"maxWidth":2048,"maxHeight":2048,"multipleOfFour":true,"premultiplyAlpha":false,"bleed":true,
              "scaleResampling":["bicubic"],"atlasExtension":".atlas.txt","packing":"grid","webp":null,"id":-1,"separator":"_"},
            "packSource":"attachments","packTarget":"single","warnings":true,"output":"/out","id":-1,"input":"/in/cat.spine","open":false}"#;
        let got = import_spine_settings(text, &ExportOptions::default()).unwrap();
        let o = &got.options;
        assert_eq!(o.format, SkeletonFormat::Binary);
        assert_eq!((o.skel_ext.as_str(), o.nonessential, o.clean, o.atlas), (".skel.bytes", Some(true), true, true));
        assert!(o.pack.alias && o.pack.bleed && !o.pack.pma && o.pack.multiple_of_four && o.pack.grid);
        assert_eq!((o.pack_source, o.pack_target), (PackSource::Attachments, PackTarget::Single));
        assert_eq!(got.output.as_deref(), Some("/out"));
        let saved = export_spine_settings(o, Some("/in/cat.spine"), Some("/out")).unwrap();
        assert!(saved.contains("\"packing\": \"grid\"") && !saved.contains("\"grid\":"));
        let again = import_spine_settings(&saved, &ExportOptions::default()).unwrap();
        assert_eq!(serde_json::to_string(&again.options).unwrap(), serde_json::to_string(o).unwrap());

        let json = r#"{"class":"export-json","extension":".txt","format":"Minimal","prettyPrint":true,"nonessential":false,"cleanUp":false,"packAtlas":null,"version":"4.1"}"#;
        let got = import_spine_settings(json, &ExportOptions::preset_anc()).unwrap();
        let o = &got.options;
        assert_eq!((o.format, o.json_format, o.pretty_print, o.atlas), (SkeletonFormat::Json, JsonFormat::Minimal, true, false));
        assert_eq!((o.json_ext.as_str(), got.version.as_deref()), (".txt", Some("4.1")));
        assert!(import_spine_settings(r#"{"class":"export-png"}"#, &ExportOptions::default()).is_err());
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
