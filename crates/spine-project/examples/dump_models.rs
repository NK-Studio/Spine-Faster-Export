//! 진단용: .spine 파일들을 읽어 모델을 비교하기 쉬운 JSON 한 줄씩으로 찍는다(실수는 f64 비트 16진수).
//! 사용: cargo run -p spine-project --release --example dump_models -- a.spine b.spine ...
//! 출력 한 줄: {"file":…, "ok":true, "models":[…]} 또는 {"file":…, "ok":false, "error":…}

use serde_json::{json, Value};
use spine_model::*;

fn h(x: f64) -> Value {
    json!(format!("{:016x}", x.to_bits()))
}

fn hv(v: &Option<Vec<f64>>) -> Value {
    match v {
        Some(v) => Value::Array(v.iter().map(|&x| h(x)).collect()),
        None => Value::Null,
    }
}

fn att(a: &ModelAttachment) -> Value {
    json!({
        "type": a.r#type, "name": a.name, "key": a.key, "path": a.path, "color": a.color,
        "x": h(a.x), "y": h(a.y), "width": h(a.width), "height": h(a.height),
        "vertices": hv(&a.vertices), "uvs": hv(&a.uvs), "triangles": a.triangles, "edges": a.edges, "hull": a.hull,
        "parent": a.parent, "skin": a.skin, "parentSlot": a.parent_slot,
        "sequence": a.sequence.as_ref().map(|s| json!([s.count, s.start, s.digits, s.setup])),
    })
}

fn detail(c: &ModelConstraint) -> Value {
    let m = &c.mixes;
    match c.type_str() {
        "ik" => json!({"bones": c.bones, "target": c.target, "mix": h(c.mix), "softness": h(c.softness), "bendPositive": c.bend_positive,
                        "compress": c.compress, "stretch": c.stretch, "uniform": c.uniform}),
        "transform" => json!({"bones": c.bones, "target": c.target, "rotation": h(c.rotation), "x": h(c.x), "y": h(c.y),
                        "scaleX": h(c.scale_x), "scaleY": h(c.scale_y), "shearY": h(c.shear_y), "local": c.local, "relative": c.relative,
                        "mixRotate": h(m[0]), "mixX": h(m[1]), "mixY": h(m[2]), "mixScaleX": h(m[3]), "mixScaleY": h(m[4]), "mixShearY": h(m[5])}),
        "path" => json!({"bones": c.bones, "target": c.target, "spacingMode": c.spacing_mode, "rotateMode": c.rotate_mode,
                        "positionMode": c.position_mode, "position": h(c.position), "spacing": h(c.spacing), "rotation": h(c.rotation),
                        "mixRotate": h(m[0]), "mixX": h(m[1]), "mixY": h(m[2])}),
        "physics" => json!({"bone": c.bone, "x": h(c.x), "y": h(c.y), "rotate": h(c.rotate), "scaleX": h(c.scale_x), "shearX": h(c.shear_x),
                        "limit": h(c.limit), "fps": h(c.fps), "inertia": h(c.inertia), "strength": h(c.strength), "damping": h(c.damping),
                        "mass": h(c.mass), "wind": h(c.wind), "gravity": h(c.gravity), "mix": h(c.mix),
                        "inertiaGlobal": c.inertia_global, "strengthGlobal": c.strength_global, "dampingGlobal": c.damping_global,
                        "massGlobal": c.mass_global, "windGlobal": c.wind_global, "gravityGlobal": c.gravity_global, "mixGlobal": c.mix_global}),
        "slider" => json!({"animation": c.animation, "bone": c.bone, "property": c.property, "loop": c.r#loop, "local": c.local,
                        "max": h(c.max), "scale": h(c.scale), "mix": h(c.mix), "offset": h(c.offset), "propertyOffset": h(c.property_offset)}),
        _ => Value::Null,
    }
}

fn model(m: &SkeletonModel) -> Value {
    json!({
        "name": m.name, "fps": h(m.fps), "images": m.images, "audio": m.audio, "referenceScale": h(m.reference_scale),
        "bounds": hv(&m.bounds),
        "bones": m.bones.iter().map(|b| json!({
            "name": b.name, "parent": b.parent, "length": h(b.length), "x": h(b.x), "y": h(b.y), "rotation": h(b.rotation),
            "scaleX": h(b.scale_x), "scaleY": h(b.scale_y), "shearX": h(b.shear_x), "shearY": h(b.shear_y),
            "inherit": b.inherit, "color": b.color, "icon": b.icon, "skin": b.skin, "visible": b.visible,
            "editor33": h(b.editor33), "editor34": h(b.editor34),
        })).collect::<Vec<_>>(),
        "slots": m.slots.iter().map(|s| json!({
            "name": s.name, "bone": s.bone, "color": s.color, "dark": s.dark, "attachment": s.attachment,
            "blend": s.blend, "visible": s.visible,
        })).collect::<Vec<_>>(),
        "constraints": m.constraints.iter().map(|c| json!({
            "type": c.r#type, "name": c.name, "order": c.order, "skin": c.skin, "detail": detail(c),
        })).collect::<Vec<_>>(),
        "skins": m.skins.iter().map(|s| json!({
            "name": s.name, "bones": s.bones, "constraints": s.constraints,
            "attachments": s.attachments.iter().map(|e| json!([e.slot, e.key, e.attachment.as_ref().map(att)])).collect::<Vec<_>>(),
        })).collect::<Vec<_>>(),
        "events": m.events.iter().map(|e| json!([e.name, e.int, h(e.float), e.string, e.audio, h(e.volume), h(e.balance)])).collect::<Vec<_>>(),
        "strings": m.strings,
        "animations": m.animations.iter().map(|a| json!({
            "name": a.name, "noClean": a.no_clean, "color": a.color,
            "timelines": a.timelines.iter().map(|t| json!({
                "type": t.r#type, "code": t.code, "bone": t.bone, "slot": t.slot, "constraint": t.constraint,
                "attachment": t.attachment, "skin": t.skin,
                "keys": t.keys.iter().map(|k| json!({
                    "frame": h(k.frame), "curve": k.curve, "values": hv(&k.values),
                    "handles": k.handles.as_ref().map(|hs| hs.iter().map(|x| x.iter().map(|&y| h(y)).collect::<Vec<_>>()).collect::<Vec<_>>()),
                    "name": k.name, "dense": hv(&k.dense),
                    "event": k.event.as_ref().map(|e| json!([e.name, e.int, h(e.float), e.string, h(e.volume), h(e.balance)])),
                    "offsets": k.offsets,
                })).collect::<Vec<_>>(),
            })).collect::<Vec<_>>(),
        })).collect::<Vec<_>>(),
    })
}

fn main() {
    for path in std::env::args().skip(1) {
        let bytes = std::fs::read(&path).expect("파일을 읽을 수 없습니다");
        let t0 = std::time::Instant::now();
        let line = match spine_project::SpineProject::load(&bytes) {
            Err(e) => json!({"file": path, "ok": false, "error": e.to_string()}),
            Ok(p) => {
                let t_load = t0.elapsed();
                let mut models = Vec::new();
                let mut error = None;
                let mut read = std::time::Duration::ZERO;
                for i in 0..p.skeleton_names().len() {
                    let t1 = std::time::Instant::now();
                    let r = p.read_model(i);
                    read += t1.elapsed();
                    match r {
                        Ok(m) => models.push(model(&m)),
                        Err(e) => {
                            error = Some(e.to_string());
                            break;
                        }
                    }
                }
                let t_all = t_load + read;
                json!({"file": path, "ok": error.is_none(), "error": error, "format": p.format_version(),
                       "schema": p.schema_name(), "partial": p.partial_error(),
                       "load_ms": t_load.as_secs_f64() * 1000.0, "total_ms": t_all.as_secs_f64() * 1000.0, "models": models})
            }
        };
        println!("{line}");
    }
}
