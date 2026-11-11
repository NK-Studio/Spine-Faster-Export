//! 4.x 모델 → 3.8 JSON 에 필요한 변환(spine-runtimes 3.8 `SkeletonJson` 기준).
//!
//! Spine 공식 CLI 의 3.8 내보내기(4.x → 4.0 → 3.8)와 같게 쓴다:
//! - 곡선: 4.x 프로젝트의 베지어 곡선은 직선으로 바꾼다(계단은 그대로). Spine 도 "Bezier curves changed to linear" 라고 알린다.
//!   3.8 프로젝트는 곡선이 원래 키마다 하나라 그대로 옮긴다(Spine 3.8 편집기의 내보내기와 같다).
//! - 회전: 키를 나누지 않는다(3.8 런타임은 키 사이를 짧은 쪽으로 돈다).
//! - 이동: 4.x 의 X·Y 따로 이동은 3.8 에 없어 한 타임라인으로 합친다.

use spine_model::ModelKey;

use crate::{eq, handles, values, UNSET};

pub(crate) const NOTE_TRANSFORM_MIX: &str = "3.8 에는 Y·scaleY mix 가 따로 없어 X·scaleX mix 로 내보냈습니다(변형 제약)";
pub(crate) const NOTE_PATH_MIX: &str = "3.8 에는 경로 제약의 Y mix 가 따로 없어 X mix 로 내보냈습니다";
pub(crate) const NOTE_LINEAR: &str = "3.8 로 내보내며 베지어 곡선을 직선으로 바꿨습니다(Spine 의 3.8 내보내기와 같음)";
pub(crate) const NOTE_TRANSLATE_MERGE: &str = "3.8 은 이동 X·Y 키를 따로 둘 수 없어 한 타임라인으로 합쳤습니다";
pub(crate) const NOTE_TRANSLATE_DROP: &str = "이동과 X·Y 따로 이동이 함께 있어 X·Y 따로 이동은 뺐습니다";
pub(crate) const NOTE_SEQUENCE: &str = "시퀀스 키는 3.8 에 없어 뺐습니다";

type P = (f64, f64);

fn v0(k: &ModelKey) -> f64 {
    values(k).first().copied().unwrap_or(0.0)
}

/// 채널 c 의 베지어 네 점 (프레임, 값). 값이 없는 타임라인(변형)은 0→1 비율.
/// 손잡이가 없으면 4.x 편집기의 기본값(시간 ⅓·⅔, 값은 양 끝).
fn seg(k: &ModelKey, next: &ModelKey, c: usize) -> [P; 4] {
    let (t0, t1) = (k.frame, next.frame);
    let (a, b) = (values(k), values(next));
    let (va, vb) = if a.len() > c && b.len() > c { (a[c], b[c]) } else { (0.0, 1.0) };
    let dt = t1 - t0;
    let mut p1 = (t0 + dt / 3.0, va);
    if let Some(h) = handles(k).get(c) {
        if h[2] != UNSET {
            p1 = (h[2], h[3]);
        }
    }
    let mut p2 = (t1 - dt / 3.0, vb);
    if let Some(h) = handles(next).get(c) {
        if h[0] != UNSET {
            p2 = (h[0], h[1]);
        }
    }
    [(t0, va), p1, p2, (t1, vb)]
}

/// 3.8 프로젝트의 k → next 곡선 [c1 c2 c3 c4](0~1 로 정규화). 직선과 같거나 값이 변하지 않으면 None.
/// 3.8 프로젝트를 읽을 때 키의 곡선 하나를 모든 채널에 똑같이 펼쳐 두었으므로(spine-project `handles38`) 값이 변하는 채널 하나에서 되돌리면 된다.
pub(crate) fn curve(k: &ModelKey, next: &ModelKey, channels: usize) -> Option<[f64; 4]> {
    let (a, b) = (values(k), values(next));
    let delta = |c: usize| if a.len() > c && b.len() > c { (b[c] - a[c]).abs() } else { 1.0 };
    let c = (0..channels.max(1)).fold(0, |best, c| if delta(c) > delta(best) { c } else { best });
    let p = seg(k, next, c);
    let (dt, dv) = (p[3].0 - p[0].0, p[3].1 - p[0].1);
    if dt.is_nan() || dt <= 0.0 || dv.abs() <= 1e-9 {
        return None;
    }
    let n = [(p[1].0 - p[0].0) / dt, (p[1].1 - p[0].1) / dv, (p[2].0 - p[0].0) / dt, (p[2].1 - p[0].1) / dv];
    let near = |x: f64, y: f64| (x - y).abs() <= 1e-4;
    let linear = near(n[0], 1.0 / 3.0) && near(n[1], 1.0 / 3.0) && near(n[2], 2.0 / 3.0) && near(n[3], 2.0 / 3.0);
    if linear {
        None
    } else {
        Some(n)
    }
}

fn bezier(p: [P; 4], s: f64) -> P {
    let u = 1.0 - s;
    let (w0, w1, w2, w3) = (u * u * u, 3.0 * u * u * s, 3.0 * u * s * s, s * s * s);
    (w0 * p[0].0 + w1 * p[1].0 + w2 * p[2].0 + w3 * p[3].0, w0 * p[0].1 + w1 * p[1].1 + w2 * p[2].1 + w3 * p[3].1)
}

/// 곡선 위에서 시간이 frame 인 점의 값(시간은 손잡이 사이에서 단조라 이분법으로 매개변수를 찾는다).
fn value_at(p: [P; 4], frame: f64) -> f64 {
    let (mut lo, mut hi) = (0.0, 1.0);
    for _ in 0..52 {
        let mid = (lo + hi) / 2.0;
        if bezier(p, mid).0 < frame {
            lo = mid;
        } else {
            hi = mid;
        }
    }
    bezier(p, (lo + hi) / 2.0).1
}

/// 한 채널 타임라인의 frame 에서의 값(4.x 곡선 그대로).
fn eval(keys: &[ModelKey], frame: f64) -> f64 {
    let Some(first) = keys.first() else {
        return 0.0;
    };
    if frame <= first.frame {
        return v0(first);
    }
    for w in keys.windows(2) {
        let (a, b) = (&w[0], &w[1]);
        if frame >= b.frame {
            continue;
        }
        if eq(&a.curve, "stepped") {
            return v0(a);
        }
        if eq(&a.curve, "linear") {
            return v0(a) + (v0(b) - v0(a)) * (frame - a.frame) / (b.frame - a.frame);
        }
        return value_at(seg(a, b, 0), frame);
    }
    v0(keys.last().unwrap())
}

/// 한 채널 키를 두 채널(x, y) 키로. 없는 채널은 0.
fn widen(keys: &[ModelKey], at: usize) -> Vec<ModelKey> {
    keys.iter()
        .map(|k| {
            let mut values = vec![0.0, 0.0];
            values[at] = v0(k);
            ModelKey { values: Some(values), handles: None, ..k.clone() }
        })
        .collect()
}

/// X·Y 따로 이동 → 이동 하나. 둘째 값: 두 채널이 모두 있어 키를 합쳤는지.
pub(crate) fn merge_translate(x: Option<&[ModelKey]>, y: Option<&[ModelKey]>) -> (Vec<ModelKey>, bool) {
    let (xs, ys) = match (x, y) {
        (Some(xs), None) => return (widen(xs, 0), false),
        (None, Some(ys)) => return (widen(ys, 1), false),
        (None, None) => return (Vec::new(), false),
        (Some(xs), Some(ys)) => (xs, ys),
    };
    // 프레임을 모아 각 채널 값을 4.x 곡선대로 계산한다. 두 채널이 모두 계단인 구간만 계단으로 둔다
    let mut frames: Vec<f64> = xs.iter().chain(ys).map(|k| k.frame).collect();
    frames.sort_by(|a, b| a.total_cmp(b));
    frames.dedup();
    let stepped_at = |ks: &[ModelKey], f: f64| ks.windows(2).find(|w| f >= w[0].frame && f < w[1].frame).is_some_and(|w| eq(&w[0].curve, "stepped"));
    let keys = frames
        .iter()
        .map(|&f| {
            let curve = if stepped_at(xs, f) && stepped_at(ys, f) { "stepped" } else { "linear" };
            ModelKey { frame: f, curve: Some(curve.into()), values: Some(vec![eval(xs, f), eval(ys, f)]), ..Default::default() }
        })
        .collect();
    (keys, true)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(frame: f64, v: f64, curve: &str) -> ModelKey {
        ModelKey { frame, curve: Some(curve.into()), values: Some(vec![v]), handles: Some(vec![[UNSET; 4]]), ..Default::default() }
    }

    #[test]
    fn curve_round_trip() {
        // 3.8 곡선 (0.25, 0, 0.75, 1) 을 4.x 손잡이로 펼친 것(handles38 과 같은 계산) → 다시 같은 곡선
        let mut a = key(0.0, 10.0, "bezier");
        a.handles = Some(vec![[UNSET, UNSET, 7.5, 10.0]]);
        let mut b = key(30.0, 50.0, "linear");
        b.handles = Some(vec![[22.5, 50.0, UNSET, UNSET]]);
        let c = curve(&a, &b, 1).unwrap();
        assert!(c.iter().zip([0.25, 0.0, 0.75, 1.0]).all(|(x, y)| (x - y).abs() < 1e-9));
        assert_eq!(curve(&key(0.0, 0.0, "linear"), &key(30.0, 0.0, "linear"), 1), None);
    }

    #[test]
    fn merge_translate_union() {
        let xs = vec![key(0.0, 0.0, "linear"), key(30.0, 30.0, "linear")];
        let ys = vec![key(0.0, 0.0, "linear"), key(10.0, 10.0, "linear"), key(30.0, 0.0, "linear")];
        let (keys, merged) = merge_translate(Some(&xs), Some(&ys));
        assert!(merged);
        let got: Vec<(f64, Vec<f64>)> = keys.iter().map(|k| (k.frame, k.values.clone().unwrap())).collect();
        assert_eq!(got, vec![(0.0, vec![0.0, 0.0]), (10.0, vec![10.0, 10.0]), (30.0, vec![30.0, 0.0])]);
        let (only_y, merged) = merge_translate(None, Some(&ys));
        assert!(!merged && only_y[1].values.as_deref() == Some(&[0.0, 10.0][..]));
    }

    #[test]
    fn merged_values_follow_bezier() {
        // x 는 곡선(빨리 출발), y 키 시각에서 x 값은 직선보다 앞서 있다
        let mut a = key(0.0, 0.0, "bezier");
        a.handles = Some(vec![[UNSET, UNSET, 3.0, 90.0]]);
        let xs = vec![a, key(30.0, 100.0, "linear")];
        let ys = vec![key(0.0, 0.0, "linear"), key(15.0, 5.0, "linear"), key(30.0, 0.0, "linear")];
        let (keys, _) = merge_translate(Some(&xs), Some(&ys));
        assert!(values(&keys[1])[0] > 50.0);
    }
}
