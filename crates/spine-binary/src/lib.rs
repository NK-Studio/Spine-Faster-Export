//! 스켈레톤 모델 → `.skel` 바이너리, 애니메이션 정리.
//! 파이썬 `export_poc/binary_writer.py`·`anim_clean.py` 포팅.
//!
//! 공개 API(계약): [`unsupported_binary`], [`write_binary`], [`clean_model`]

mod clean;
mod out;
mod util;
mod writer;

use spine_model::{SkeletonModel, SpineError, SpineResult};

/// 이 버전의 `.skel` 로 낼 수 없으면 그 이유(파이썬 `unsupported`). 낼 수 있으면 None.
pub fn unsupported_binary(model: &SkeletonModel, version: &str) -> Option<String> {
    let major = match writer::parse_version(version) {
        Ok((m, _)) => m,
        Err(e) => return Some(e.message().to_string()),
    };
    let mut bad: Vec<&str> = Vec::new();
    for c in &model.constraints {
        if c.type_str() == "slider" && major < (4, 3) {
            bad.push("슬라이더 제약(4.3 전용)");
        }
        if c.type_str() == "physics" && major < (4, 2) {
            bad.push("물리 제약(4.2 이상)");
        }
    }
    if major < (4, 0) {
        for c in &model.constraints {
            if c.type_str() == "path" && c.spacing_mode.as_deref() == Some("proportional") {
                bad.push("경로 제약의 Proportional 간격(4.0 이상)");
            }
        }
    }
    if major < (4, 1) {
        for s in &model.skins {
            for e in &s.attachments {
                if e.attachment
                    .as_ref()
                    .map(|a| a.sequence.is_some())
                    .unwrap_or(false)
                {
                    bad.push("시퀀스 첨부(4.1 이상)");
                }
            }
        }
    }
    bad.sort();
    bad.dedup();
    if bad.is_empty() {
        None
    } else {
        Some(bad.join(", "))
    }
}

/// 모델을 Spine `.skel`(3.8.xx, 4.0.xx~4.3.xx)로 쓴다.
pub fn write_binary(
    model: &SkeletonModel,
    version: &str,
    nonessential: bool,
) -> SpineResult<Vec<u8>> {
    if let Some(bad) = unsupported_binary(model, version) {
        return Err(SpineError::Unsupported(bad));
    }
    writer::Writer::new(model, version, nonessential)?.build()
}

/// Spine 의 "애니메이션 정리"와 같은 키 정리를 모델에 적용한다.
pub fn clean_model(model: &mut SkeletonModel) {
    clean::clean_model(model)
}

#[cfg(test)]
mod tests {
    use super::*;
    use spine_model::{ModelAnimation, ModelBone, ModelKey, ModelSlot, ModelTimeline};

    fn tiny() -> SkeletonModel {
        SkeletonModel {
            fps: 30.0,
            reference_scale: 100.0,
            images: Some(String::new()),
            audio: Some(String::new()),
            bones: vec![ModelBone {
                name: Some("root".into()),
                scale_x: 1.0,
                scale_y: 1.0,
                inherit: Some("normal".into()),
                icon: Some("bone".into()),
                visible: true,
                editor33: 1.0,
                ..Default::default()
            }],
            slots: vec![ModelSlot {
                name: Some("s".into()),
                bone: Some("root".into()),
                color: Some("ffffffff".into()),
                blend: Some("normal".into()),
                visible: true,
                ..Default::default()
            }],
            animations: vec![ModelAnimation {
                name: Some("anim".into()),
                timelines: vec![ModelTimeline {
                    r#type: Some("rotate".into()),
                    bone: Some("root".into()),
                    keys: vec![
                        ModelKey {
                            frame: 0.0,
                            curve: Some("bezier".into()),
                            values: Some(vec![0.0]),
                            handles: Some(vec![[util::UNSET; 4]]),
                            ..Default::default()
                        },
                        ModelKey {
                            frame: 30.0,
                            curve: Some("linear".into()),
                            values: Some(vec![90.0]),
                            handles: Some(vec![[util::UNSET; 4]]),
                            ..Default::default()
                        },
                    ],
                    ..Default::default()
                }],
                ..Default::default()
            }],
            ..Default::default()
        }
    }

    #[test]
    fn unsupported_messages() {
        let mut m = tiny();
        m.constraints.push(spine_model::ModelConstraint {
            r#type: Some("slider".into()),
            ..Default::default()
        });
        m.constraints.push(spine_model::ModelConstraint {
            r#type: Some("physics".into()),
            ..Default::default()
        });
        assert_eq!(
            unsupported_binary(&m, "4.1.24").as_deref(),
            Some("물리 제약(4.2 이상), 슬라이더 제약(4.3 전용)")
        );
        assert_eq!(
            unsupported_binary(&m, "4.2.43").as_deref(),
            Some("슬라이더 제약(4.3 전용)")
        );
        assert!(write_binary(&m, "4.0.64", false).is_err());
    }

    /// 손으로 계산한 기대 바이트(4.2, 비필수 없음).
    #[test]
    fn tiny_skel_42() {
        let b = write_binary(&tiny(), "4.2.43", false).unwrap();
        let mut e: Vec<u8> = vec![7];
        e.extend_from_slice(b"4.2.43");
        e.extend_from_slice(&[0; 16]); // bounds
        e.extend_from_slice(&100f32.to_be_bytes());
        e.push(0); // nonessential
        e.push(0); // strings
                   // bones
        e.push(1);
        e.extend_from_slice(&[5, b'r', b'o', b'o', b't']);
        for v in [0f32, 0., 0., 1., 1., 0., 0., 0.] {
            e.extend_from_slice(&v.to_be_bytes());
        }
        e.push(0); // inherit
        e.push(0); // skin
                   // slots
        e.extend_from_slice(&[1, 2, b's', 0]);
        e.extend_from_slice(&[0xFF; 8]);
        e.extend_from_slice(&[0, 0]); // attachment null, blend
        e.extend_from_slice(&[0, 0, 0, 0]); // ik, transform, path, physics
        e.push(0); // default skin: 슬롯 0
        e.push(0); // skins
        e.push(0); // events
        e.push(1); // animations
        e.extend_from_slice(&[5, b'a', b'n', b'i', b'm']);
        e.push(1); // timeline count
        e.push(0); // slots
        e.extend_from_slice(&[1, 0, 1]); // bones: 1, bone 0, 1 timeline
        e.extend_from_slice(&[0, 2, 1]); // rotate, 2 keys, 1 bezier
        e.extend_from_slice(&0f32.to_be_bytes());
        e.extend_from_slice(&0f32.to_be_bytes());
        let t1 = (30f32 as f64 * (1.0f64 / 30.0) as f32 as f64) as f32;
        e.extend_from_slice(&t1.to_be_bytes());
        e.extend_from_slice(&90f32.to_be_bytes());
        e.push(2);
        let sec = |f: f64| (f as f32 as f64 * (1.0f64 / 30.0) as f32 as f64) as f32;
        for v in [sec(10.0), 0.0, sec(20.0), 90.0] {
            e.extend_from_slice(&v.to_be_bytes());
        }
        e.extend_from_slice(&[0, 0, 0, 0, 0, 0, 0]); // ik tr path phys deform draw events
        assert_eq!(&b[8..], &e[..]);
        let h = sha1_smol::Sha1::from(&b[8..]).digest().bytes();
        assert_eq!(&b[..8], &h[..8]);
    }

    /// 3.8 의 머리말부터 뼈 타임라인 앞까지(해시 문자열 뒤). 3.8 은 해시 · 버전이 문자열이고 reference scale · 물리가 없다.
    fn head_38() -> Vec<u8> {
        let mut e: Vec<u8> = vec![7];
        e.extend_from_slice(b"3.8.99");
        e.extend_from_slice(&[0; 16]); // x y width height
        e.push(0); // nonessential
        e.push(0); // strings
        e.push(1); // bones
        e.extend_from_slice(&[5, b'r', b'o', b'o', b't']);
        for v in [0f32, 0., 0., 1., 1., 0., 0., 0.] {
            e.extend_from_slice(&v.to_be_bytes());
        }
        e.extend_from_slice(&[0, 0]); // transform mode, skin required
        e.extend_from_slice(&[1, 2, b's', 0]); // slots: 1, "s", bone 0
        e.extend_from_slice(&[0xFF; 8]); // color, dark(-1)
        e.extend_from_slice(&[0, 0]); // attachment null, blend
        e.extend_from_slice(&[0, 0, 0]); // ik, transform, path
        e.push(0); // default skin: 슬롯 0
        e.push(0); // skins
        e.push(0); // events
        e.push(1); // animations
        e.extend_from_slice(&[5, b'a', b'n', b'i', b'm']);
        e.push(0); // slot timelines
        e.extend_from_slice(&[1, 0, 1]); // bones: 1, bone 0, 1 timeline
        e
    }

    /// 손으로 계산한 기대 바이트(3.8, 비필수 없음). 4.x 프로젝트의 베지어는 직선(0)으로 나간다.
    #[test]
    fn tiny_skel_38() {
        let b = write_binary(&tiny(), "3.8.99", false).unwrap();
        // 해시: 길이 + 1 을 적은 27자 문자열
        assert_eq!(b[0], 28);
        assert!(b[1..28].iter().all(|c| c.is_ascii_alphanumeric() || *c == b'+' || *c == b'/'));
        let mut e = head_38();
        e.extend_from_slice(&[0, 2]); // rotate, 2 keys
        for v in [0f32, 0.] {
            e.extend_from_slice(&v.to_be_bytes());
        }
        e.push(0); // 직선
        for v in [1f32, 90.] {
            e.extend_from_slice(&v.to_be_bytes());
        }
        e.extend_from_slice(&[0, 0, 0, 0, 0, 0]); // ik tr path deform draw events
        assert_eq!(&b[28..], &e[..]);
    }

    /// 3.8 프로젝트의 키: 곡선은 저장된 값 그대로, 회전은 저장된 각도로 쓰고 비교한다.
    #[test]
    fn keys_of_38_project() {
        let mut m = tiny();
        let key = |frame: f64, angle: f64, joined: f64, curve: &str, c: Option<[f64; 4]>| ModelKey {
            frame,
            curve: Some(curve.into()),
            values: Some(vec![joined]),
            handles: Some(vec![[util::UNSET; 4]]),
            angle38: Some(angle),
            curve38: c,
            ..Default::default()
        };
        m.animations[0].timelines[0].keys = vec![
            key(0.0, -297.81, -297.81, "linear", None),
            // 62.19 는 이어 붙이면 -297.81 과 같은 값이지만 3.8 에서는 다른 키다: 계단이 아니다
            key(10.0, 62.19, -297.81, "bezier", Some([0.25, 0.0, 0.75, 1.0])),
            key(20.0, 30.0, -330.0, "bezier", Some([0.0, 0.0, 1.0, 1.0])), // 직선과 같은 곡선
            key(30.0, 30.0, -330.0, "linear", None), // 같은 각도: 계단
            key(40.0, 30.0, -330.0, "linear", None),
        ];
        let b = write_binary(&m, "3.8.99", false).unwrap();
        let mut e = head_38();
        e.extend_from_slice(&[0, 5]); // rotate, 5 keys
        let f = |e: &mut Vec<u8>, vs: &[f32]| vs.iter().for_each(|v| e.extend_from_slice(&v.to_be_bytes()));
        f(&mut e, &[0.0, -297.81]);
        e.push(0); // 직선
        f(&mut e, &[(10.0f64 / 30.0) as f32, 62.19]);
        e.push(2); // 베지어
        f(&mut e, &[0.25, 0.0, 0.75, 1.0]);
        f(&mut e, &[(20.0f64 / 30.0) as f32, 30.0]);
        e.push(0); // (0, 0, 1, 1) 은 직선
        f(&mut e, &[1.0, 30.0]);
        e.push(1); // 계단
        f(&mut e, &[(40.0f64 / 30.0) as f32, 30.0]);
        e.extend_from_slice(&[0, 0, 0, 0, 0, 0]);
        assert_eq!(&b[28..], &e[..]);
    }
}
