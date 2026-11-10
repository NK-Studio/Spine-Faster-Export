//! C# 오라클이 Newtonsoft 로 직렬화한 SkeletonModel(double 은 "d<비트>" 문자열) → Rust 모델.
//! examples/cs_parity.rs 와 tests/cs_regression.rs 가 함께 쓴다.

#![allow(dead_code)]

use serde_json::Value;
use spine_model::*;

fn f(v: &Value) -> f64 {
    match v {
        Value::String(s) if s.starts_with('d') => f64::from_bits(s[1..].parse::<i64>().unwrap() as u64),
        Value::Number(n) => n.as_f64().unwrap(),
        other => panic!("double 아님: {other}"),
    }
}
fn s(v: &Value) -> Option<String> {
    v.as_str().map(str::to_string)
}
fn b(v: &Value) -> bool {
    v.as_bool().unwrap()
}
fn i(v: &Value) -> i64 {
    v.as_i64().unwrap()
}
fn arr(v: &Value) -> &Vec<Value> {
    v.as_array().unwrap()
}
fn fl(v: &Value) -> Option<Vec<f64>> {
    v.as_array().map(|a| a.iter().map(f).collect())
}
fn il(v: &Value) -> Option<Vec<i64>> {
    v.as_array().map(|a| a.iter().map(i).collect())
}
fn sl(v: &Value) -> Vec<String> {
    arr(v).iter().map(|x| x.as_str().unwrap().to_string()).collect()
}

fn bone(v: &Value) -> ModelBone {
    ModelBone {
        name: s(&v["Name"]),
        parent: s(&v["Parent"]),
        length: f(&v["Length"]),
        x: f(&v["X"]),
        y: f(&v["Y"]),
        rotation: f(&v["Rotation"]),
        scale_x: f(&v["ScaleX"]),
        scale_y: f(&v["ScaleY"]),
        shear_x: f(&v["ShearX"]),
        shear_y: f(&v["ShearY"]),
        inherit: s(&v["Inherit"]),
        color: s(&v["Color"]),
        icon: s(&v["Icon"]),
        skin: b(&v["Skin"]),
        ..Default::default()
    }
}

fn slot(v: &Value) -> ModelSlot {
    ModelSlot {
        name: s(&v["Name"]),
        bone: s(&v["Bone"]),
        color: s(&v["Color"]),
        dark: s(&v["Dark"]),
        attachment: s(&v["Attachment"]),
        blend: s(&v["Blend"]),
        visible: b(&v["Visible"]),
    }
}

fn constraint(v: &Value) -> ModelConstraint {
    let mut mixes = [0.0; 6];
    for (k, x) in arr(&v["Mixes"]).iter().enumerate() {
        mixes[k] = f(x);
    }
    ModelConstraint {
        r#type: s(&v["Type"]),
        name: s(&v["Name"]),
        order: i(&v["Order"]) as i32,
        skin: b(&v["Skin"]),
        bones: sl(&v["Bones"]),
        target: s(&v["Target"]),
        mixes,
        mix: f(&v["Mix"]),
        softness: f(&v["Softness"]),
        bend_positive: b(&v["BendPositive"]),
        compress: b(&v["Compress"]),
        stretch: b(&v["Stretch"]),
        uniform: b(&v["Uniform"]),
        rotation: f(&v["Rotation"]),
        x: f(&v["X"]),
        y: f(&v["Y"]),
        scale_x: f(&v["ScaleX"]),
        scale_y: f(&v["ScaleY"]),
        shear_y: f(&v["ShearY"]),
        local: b(&v["Local"]),
        relative: b(&v["Relative"]),
        properties: v["Properties"].as_array().map(|a| {
            a.iter()
                .map(|p| ModelFromProperty {
                    name: s(&p["Name"]),
                    offset: f(&p["Offset"]),
                    to: arr(&p["To"])
                        .iter()
                        .map(|t| ModelToProperty {
                            name: s(&t["Name"]),
                            offset: f(&t["Offset"]),
                            max: f(&t["Max"]),
                            scale: f(&t["Scale"]),
                        })
                        .collect(),
                })
                .collect()
        }),
        spacing_mode: s(&v["SpacingMode"]),
        rotate_mode: s(&v["RotateMode"]),
        position_mode: s(&v["PositionMode"]),
        position: f(&v["Position"]),
        spacing: f(&v["Spacing"]),
        bone: s(&v["Bone"]),
        rotate: f(&v["Rotate"]),
        shear_x: f(&v["ShearX"]),
        limit: f(&v["Limit"]),
        fps: f(&v["Fps"]),
        inertia: f(&v["Inertia"]),
        strength: f(&v["Strength"]),
        damping: f(&v["Damping"]),
        mass: f(&v["Mass"]),
        wind: f(&v["Wind"]),
        gravity: f(&v["Gravity"]),
        inertia_global: b(&v["InertiaGlobal"]),
        strength_global: b(&v["StrengthGlobal"]),
        damping_global: b(&v["DampingGlobal"]),
        mass_global: b(&v["MassGlobal"]),
        wind_global: b(&v["WindGlobal"]),
        gravity_global: b(&v["GravityGlobal"]),
        mix_global: b(&v["MixGlobal"]),
        animation: s(&v["Animation"]),
        property: s(&v["Property"]),
        r#loop: b(&v["Loop"]),
        max: f(&v["Max"]),
        scale: f(&v["Scale"]),
        offset: f(&v["Offset"]),
        ..Default::default()
    }
}

fn attachment(v: &Value) -> Option<ModelAttachment> {
    if v.is_null() {
        return None;
    }
    Some(ModelAttachment {
        name: s(&v["Name"]),
        key: s(&v["Key"]),
        r#type: s(&v["Type"]),
        path: s(&v["Path"]),
        color: s(&v["Color"]),
        sequence: if v["Sequence"].is_null() {
            None
        } else {
            let q = &v["Sequence"];
            Some(ModelSequence { count: i(&q["Count"]), start: i(&q["Start"]), digits: i(&q["Digits"]), setup: i(&q["Setup"]) })
        },
        x: f(&v["X"]),
        y: f(&v["Y"]),
        scale_x: f(&v["ScaleX"]),
        scale_y: f(&v["ScaleY"]),
        rotation: f(&v["Rotation"]),
        width: f(&v["Width"]),
        height: f(&v["Height"]),
        vertices: fl(&v["Vertices"]),
        weighted: b(&v["Weighted"]),
        dense_map: il(&v["DenseMap"]).map(|l| l.into_iter().map(|x| x as i32).collect()),
        setup: fl(&v["Setup"]),
        vertex_count: i(&v["VertexCount"]) as i32,
        closed: b(&v["Closed"]),
        constant_speed: b(&v["ConstantSpeed"]),
        lengths: fl(&v["Lengths"]),
        end: s(&v["End"]),
        convex: b(&v["Convex"]),
        parent: s(&v["Parent"]),
        skin: s(&v["Skin"]),
        timelines: b(&v["Timelines"]),
        uvs: fl(&v["Uvs"]),
        triangles: il(&v["Triangles"]),
        edges: il(&v["Edges"]),
        hull: i(&v["Hull"]) as i32,
        ..Default::default()
    })
}

fn key(v: &Value) -> ModelKey {
    ModelKey {
        frame: f(&v["Frame"]),
        curve: s(&v["Curve"]),
        values: fl(&v["Values"]),
        handles: v["Handles"].as_array().map(|a| {
            a.iter()
                .map(|h| {
                    let h = arr(h);
                    [f(&h[0]), f(&h[1]), f(&h[2]), f(&h[3])]
                })
                .collect()
        }),
        name: s(&v["Name"]),
        dense: fl(&v["Dense"]),
        event: if v["Event"].is_null() {
            None
        } else {
            let e = &v["Event"];
            Some(ModelKeyEvent {
                name: s(&e["Name"]),
                int: i(&e["Int"]),
                float: f(&e["Float"]),
                string: s(&e["String"]),
                volume: f(&e["Volume"]),
                balance: f(&e["Balance"]),
            })
        },
        offsets: v["Offsets"]
            .as_array()
            .map(|a| a.iter().map(|o| (o["Key"].as_str().unwrap().to_string(), i(&o["Value"]))).collect()),
    }
}

pub fn model(v: &Value) -> SkeletonModel {
    SkeletonModel {
        name: s(&v["Name"]),
        format: s(&v["Format"]),
        fps: f(&v["Fps"]),
        images: s(&v["Images"]),
        audio: s(&v["Audio"]),
        reference_scale: f(&v["ReferenceScale"]),
        bounds: fl(&v["Bounds"]),
        bones: arr(&v["Bones"]).iter().map(bone).collect(),
        slots: arr(&v["Slots"]).iter().map(slot).collect(),
        constraints: arr(&v["Constraints"]).iter().map(constraint).collect(),
        skins: arr(&v["Skins"])
            .iter()
            .map(|k| ModelSkin {
                name: s(&k["Name"]),
                bones: sl(&k["Bones"]),
                constraints: sl(&k["Constraints"]),
                attachments: arr(&k["Attachments"])
                    .iter()
                    .map(|e| ModelSkinEntry { slot: s(&e["Slot"]), key: s(&e["Key"]), attachment: attachment(&e["Attachment"]) })
                    .collect(),
                ..Default::default()
            })
            .collect(),
        events: arr(&v["Events"])
            .iter()
            .map(|e| ModelEvent {
                name: s(&e["Name"]),
                int: i(&e["Int"]),
                float: f(&e["Float"]),
                string: s(&e["String"]),
                audio: s(&e["Audio"]),
                volume: f(&e["Volume"]),
                balance: f(&e["Balance"]),
            })
            .collect(),
        animations: arr(&v["Animations"])
            .iter()
            .map(|a| ModelAnimation {
                name: s(&a["Name"]),
                timelines: arr(&a["Timelines"])
                    .iter()
                    .map(|t| ModelTimeline {
                        r#type: s(&t["Type"]),
                        code: i(&t["Code"]) as i32,
                        bone: s(&t["Bone"]),
                        slot: s(&t["Slot"]),
                        constraint: s(&t["Constraint"]),
                        attachment: s(&t["Attachment"]),
                        skin: s(&t["Skin"]),
                        keys: arr(&t["Keys"]).iter().map(key).collect(),
                    })
                    .collect(),
                ..Default::default()
            })
            .collect(),
        ..Default::default()
    }
}

