//! 손으로 만든 모델로 시험한다.

use super::*;
use spine_model::{ModelAttachment, ModelSequence, ModelSkin, ModelSkinEntry};
use std::path::PathBuf;

fn att(kind: &str, name: &str, path: &str) -> ModelSkinEntry {
    ModelSkinEntry {
        slot: Some("s".into()),
        key: Some(name.into()),
        attachment: Some(ModelAttachment {
            name: Some(name.into()),
            r#type: Some(kind.into()),
            path: Some(path.into()),
            ..Default::default()
        }),
    }
}

fn model(entries: Vec<ModelSkinEntry>) -> SkeletonModel {
    SkeletonModel {
        skins: vec![ModelSkin { name: Some("default".into()), attachments: entries, ..Default::default() }],
        ..Default::default()
    }
}

fn temp_dir(tag: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("spine-atlas-test-{}-{}", tag, std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

/// w×h, 테두리 b 픽셀은 투명, 안쪽은 (seed 기반 색, 알파 128~255).
fn image(w: usize, h: usize, b: usize, seed: u8) -> Vec<u8> {
    let mut v = vec![0u8; w * h * 4];
    for y in 0..h {
        for x in 0..w {
            let o = (y * w + x) * 4;
            if x >= b && y >= b && x + b < w && y + b < h {
                v[o] = seed.wrapping_add(x as u8);
                v[o + 1] = seed.wrapping_mul(3).wrapping_add(y as u8);
                v[o + 2] = 200;
                v[o + 3] = 128 + ((x + y) % 128) as u8;
            } else {
                v[o] = 50; // 투명 픽셀에도 RGB 가 있다(블리드·에일리어스 시험용)
                v[o + 1] = 60;
                v[o + 2] = 70;
            }
        }
    }
    v
}

fn write(dir: &Path, name: &str, w: u32, h: u32, rgba: &[u8]) {
    let p = dir.join(format!("{}.png", name));
    std::fs::create_dir_all(p.parent().unwrap()).unwrap();
    std::fs::write(p, png_codec::encode(w, h, rgba)).unwrap();
}

#[test]
fn names_and_sequences() {
    let mut seq = att("region", "fx", "fx/flare");
    seq.attachment.as_mut().unwrap().sequence = Some(ModelSequence { count: 3, start: 9, digits: 2, setup: 0 });
    let mut by_key = att("region", "", "");
    by_key.key = Some("keyname".into());
    let m = model(vec![
        att("mesh", "body", ""),
        att("region", "head", "parts/head"),
        att("boundingbox", "bb", ""),
        att("linkedmesh", "arm10", ""),
        att("region", "arm2", ""),
        seq,
        by_key,
    ]);
    let (names, mesh) = region_names(&m);
    assert_eq!(
        names,
        vec!["arm2", "arm10", "body", "fx/flare09", "fx/flare10", "fx/flare11", "keyname", "parts/head"]
    );
    assert!(mesh.contains("body") && mesh.contains("arm10") && !mesh.contains("arm2"));
    assert_eq!(zfill(-5, 3), "-05");
    assert_eq!(zfill(123, 2), "123");
}

#[test]
fn pack_strip_rotate_pma() {
    let dir = temp_dir("basic");
    let a = image(40, 20, 3, 1);
    let b = image(10, 30, 0, 2);
    let c = image(16, 16, 2, 3);
    write(&dir, "a", 40, 20, &a);
    write(&dir, "sub/b", 10, 30, &b);
    write(&dir, "c", 16, 16, &c);
    let m = model(vec![att("region", "a", ""), att("region", "b", "sub/b"), att("mesh", "c", ""), att("region", "nope", "")]);
    let res = pack_detailed(&m, &dir, "t", &AtlasSettings::default()).unwrap();
    assert_eq!(res.missing, vec!["nope"]);
    let out = res.output;
    assert_eq!(out.pages.len(), 1);
    let text = &out.atlas_text;
    assert!(text.starts_with("t.png\nsize:"), "{}", text);
    assert!(text.contains("filter:Linear,Linear\npma:true\na\nbounds:"));
    assert!(text.contains("offsets:3,3,40,20"));
    assert!(text.contains("nope\nbounds:"));
    assert!(text.ends_with('\n') && !text.ends_with("\n\n"));

    // 페이지에서 영역을 다시 꺼내 원본(잘라낸 뒤 PMA)과 비교한다
    let page = &out.pages[0];
    let lines: Vec<&str> = text.lines().collect();
    for (name, src, w, strip) in [("a", &a, 40usize, 3usize), ("sub/b", &b, 10, 0), ("c", &c, 16, 0)] {
        let at = lines.iter().position(|l| *l == name).unwrap();
        let nums: Vec<usize> = lines[at + 1]["bounds:".len()..].split(',').map(|v| v.parse().unwrap()).collect();
        let rot = lines.get(at + 2).is_some_and(|l| *l == "rotate:90")
            || lines.get(at + 3).is_some_and(|l| *l == "rotate:90");
        let (bx, by, bw, bh) = (nums[0], nums[1], nums[2], nums[3]);
        for y in 0..bh {
            for x in 0..bw {
                let s = ((y + strip) * w + x + strip) * 4;
                let a = src[s + 3] as u32;
                let want = [
                    (src[s] as u32 * a / 255) as u8,
                    (src[s + 1] as u32 * a / 255) as u8,
                    (src[s + 2] as u32 * a / 255) as u8,
                    a as u8,
                ];
                let (px, py) = if rot { (bx + y, by + bw - 1 - x) } else { (bx + x, by + y) };
                let d = (py * page.width as usize + px) * 4;
                assert_eq!(&page.rgba[d..d + 4], &want, "{} ({},{})", name, x, y);
            }
        }
    }
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn alias_bleed_no_pma_and_pages() {
    let dir = temp_dir("opts");
    let a = image(30, 30, 4, 7);
    let mut a2 = a.clone();
    a2[0] = 255; // 완전 투명 픽셀의 RGB 만 다르다 → 같은 그림
    write(&dir, "a", 30, 30, &a);
    write(&dir, "a2", 30, 30, &a2);
    for i in 0..6 {
        write(&dir, &format!("big{}", i), 60, 60, &image(60, 60, 0, i));
    }
    let mut entries = vec![att("region", "a", ""), att("region", "a2", "")];
    for i in 0..6 {
        entries.push(att("region", &format!("big{}", i), ""));
    }
    let m = model(entries);
    let s = AtlasSettings { pma: false, bleed: true, alias: true, ..AtlasSettings::default() }.with_max_size(128);
    let out = pack(&m, &dir, "p", &s).unwrap();
    assert!(out.pages.len() > 1);
    assert!(!out.atlas_text.contains("pma:true"));
    assert!(out.atlas_text.contains("\n\np_2.png\n"));
    assert_eq!(out.pages[0].name, "p.png");
    let lines: Vec<&str> = out.atlas_text.lines().collect();
    let ia = lines.iter().position(|l| *l == "a").unwrap();
    let ia2 = lines.iter().position(|l| *l == "a2").unwrap();
    assert_eq!(lines[ia + 1], lines[ia2 + 1]);
    for p in &out.pages {
        assert!(p.width <= 128 && p.height <= 128);
    }
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn bleed_fills_two_rings() {
    // 5×1: 가운데만 보인다 → 양옆 2칸씩 채워진다
    let mut px = vec![0u8; 5 * 4];
    px[8..12].copy_from_slice(&[90, 30, 60, 255]);
    let out = bleed_rgb(5, 1, px, 2);
    assert_eq!(&out[0..3], &[90, 30, 60]);
    assert_eq!(&out[4..7], &[90, 30, 60]);
    assert_eq!(out[3], 0);
}

#[test]
fn errors() {
    let dir = temp_dir("err");
    assert!(pack(&model(vec![]), &dir, "x", &AtlasSettings::default()).is_err());
    assert!(pack(&model(vec![att("region", "a", "")]), &dir.join("none"), "x", &AtlasSettings::default()).is_err());
    let _ = std::fs::remove_dir_all(&dir);
}

/// atlas 텍스트에서 (이름, x, y, w, h, 회전) 을 뽑는다.
fn rects(text: &str) -> Vec<(String, i32, i32, i32, i32, bool)> {
    let lines: Vec<&str> = text.lines().collect();
    let mut out = Vec::new();
    for (i, l) in lines.iter().enumerate() {
        if let Some(b) = l.strip_prefix("bounds:") {
            let v: Vec<i32> = b.split(',').map(|n| n.parse().unwrap()).collect();
            let mut j = i;
            while lines[j - 1].contains(':') {
                j -= 1;
            }
            let rot = lines.get(i + 1..).unwrap_or(&[]).iter().take_while(|n| n.contains(':')).any(|n| *n == "rotate:90");
            let (w, h) = if rot { (v[3], v[2]) } else { (v[2], v[3]) };
            out.push((lines[j - 1].to_string(), v[0], v[1], w, h, rot));
        }
    }
    out
}

fn many(dir: &Path) -> SkeletonModel {
    let mut entries = Vec::new();
    for i in 0..12 {
        let (w, h) = (8 + (i * 7) % 30, 6 + (i * 11) % 25);
        write(dir, &format!("img_{i}"), w as u32, h as u32, &image(w, h, 0, i as u8));
        entries.push(att("region", &format!("img_{i}"), &format!("img_{i}")));
    }
    model(entries)
}

#[test]
fn padding_pot_square_no_rotation() {
    let dir = temp_dir("padpot");
    let m = many(&dir);
    let s = AtlasSettings { padding_x: 4, padding_y: 1, rotation: false, pot: true, square: true, min_width: 64, ..AtlasSettings::default() };
    let out = pack(&m, &dir, "o", &s).unwrap();
    assert_eq!(out.pages.len(), 1);
    let p = &out.pages[0];
    assert!(p.width.is_power_of_two() && p.width == p.height && p.width >= 64);
    let rs = rects(&out.atlas_text);
    assert_eq!(rs.len(), 12);
    assert!(rs.iter().all(|r| !r.5));
    for (i, a) in rs.iter().enumerate() {
        // 가장자리 여백
        assert!(a.1 >= 4 && a.2 >= 1 && a.1 + a.3 + 4 <= p.width as i32 && a.2 + a.4 + 1 <= p.height as i32);
        for b in &rs[i + 1..] {
            // 여백을 포함해 겹치지 않는다
            let apart = a.1 + a.3 + 4 <= b.1 || b.1 + b.3 + 4 <= a.1 || a.2 + a.4 + 1 <= b.2 || b.2 + b.4 + 1 <= a.2;
            assert!(apart, "{a:?} {b:?}");
        }
    }
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn grid_names_runtime_lines() {
    let dir = temp_dir("grid");
    let m = many(&dir);
    let s = AtlasSettings {
        grid: true,
        edge_padding: false,
        use_indexes: true,
        filter_min: TextureFilter::MipMapLinearLinear,
        filter_mag: TextureFilter::Nearest,
        wrap_x: TextureWrap::Repeat,
        format: PixelFormat::RGBA4444,
        pma: false,
        ..AtlasSettings::default()
    }
    .with_max_size(128);
    let out = pack(&m, &dir, "g", &s).unwrap();
    let t = &out.atlas_text;
    assert!(t.contains("format:RGBA4444\nfilter:MipMapLinearLinear,Nearest\nrepeat:x\n"), "{t}");
    assert!(t.contains("img\nindex:0\nbounds:"));
    assert!(t.contains("img\nindex:11\nbounds:"));
    // 격자: 위치가 칸 크기의 배수
    let rs = rects(t);
    let xs: HashSet<i32> = rs.iter().map(|r| r.1).collect();
    assert!(xs.contains(&0));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn blank_flatten_scale_debug_duplicate() {
    let dir = temp_dir("misc");
    write(&dir, "deep/a", 20, 20, &image(20, 20, 2, 5));
    write(&dir, "blank", 8, 8, &vec![0u8; 8 * 8 * 4]);
    let m = model(vec![att("region", "a", "deep/a"), att("region", "blank", "")]);
    let s = AtlasSettings {
        ignore_blank_images: true,
        flatten_paths: true,
        debug: true,
        duplicate_padding: true,
        pma: false,
        scale: vec![1.0, 0.5],
        scale_suffix: vec![String::new(), "@0.5x".into()],
        ..AtlasSettings::default()
    };
    let all = pack_scales(&m, &dir, "m", &s).unwrap();
    assert_eq!(all.len(), 2);
    let (sfx, half) = &all[1];
    assert_eq!(sfx, "@0.5x");
    assert_eq!(half.output.pages[0].name, "m@0.5x.png");
    assert_eq!(half.ignored, vec!["blank".to_string()]);
    let full = &all[0].1.output;
    assert!(full.atlas_text.contains("\na\nbounds:"));
    assert!(!full.atlas_text.contains("blank"));
    // 0.5배: 원본 크기가 절반
    assert!(half.output.atlas_text.contains(",10,10\n"), "{}", half.output.atlas_text);
    // debug: 영역 테두리가 분홍
    let p = &full.pages[0];
    let r = &rects(&full.atlas_text)[0];
    let o = ((r.2 as usize) * p.width as usize + r.1 as usize) * 4;
    assert_eq!(&p.rgba[o..o + 4], &[255, 0, 255, 255]);
    // 같은 접미사 두 번은 막는다
    let bad = AtlasSettings { scale: vec![1.0, 2.0], scale_suffix: vec![], ..AtlasSettings::default() };
    assert!(pack_scales(&m, &dir, "m", &bad).is_err());
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn duplicate_padding_copies_edges() {
    let mut out = vec![0u8; 6 * 4 * 4];
    let px = |x: usize, y: usize| (y * 6 + x) * 4;
    out[px(2, 1)..px(2, 1) + 4].copy_from_slice(&[9, 8, 7, 255]);
    out[px(3, 2)..px(3, 2) + 4].copy_from_slice(&[1, 2, 3, 255]);
    duplicate_edges(&mut out, 6, 4, (2, 1, 2, 2), 1, 1);
    assert_eq!(&out[px(1, 1)..px(1, 1) + 4], &[9, 8, 7, 255]); // 왼쪽
    assert_eq!(&out[px(1, 0)..px(1, 0) + 4], &[9, 8, 7, 255]); // 왼쪽 위 모서리
    assert_eq!(&out[px(4, 3)..px(4, 3) + 4], &[1, 2, 3, 255]); // 오른쪽 아래 모서리
}

#[test]
fn spine_pack_json() {
    // Spine 이 저장하는 pack.json(모르는 키는 무시)
    let j = r#"{"stripWhitespaceX":false,"stripWhitespaceY":true,"rotation":false,"alias":true,"ignoreBlankImages":true,
      "alphaThreshold":0,"paddingX":2,"paddingY":2,"edgePadding":true,"duplicatePadding":false,"minWidth":16,"minHeight":16,
      "maxWidth":1024,"maxHeight":1024,"pot":true,"square":true,"filterMin":"Linear","filterMag":"Linear","wrapX":"ClampToEdge",
      "wrapY":"ClampToEdge","format":"RGBA8888","outputFormat":"png","premultiplyAlpha":true,"bleed":false,"scale":[1],
      "scaleSuffix":[""],"scaleResampling":["bicubic"],"atlasExtension":".atlas.txt","combineSubdirectories":false,
      "flattenPaths":false,"useIndexes":false,"fast":false,"debug":false,"multipleOfFour":false,"jpegQuality":0.9,"prettyPrint":true}"#;
    let s: AtlasSettings = serde_json::from_str(j).unwrap();
    assert!(!s.strip_whitespace_x && s.pot && s.square && s.alias && s.max_width == 1024 && s.atlas_extension == ".atlas.txt");
    assert_eq!(s.bleed_iterations, 2);
    let back = serde_json::to_string(&s).unwrap();
    assert!(back.contains("\"premultiplyAlpha\":true") && back.contains("\"maxWidth\":1024"));
}
