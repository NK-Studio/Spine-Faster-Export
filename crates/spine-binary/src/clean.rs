//! 애니메이션 정리: Spine 내보내기의 "애니메이션 정리"와 같은 일을 모델에 적용한다(파이썬 `anim_clean.py`).
//!
//! - 값이 앞 키와 같고 그 구간이 평평하며, 뒤 키도 같거나 마지막인 키는 지운다.
//! - 남은 키가 설정 자세와 같은 키 하나뿐이면 타임라인을 지운다.
//! - 첨부·드로우 오더는 앞 키와 같으면 지우고, 앞쪽의 설정과 같은 키도 지운다.
//! - 그 결과 애니메이션이 짧아지면, 마지막 시간에 키가 있던 타임라인 중 맨 뒤의 것에 그 키를 되살린다.
//! - 앞쪽이 설정 자세로 평평하면 변화 직전 키만 남긴다.
//! - IK, 물리, 슬라이더, 이벤트, 시퀀스는 건드리지 않는다.

use std::collections::HashMap;

use spine_model::{ModelConstraint, ModelKey, ModelSlot, ModelTimeline, SkeletonModel};

use crate::util::{byte255, curve_str, hds, ulps, vals, UNSET};

fn channels(ty: &str) -> Option<usize> {
    Some(match ty {
        "rotate" | "translatex" | "translatey" | "pathPosition" | "pathSpacing" => 1,
        "translate" | "scale" | "shear" => 2,
        "color" => 7,
        "transform" => 6,
        "pathMix" => 3,
        _ => return None,
    })
}

/// 정리 대상 타임라인인가(CHANNELS 또는 첨부·변형·드로우 오더).
fn cleanable(ty: &str) -> bool {
    channels(ty).is_some() || matches!(ty, "attachment" | "deform" | "drawOrder")
}

fn near(a: f64, b: f64) -> bool {
    (a - b).abs() <= 1e-6 || ulps(a, b) <= 1
}

fn color_bytes(v: &[f64]) -> Vec<i64> {
    v.iter().map(|x| byte255(*x)).collect()
}

fn hex_bytes(h: &str, n: usize) -> Vec<i64> {
    (0..n)
        .map(|i| {
            h.get(i * 2..i * 2 + 2)
                .and_then(|x| i64::from_str_radix(x, 16).ok())
                .unwrap_or(-1)
        })
        .collect()
}

/// 드로우 오더 키에서 실제로 움직인 슬롯(정렬).
fn order(k: &ModelKey) -> Vec<(String, i64)> {
    let mut v: Vec<(String, i64)> = k
        .offsets
        .as_deref()
        .unwrap_or(&[])
        .iter()
        .filter(|(_, n)| *n != 0)
        .cloned()
        .collect();
    v.sort();
    v
}

struct Cleaner<'a> {
    slots: HashMap<&'a str, &'a ModelSlot>,
    cons: HashMap<&'a str, &'a ModelConstraint>,
}

impl<'a> Cleaner<'a> {
    fn same(&self, ty: &str, a: &ModelKey, b: &ModelKey) -> bool {
        match ty {
            "attachment" => a.name == b.name,
            "deform" => {
                let (x, y) = (
                    a.dense.as_deref().unwrap_or(&[]),
                    b.dense.as_deref().unwrap_or(&[]),
                );
                let n = x.len().max(y.len());
                (0..n).all(|i| {
                    near(
                        x.get(i).copied().unwrap_or(0.0),
                        y.get(i).copied().unwrap_or(0.0),
                    )
                })
            }
            "drawOrder" => order(a) == order(b),
            "color" => color_bytes(vals(a)) == color_bytes(vals(b)),
            _ => vals(a).iter().zip(vals(b)).all(|(p, q)| near(*p, *q)),
        }
    }

    /// a 에서 b 까지 값이 변하지 않는가(a 와 b 의 값이 같다고 가정).
    fn flat(&self, ty: &str, a: &ModelKey, b: &ModelKey) -> bool {
        if matches!(ty, "attachment" | "drawOrder") || curve_str(a) != "bezier" {
            return true;
        }
        if ty == "deform" {
            return true;
        }
        let n = channels(ty).unwrap_or(1);
        let (ha, hb) = (hds(a), hds(b));
        if ha.len() < n || hb.len() < n {
            return false;
        }
        let (va, vb) = (vals(a), vals(b));
        // 정해지지 않은 손잡이(UNSET)는 값 높이에 놓이므로 평평하다
        (0..n).all(|c| {
            (ha[c][3] == UNSET || (ha[c][3] - va.get(c).copied().unwrap_or(0.0)).abs() <= 1e-5)
                && (hb[c][1] == UNSET
                    || (hb[c][1] - vb.get(c).copied().unwrap_or(0.0)).abs() <= 1e-5)
        })
    }

    fn is_setup(&self, t: &ModelTimeline, k: &ModelKey) -> bool {
        let v = vals(k);
        let g = |i: usize| v.get(i).copied().unwrap_or(0.0);
        let con = || {
            self.cons
                .get(t.constraint.as_deref().unwrap_or(""))
                .copied()
        };
        match t.type_str() {
            "rotate" | "translatex" | "translatey" => near(g(0), 0.0),
            "translate" | "shear" => near(g(0), 0.0) && near(g(1), 0.0),
            "scale" => near(g(0), 1.0) && near(g(1), 1.0),
            "attachment" => match self.slots.get(t.slot.as_deref().unwrap_or("")) {
                Some(s) => k.name == s.attachment,
                None => false,
            },
            "deform" => k
                .dense
                .as_deref()
                .unwrap_or(&[])
                .iter()
                .all(|x| near(*x, 0.0)),
            "drawOrder" => order(k).is_empty(),
            "color" => {
                let Some(s) = self.slots.get(t.slot.as_deref().unwrap_or("")) else {
                    return false;
                };
                let cb = color_bytes(v);
                let c = hex_bytes(s.color.as_deref().unwrap_or(""), 4);
                // 투 컬러 슬롯은 어두운 색까지 같아야 설정과 같다
                cb.get(..4) == Some(&c[..])
                    && match &s.dark {
                        None => true,
                        Some(d) => cb.get(4..7.min(cb.len())) == Some(&hex_bytes(d, 3)[..]),
                    }
            }
            "transform" => match con() {
                Some(c) => v.iter().zip(c.mixes.iter()).all(|(x, m)| near(*x, *m)),
                None => false,
            },
            "pathPosition" => con().map(|c| near(g(0), c.position)).unwrap_or(false),
            "pathSpacing" => con().map(|c| near(g(0), c.spacing)).unwrap_or(false),
            "pathMix" => match con() {
                Some(c) => v.iter().zip(c.mixes[..3].iter()).all(|(x, m)| near(*x, *m)),
                None => false,
            },
            _ => false,
        }
    }

    fn clean_keys(&self, t: &mut ModelTimeline) {
        let ty = t.type_str().to_string();
        let keys = std::mem::take(&mut t.keys);
        if keys.is_empty() {
            return;
        }
        let mut out: Vec<ModelKey> = Vec::with_capacity(keys.len());
        if ty == "attachment" || ty == "drawOrder" {
            // 보간이 없는 타임라인: 앞 키와 같으면 지운다
            let mut it = keys.into_iter();
            out.push(it.next().unwrap());
            for k in it {
                if !self.same(&ty, out.last().unwrap(), &k) {
                    out.push(k);
                }
            }
            // 설정과 같은 앞쪽 키는 없어도 결과가 같다
            let n = out.iter().take_while(|k| self.is_setup(t, k)).count();
            out.drain(..n);
            t.keys = out;
            return;
        }
        let n = keys.len();
        let mut keep = vec![false; n];
        keep[0] = true;
        let mut last = 0usize;
        for i in 1..n {
            let (k, prev) = (&keys[i], &keys[last]);
            let nxt = keys.get(i + 1);
            if self.same(&ty, prev, k)
                && self.flat(&ty, prev, k)
                && nxt
                    .map(|x| self.same(&ty, k, x) && self.flat(&ty, k, x))
                    .unwrap_or(true)
            {
                continue;
            }
            keep[i] = true;
            last = i;
        }
        out.extend(
            keys.into_iter()
                .zip(keep)
                .filter(|(_, k)| *k)
                .map(|(x, _)| x),
        );
        // 설정 자세로 시작해 값이 안 변하는 앞쪽 키: 변화 직전 키만 남긴다
        let mut drop = 0;
        while out.len() - drop >= 2
            && self.is_setup(t, &out[drop])
            && self.same(&ty, &out[drop], &out[drop + 1])
            && self.flat(&ty, &out[drop], &out[drop + 1])
        {
            drop += 1;
        }
        out.drain(..drop);
        t.keys = out;
    }

    /// 타임라인을 남길지 돌려준다.
    fn clean_timeline(&self, t: &mut ModelTimeline) -> bool {
        let ty = t.type_str().to_string();
        if !cleanable(&ty) {
            return true;
        }
        self.clean_keys(t);
        if t.keys.is_empty() {
            return false;
        }
        if t.keys.len() == 1 && self.is_setup(t, &t.keys[0]) {
            return false;
        }
        // 지운 키 때문에 이웃이 된 두 키의 값이 같으면 사이 구간은 계단이다
        for i in 0..t.keys.len() - 1 {
            if self.same(&ty, &t.keys[i], &t.keys[i + 1])
                && self.flat(&ty, &t.keys[i], &t.keys[i + 1])
            {
                t.keys[i].curve = Some("stepped".to_string());
            }
        }
        true
    }
}

fn last_frame<'b, I: IntoIterator<Item = &'b ModelTimeline>>(tls: I) -> f64 {
    let mut m: Option<f64> = None;
    for t in tls {
        for k in &t.keys {
            m = Some(match m {
                Some(x) if x >= k.frame => x,
                _ => k.frame,
            });
        }
    }
    m.unwrap_or(0.0)
}

pub fn clean_model(model: &mut SkeletonModel) {
    // 설정값 표는 애니메이션을 고치기 전에 복사해 둔다(빌림 충돌 방지). 같은 이름이면 뒤의 것이 이긴다.
    let slots = model.slots.clone();
    let cons = model.constraints.clone();
    let cleaner = Cleaner {
        slots: slots
            .iter()
            .map(|s| (s.name.as_deref().unwrap_or(""), s))
            .collect(),
        cons: cons
            .iter()
            .map(|c| (c.name.as_deref().unwrap_or(""), c))
            .collect(),
    };
    for an in &mut model.animations {
        if an.no_clean {
            // 애니메이션의 정리 제외 표시
            continue;
        }
        let before: Vec<Option<ModelKey>> = an
            .timelines
            .iter()
            .map(|t| t.keys.last().cloned())
            .collect();
        let duration = last_frame(&an.timelines);
        let mut kept: Vec<bool> = an
            .timelines
            .iter_mut()
            .map(|t| cleaner.clean_timeline(t))
            .collect();
        let kept_last = last_frame(
            an.timelines
                .iter()
                .zip(&kept)
                .filter(|(_, k)| **k)
                .map(|(t, _)| t),
        );
        if kept_last < duration {
            // 정리로 애니메이션 길이가 줄면 마지막 키를 하나 남긴다(원래 순서에서 마지막으로 나오는 타임라인)
            for i in (0..an.timelines.len()).rev() {
                let Some(key) = &before[i] else { continue };
                let t = &mut an.timelines[i];
                if key.frame == duration && cleanable(t.type_str()) {
                    if !kept[i] {
                        t.keys.clear();
                        kept[i] = true;
                    }
                    t.keys.push(key.clone());
                    break;
                }
            }
        }
        let mut i = 0;
        an.timelines.retain(|_| {
            let k = kept[i];
            i += 1;
            k
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use spine_model::ModelAnimation;

    fn key(frame: f64, v: &[f64], curve: &str) -> ModelKey {
        ModelKey {
            frame,
            curve: Some(curve.into()),
            values: Some(v.to_vec()),
            handles: Some(vec![]),
            ..Default::default()
        }
    }

    fn tl(ty: &str, keys: Vec<ModelKey>) -> ModelTimeline {
        ModelTimeline {
            r#type: Some(ty.into()),
            bone: Some("b".into()),
            keys,
            ..Default::default()
        }
    }

    #[test]
    fn removes_flat_and_setup() {
        let mut m = SkeletonModel::default();
        m.animations.push(ModelAnimation {
            name: Some("a".into()),
            timelines: vec![
                // 설정 자세 하나만 남는 회전 → 지운다
                tl(
                    "rotate",
                    vec![key(0.0, &[0.0], "linear"), key(10.0, &[0.0], "linear")],
                ),
                // 가운데 같은 값 키 제거
                tl(
                    "translate",
                    vec![
                        key(0.0, &[1.0, 1.0], "linear"),
                        key(5.0, &[1.0, 1.0], "linear"),
                        key(10.0, &[1.0, 1.0], "linear"),
                        key(20.0, &[2.0, 1.0], "linear"),
                    ],
                ),
            ],
            ..Default::default()
        });
        clean_model(&mut m);
        let an = &m.animations[0];
        assert_eq!(an.timelines.len(), 1);
        let fr: Vec<f64> = an.timelines[0].keys.iter().map(|k| k.frame).collect();
        assert_eq!(fr, vec![0.0, 10.0, 20.0]);
        assert_eq!(an.timelines[0].keys[0].curve.as_deref(), Some("stepped"));
    }

    #[test]
    fn restores_last_key_when_shortened() {
        let mut m = SkeletonModel::default();
        m.animations.push(ModelAnimation {
            name: Some("a".into()),
            timelines: vec![
                tl(
                    "rotate",
                    vec![key(0.0, &[5.0], "linear"), key(30.0, &[5.0], "linear")],
                ),
                tl(
                    "scale",
                    vec![
                        key(0.0, &[1.0, 1.0], "linear"),
                        key(30.0, &[1.0, 1.0], "linear"),
                    ],
                ),
            ],
            ..Default::default()
        });
        clean_model(&mut m);
        let an = &m.animations[0];
        // 회전은 [0] 만 남고, 길이를 지키려고 맨 뒤 타임라인(scale)에 30 프레임 키를 되살린다
        assert_eq!(an.timelines.len(), 2);
        assert_eq!(an.timelines[0].keys.len(), 1);
        assert_eq!(an.timelines[1].keys.len(), 1);
        assert_eq!(an.timelines[1].keys[0].frame, 30.0);
    }
}
