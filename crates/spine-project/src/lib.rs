//! `.spine` 프로젝트 파일 읽기와 스켈레톤 모델 만들기.
//! C# `Editor/Core/Project/*` 와 `Editor/Core/Export/{SpineProjectView,SkeletonModelReader}.cs` 포팅.
//!
//! 공개 API(계약 — 이름·시그니처를 바꾸지 말 것):
//! - [`SpineProject::load`]
//! - [`SpineProject::format_version`]
//! - [`SpineProject::skeleton_names`]
//! - [`SpineProject::read_model`]
//! - [`SpineProject::last_export_settings`]
//!
//! 모듈:
//! - `container`: raw deflate 컨테이너(+ 4.3.17 꼬리)
//! - `schema`: 내장 포맷 규칙(JSON)
//! - `document`: Kryo 객체 그래프 읽기와 «다시 쓰기 = 원본» 확인
//! - `view`: 태그 번호로 필드 찾기, 잎 값 꺼내기
//! - `reader`: 그래프 → [`SkeletonModel`]
//! - `export_settings`: 프로젝트에 저장된 내보내기 설정 → Spine 설정 JSON

mod container;
mod document;
mod export_settings;
mod reader;
mod schema;
mod view;

use std::panic::{catch_unwind, AssertUnwindSafe};

use spine_model::{SkeletonModel, SpineError, SpineResult};

use document::{Document, NodeId};
use schema::Schema;
use view::View;

/// 그래프는 깊게 재귀하므로 읽기는 큰 스택 스레드에서 돈다(C# BigStack).
const STACK_SIZE: usize = 64 * 1024 * 1024;

/// 내보내기용으로 읽은 `.spine` 프로젝트 하나(C# `SpineProjectFile.LoadForExport` 결과 + 스켈레톤 목록).
/// `Send + Sync` 여야 한다(백그라운드 스레드에서 여러 스켈레톤을 동시에 읽는다).
pub struct SpineProject {
    body: Vec<u8>,
    doc: Document,
    version: String,
    skeletons: Vec<(Option<String>, NodeId)>,
}

impl SpineProject {
    /// 압축된 `.spine` 파일 바이트 전체를 읽는다. 스키마는 크레이트 안에 내장한다.
    pub fn load(file: &[u8]) -> SpineResult<SpineProject> {
        let (body, doc) = big_stack(|| load_for_export(file))?;
        let version = doc.version.clone().unwrap_or_default();
        let skeletons = {
            let view = View { doc: &doc, body: &body, format: &version };
            reader::skeletons(&view)?
        };
        Ok(SpineProject { body, doc, version, skeletons })
    }

    /// 프로젝트에 적힌 포맷 버전 문자열(예: "4.2.43").
    pub fn format_version(&self) -> &str {
        &self.version
    }

    /// 프로젝트 안 스켈레톤 이름(프로젝트 순서).
    pub fn skeleton_names(&self) -> Vec<String> {
        self.skeletons.iter().map(|(n, _)| n.clone().unwrap_or_default()).collect()
    }

    /// index 번째 스켈레톤의 모델을 만든다.
    pub fn read_model(&self, index: usize) -> SpineResult<SkeletonModel> {
        self.read_model_with(index, false)
    }

    /// [`read_model`](Self::read_model) 과 같고, export_all 이면 Export 체크가 꺼진 첨부도 넣는다(Spine «Export all»).
    pub fn read_model_with(&self, index: usize, export_all: bool) -> SpineResult<SkeletonModel> {
        self.read_model_for(index, export_all, None)
    }

    /// [`read_model_with`](Self::read_model_with) 와 같고, 내보낼 Spine 버전(예: "4.2.43")을 알려 준다.
    /// 가중치 메시의 삼각형 순서가 내보내는 편집기 버전에 따라 달라서 필요하다: 4.2 이하 편집기는 프로젝트를 열며
    /// 삼각형을 정렬하고, 4.3 은 저장된 순서 그대로 쓴다. 버전을 주지 않으면 프로젝트를 저장한 버전의 편집기로 본다.
    pub fn read_model_for(&self, index: usize, export_all: bool, target_version: Option<&str>) -> SpineResult<SkeletonModel> {
        let Some(&(_, sk)) = self.skeletons.get(index) else {
            return Err(SpineError::Usage(format!(
                "스켈레톤 번호 {index}가 범위를 벗어났습니다(스켈레톤 {}개)",
                self.skeletons.len()
            )));
        };
        let view = View { doc: &self.doc, body: &self.body, format: &self.version };
        let read = || {
            let mut reader = reader::Reader::new(&view).export_all(export_all);
            if let Some(v) = target_version {
                reader = reader.target_version(v);
            }
            reader.read(sk)
        };
        match catch_unwind(AssertUnwindSafe(read)) {
            Ok(result) => result,
            Err(_) => Err(view::broken("내부 오류")),
        }
    }

    /// 내보내기용 부분 읽기에서 멈춘 이유(끝까지 읽었으면 None).
    pub fn partial_error(&self) -> Option<&str> {
        self.doc.partial_error.as_deref()
    }

    /// 프로젝트에 저장된 마지막 데이터 내보내기 설정(Spine Export 대화상자의 값).
    /// Spine CLI `--last-export-settings` 와 같은 모양의 JSON(`"class": "export-json" | "export-binary"`).
    /// 저장된 설정이 없거나(내보낸 적 없음) 그 부분을 읽지 못했으면 None.
    pub fn last_export_settings(&self) -> Option<serde_json::Value> {
        let view = View { doc: &self.doc, body: &self.body, format: &self.version };
        catch_unwind(AssertUnwindSafe(|| export_settings::last(&view))).ok().flatten()
    }

    /// 고른 스키마 이름(진단용).
    pub fn schema_name(&self) -> &str {
        &self.doc.schema.name
    }
}

fn big_stack<T: Send>(work: impl FnOnce() -> SpineResult<T> + Send) -> SpineResult<T> {
    std::thread::scope(|scope| {
        let handle = std::thread::Builder::new()
            .name("spine-project".into())
            .stack_size(STACK_SIZE)
            .spawn_scoped(scope, || catch_unwind(AssertUnwindSafe(work)));
        match handle {
            Ok(h) => match h.join() {
                Ok(Ok(result)) => result,
                _ => Err(view::broken("내부 오류")),
            },
            // 스레드를 만들 수 없다(메모리 부족 등)
            Err(_) => Err(SpineError::Io("읽기 스레드를 만들 수 없습니다".into())),
        }
    })
}

fn supports(s: &Schema, version: Option<&str>) -> bool {
    match version {
        Some(v) => s.formats.iter().any(|f| f == v),
        None => false,
    }
}

/// C# `SpineProjectFile.LoadForExport`: 맞는 스키마가 하나뿐이면 부분 읽기를 허용해 바로 읽고,
/// 여럿이면 «끝까지 읽힘 + 다시 쓰기 = 원본»으로 고른다(Load). 그래도 안 되면 부분 읽기로 다시 시험한다.
fn load_for_export(file: &[u8]) -> SpineResult<(Vec<u8>, Document)> {
    let schemas = schema::all();
    let body = container::decompress(file)?;
    let version = document::read_version(&body)?;
    let only: Vec<&'static Schema> = schemas.iter().filter(|s| supports(s, version.as_deref())).take(2).collect();
    if only.len() == 1 {
        if let Ok(doc) = Document::parse(&body, only[0], true) {
            return Ok((body, doc));
        }
        // 아래 기존 경로가 같은 오류를 알린다
    }

    match load(&body, version.as_deref()) {
        Ok(doc) => Ok((body, doc)),
        Err(SpineError::Format(message)) => {
            for schema in schemas.iter().filter(|s| supports(s, version.as_deref())) {
                if let Ok(doc) = Document::parse(&body, schema, true) {
                    return Ok((body, doc));
                }
            }
            Err(SpineError::Format(message))
        }
        Err(other) => Err(other),
    }
}

/// C# `SpineProjectFile.Load`(쓰기용 엄격한 읽기).
fn load(body: &[u8], version: Option<&str>) -> SpineResult<Document> {
    let schemas = schema::all();
    // 3.x 이하는 스키마가 맡은 버전(3.8)만 연다. 그 밖의 옛 버전은 구조가 달라 안내하고 거부한다.
    if let Some(v) = version {
        if let Ok(major) = v.split('.').next().unwrap_or("").trim().parse::<i32>() {
            if (1..=3).contains(&major) && !schemas.iter().any(|s| supports(s, version)) {
                return Err(SpineError::Usage(format!("Spine {v} 프로젝트입니다. Spine 3.8 이후 프로젝트만 지원합니다")));
            }
        }
    }

    let mut candidates: Vec<&'static Schema> = schemas.iter().filter(|s| supports(s, version)).collect();
    candidates.sort_by_key(|s| Some(s.name.as_str()) != version);
    if candidates.is_empty() {
        let mut have: Vec<&str> = schemas.iter().flat_map(|s| s.formats.iter().map(String::as_str)).collect();
        have.sort_unstable();
        have.dedup();
        return Err(SpineError::Format(format!(
            "지원하지 않는 프로젝트 포맷 버전 '{}' (지원: {})",
            version.unwrap_or(""),
            have.join(", ")
        )));
    }

    let mut errors = Vec::new();
    for schema in candidates {
        match Document::parse(body, schema, false) {
            Ok(doc) => {
                if document::rewrite_equals(&doc, body) {
                    return Ok(doc);
                }
                errors.push(format!("{}: 다시 쓰기가 원본과 다름", schema.name));
            }
            Err(SpineError::Format(m)) => errors.push(format!("{}: {}", schema.name, m)),
            Err(other) => return Err(other),
        }
    }
    Err(SpineError::Format(errors.join("; ")))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn send_sync() {
        fn check<T: Send + Sync>() {}
        check::<SpineProject>();
    }

    #[test]
    fn schemas_parse() {
        assert_eq!(schema::all().len(), 14);
    }
}
