//! 파이썬 PoC 모델(dump.py 가 덤프한 JSON, 실수는 "d:<비트>")을 읽어 `.skel` 을 쓴다 — 파이썬 출력과 바이트 비교용.
//!
//! 사용법: `py_parity <model.json> <출력 디렉터리> <접두사>` →
//! `<접두사>_<버전>_<ness>_<clean>.rs.skel` (또는 `.rs.err`)

use serde_json::Value;
use spine_model::*;

fn num(v: &Value) -> f64 {
    match v {
        Value::String(s) if s.starts_with("d:") => f64::from_bits(s[2..].parse::<u64>().unwrap()),
        Value::Number(n) => n.as_f64().unwrap(),
        Value::Bool(b) => *b as i64 as f64,
        Value::Null => 0.0,
        _ => panic!("숫자가 아님: {v}"),
    }
}

fn f(o: &Value, k: &str) -> f64 {
    o.get(k).map(num).unwrap_or(0.0)
}

fn i(o: &Value, k: &str) -> i64 {
    f(o, k) as i64
}

fn b(o: &Value, k: &str) -> bool {
    match o.get(k) {
        Some(Value::Bool(x)) => *x,
        Some(Value::Null) | None => false,
        Some(v) => num(v) != 0.0,
    }
}

fn st(o: &Value, k: &str) -> Option<String> {
    o.get(k).and_then(|v| v.as_str()).map(|s| s.to_string())
}

fn sv(v: &Value) -> Option<String> {
    v.as_str().map(|s| s.to_string())
}

fn fl(o: &Value, k: &str) -> Option<Vec<f64>> {
    o.get(k)
        .and_then(|v| v.as_array())
        .map(|a| a.iter().map(num).collect())
}

fn il(o: &Value, k: &str) -> Option<Vec<i64>> {
    o.get(k)
        .and_then(|v| v.as_array())
        .map(|a| a.iter().map(|x| num(x) as i64).collect())
}

fn strs(o: &Value, k: &str) -> Vec<String> {
    o.get(k)
        .and_then(|v| v.as_array())
        .map(|a| a.iter().filter_map(sv).collect())
        .unwrap_or_default()
}

fn arr<'a>(o: &'a Value, k: &str) -> &'a [Value] {
    o.get(k)
        .and_then(|v| v.as_array())
        .map(|v| v.as_slice())
        .unwrap_or(&[])
}

fn attachment(a: &Value) -> ModelAttachment {
    ModelAttachment {
        name: st(a, "name"),
        key: st(a, "key"),
        r#type: st(a, "type"),
        path: st(a, "path"),
        color: st(a, "color"),
        sequence: a
            .get("sequence")
            .filter(|q| !q.is_null())
            .map(|q| ModelSequence {
                count: i(q, "count"),
                start: i(q, "start"),
                digits: i(q, "digits"),
                setup: i(q, "setup"),
            }),
        x: f(a, "x"),
        y: f(a, "y"),
        scale_x: f(a, "scaleX"),
        scale_y: f(a, "scaleY"),
        rotation: f(a, "rotation"),
        width: f(a, "width"),
        height: f(a, "height"),
        vertices: fl(a, "vertices"),
        weighted: b(a, "weighted"),
        dense_map: il(a, "dense_map").map(|v| v.into_iter().map(|x| x as i32).collect()),
        setup: fl(a, "setup"),
        vertex_count: i(a, "vertexCount") as i32,
        closed: b(a, "closed"),
        constant_speed: b(a, "constantSpeed"),
        lengths: fl(a, "lengths"),
        end: st(a, "end"),
        convex: b(a, "convex"),
        parent: st(a, "parent"),
        parent_slot: st(a, "parentSlot"),
        skin: st(a, "skin"),
        timelines: b(a, "timelines"),
        uvs: fl(a, "uvs"),
        triangles: il(a, "triangles"),
        edges: il(a, "edges"),
        hull: i(a, "hull") as i32,
    }
}

fn key(k: &Value) -> ModelKey {
    ModelKey {
        frame: f(k, "frame"),
        curve: st(k, "curve"),
        values: fl(k, "values"),
        handles: k.get("handles").and_then(|v| v.as_array()).map(|hs| {
            hs.iter()
                .map(|h| {
                    let h: Vec<f64> = h.as_array().unwrap().iter().map(num).collect();
                    [h[0], h[1], h[2], h[3]]
                })
                .collect()
        }),
        name: st(k, "name"),
        dense: fl(k, "dense"),
        event: k
            .get("event")
            .filter(|e| !e.is_null())
            .map(|e| ModelKeyEvent {
                name: st(e, "name"),
                int: i(e, "int"),
                float: f(e, "float"),
                string: st(e, "string"),
                volume: f(e, "volume"),
                balance: f(e, "balance"),
            }),
        offsets: k.get("offsets").and_then(|v| v.as_array()).map(|os| {
            os.iter()
                .map(|o| {
                    let o = o.as_array().unwrap();
                    (o[0].as_str().unwrap_or("").to_string(), num(&o[1]) as i64)
                })
                .collect()
        }),
        ..Default::default()
    }
}

fn model(m: &Value) -> SkeletonModel {
    SkeletonModel {
        name: st(m, "name"),
        format: st(m, "format"),
        fps: f(m, "fps"),
        images: st(m, "images"),
        audio: st(m, "audio"),
        reference_scale: f(m, "referenceScale"),
        bounds: fl(m, "bounds"),
        strings: strs(m, "strings"),
        bones: arr(m, "bones")
            .iter()
            .map(|x| ModelBone {
                name: st(x, "name"),
                parent: st(x, "parent"),
                length: f(x, "length"),
                x: f(x, "x"),
                y: f(x, "y"),
                rotation: f(x, "rotation"),
                scale_x: f(x, "scaleX"),
                scale_y: f(x, "scaleY"),
                shear_x: f(x, "shearX"),
                shear_y: f(x, "shearY"),
                inherit: st(x, "inherit"),
                color: st(x, "color"),
                icon: st(x, "icon"),
                skin: b(x, "skin"),
                visible: b(x, "visible"),
                editor33: f(x, "editor33"),
                editor34: f(x, "editor34"),
            })
            .collect(),
        slots: arr(m, "slots")
            .iter()
            .map(|x| ModelSlot {
                name: st(x, "name"),
                bone: st(x, "bone"),
                color: st(x, "color"),
                dark: st(x, "dark"),
                attachment: st(x, "attachment"),
                blend: st(x, "blend"),
                visible: b(x, "visible"),
            })
            .collect(),
        constraints: arr(m, "constraints")
            .iter()
            .map(|c| ModelConstraint {
                r#type: st(c, "type"),
                name: st(c, "name"),
                order: i(c, "order") as i32,
                skin: b(c, "skin"),
                bones: strs(c, "bones"),
                target: st(c, "target"),
                mixes: [
                    f(c, "mixRotate"),
                    f(c, "mixX"),
                    f(c, "mixY"),
                    f(c, "mixScaleX"),
                    f(c, "mixScaleY"),
                    f(c, "mixShearY"),
                ],
                mix: f(c, "mix"),
                softness: f(c, "softness"),
                bend_positive: b(c, "bendPositive"),
                compress: b(c, "compress"),
                stretch: b(c, "stretch"),
                uniform: b(c, "uniform"),
                rotation: f(c, "rotation"),
                x: f(c, "x"),
                y: f(c, "y"),
                scale_x: f(c, "scaleX"),
                scale_y: f(c, "scaleY"),
                shear_y: f(c, "shearY"),
                local: b(c, "local"),
                relative: b(c, "relative"),
                properties: c.get("properties").and_then(|v| v.as_array()).map(|ps| {
                    ps.iter()
                        .map(|p| {
                            let p = p.as_array().unwrap();
                            ModelFromProperty {
                                name: sv(&p[0]),
                                offset: num(&p[1]),
                                to: p[2]
                                    .as_array()
                                    .unwrap()
                                    .iter()
                                    .map(|t| {
                                        let t = t.as_array().unwrap();
                                        ModelToProperty {
                                            name: sv(&t[0]),
                                            offset: num(&t[1]),
                                            max: num(&t[2]),
                                            scale: num(&t[3]),
                                        }
                                    })
                                    .collect(),
                            }
                        })
                        .collect()
                }),
                spacing_mode: st(c, "spacingMode"),
                rotate_mode: st(c, "rotateMode"),
                position_mode: st(c, "positionMode"),
                position: f(c, "position"),
                spacing: f(c, "spacing"),
                bone: st(c, "bone"),
                rotate: f(c, "rotate"),
                shear_x: f(c, "shearX"),
                limit: f(c, "limit"),
                fps: f(c, "fps"),
                inertia: f(c, "inertia"),
                strength: f(c, "strength"),
                damping: f(c, "damping"),
                mass: f(c, "mass"),
                wind: f(c, "wind"),
                gravity: f(c, "gravity"),
                inertia_global: b(c, "inertiaGlobal"),
                strength_global: b(c, "strengthGlobal"),
                damping_global: b(c, "dampingGlobal"),
                mass_global: b(c, "massGlobal"),
                wind_global: b(c, "windGlobal"),
                gravity_global: b(c, "gravityGlobal"),
                mix_global: b(c, "mixGlobal"),
                animation: st(c, "animation"),
                property: st(c, "property"),
                r#loop: b(c, "loop"),
                max: f(c, "max"),
                scale: f(c, "scale"),
                offset: f(c, "offset"),
                property_offset: f(c, "propertyOffset"),
            })
            .collect(),
        skins: arr(m, "skins")
            .iter()
            .map(|x| ModelSkin {
                name: st(x, "name"),
                bones: strs(x, "bones"),
                constraints: strs(x, "constraints"),
                color: st(x, "color"),
                attachments: arr(x, "attachments")
                    .iter()
                    .map(|r| {
                        let r = r.as_array().unwrap();
                        ModelSkinEntry {
                            slot: sv(&r[0]),
                            key: sv(&r[1]),
                            attachment: Some(attachment(&r[2])),
                        }
                    })
                    .collect(),
            })
            .collect(),
        events: arr(m, "events")
            .iter()
            .map(|e| ModelEvent {
                name: st(e, "name"),
                int: i(e, "int"),
                float: f(e, "float"),
                string: st(e, "string"),
                audio: st(e, "audio"),
                volume: f(e, "volume"),
                balance: f(e, "balance"),
            })
            .collect(),
        animations: arr(m, "animations")
            .iter()
            .map(|an| ModelAnimation {
                name: st(an, "name"),
                color: st(an, "color"),
                no_clean: b(an, "noClean"),
                timelines: arr(an, "timelines")
                    .iter()
                    .map(|t| ModelTimeline {
                        r#type: st(t, "type"),
                        code: i(t, "code") as i32,
                        bone: st(t, "bone"),
                        slot: st(t, "slot"),
                        constraint: st(t, "constraint"),
                        attachment: st(t, "attachment"),
                        skin: st(t, "skin"),
                        keys: arr(t, "keys").iter().map(key).collect(),
                    })
                    .collect(),
            })
            .collect(),
    }
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let text = std::fs::read_to_string(&args[1]).unwrap();
    let v: Value = serde_json::from_str(&text).unwrap();
    let base = model(&v);
    let mut cleaned = base.clone();
    spine_binary::clean_model(&mut cleaned);
    for (clean, m) in [(0, &base), (1, &cleaned)] {
        for ver in ["4.0.64", "4.1.24", "4.2.43", "4.3.26"] {
            for ness in [false, true] {
                let name = format!("{}/{}_{}_{}_{}", args[2], args[3], ver, ness as i32, clean);
                match spine_binary::write_binary(m, ver, ness) {
                    Ok(d) => std::fs::write(format!("{name}.rs.skel"), d).unwrap(),
                    Err(e) => std::fs::write(format!("{name}.rs.err"), e.to_string()).unwrap(),
                }
            }
        }
    }
}
