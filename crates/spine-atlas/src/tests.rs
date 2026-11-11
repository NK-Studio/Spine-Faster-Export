//! 손으로 만든 모델로 시험한다.

use super::*;
use spine_model::{AtlasOutput, ModelAttachment, ModelSequence, ModelSkin, ModelSkinEntry};
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

/// 배율마다 고른 리샘플링 방식을 쓴다(값은 Spine 4.2 명령줄 패커의 결과).
#[test]
fn scale_resampling_per_scale() {
    let dir = temp_dir("resample");
    // 8x1: 칸마다 다른 불투명 색(R 에 칸 번호)
    let px: Vec<u8> = (0..8u8).flat_map(|x| [x * 30, 0, 0, 255]).collect();
    write(&dir, "row", 8, 1, &px);
    let m = model(vec![att("region", "row", "")]);
    let base = AtlasSettings { strip_whitespace_x: false, strip_whitespace_y: false, rotation: false, pma: false, ..AtlasSettings::default() };
    let s = AtlasSettings {
        scale: vec![0.75, 0.75, 2.0],
        scale_suffix: vec!["@n".into(), "@b".into(), "@a".into()],
        scale_resampling: vec!["nearest".into(), "bilinear".into()], // 세 번째는 비었으니 bicubic
        ..base.clone()
    };
    assert_eq!(
        s.scale_list().iter().map(|x| x.2).collect::<Vec<_>>(),
        vec![Resampling::Nearest, Resampling::Bilinear, Resampling::Area]
    );
    let all = pack_scales(&m, &dir, "m", &s).unwrap();
    let row = |i: usize| {
        let out = &all[i].1.output;
        let r = &rects(&out.atlas_text)[0];
        let p = &out.pages[0];
        (0..r.3 as usize).map(|x| p.rgba[((r.2 as usize) * p.width as usize + r.1 as usize + x) * 4]).collect::<Vec<u8>>()
    };
    // nearest 8→6: Spine 이 고른 원본 칸 0 1 3 4 5 7
    assert_eq!(row(0), [0, 1, 3, 4, 5, 7].map(|x| x * 30));
    // bilinear 은 이웃 칸을 섞는다
    assert!(row(1).iter().any(|v| v % 30 != 0), "{:?}", row(1));
    // bicubic(넓이 평균) 정수 배율 확대는 픽셀 복제
    assert_eq!(row(2), (0..16u8).map(|x| x / 2 * 30).collect::<Vec<_>>());
    // pack.json 의 이름
    let j: AtlasSettings = serde_json::from_str(r#"{"scale":[0.5,2],"scaleSuffix":["@h","@d"],"scaleResampling":["auto","bilinearProgressive"]}"#).unwrap();
    assert_eq!(j.scale_list().iter().map(|x| x.2).collect::<Vec<_>>(), vec![Resampling::Area, Resampling::Bilinear]);
    assert!(serde_json::to_string(&j).unwrap().contains(r#""scaleResampling":["auto","bilinearProgressive"]"#));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn duplicate_padding_copies_edges() {
    let mut out = vec![0u8; 6 * 4 * 4];
    let px = |x: usize, y: usize| (y * 6 + x) * 4;
    out[px(2, 1)..px(2, 1) + 4].copy_from_slice(&[9, 8, 7, 255]);
    out[px(3, 2)..px(3, 2) + 4].copy_from_slice(&[1, 2, 3, 255]);
    duplicate_edges(&mut out, 6, 4, (2, 1, 2, 2), 1, 1, None);
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
    assert_eq!((s.output_format.as_str(), s.jpeg_quality), ("png", 0.9));
    let back = serde_json::to_string(&s).unwrap();
    assert!(back.contains("\"premultiplyAlpha\":true") && back.contains("\"maxWidth\":1024"));
}

fn mesh_att(name: &str, uvs: &[f64], triangles: &[i64]) -> ModelSkinEntry {
    let mut e = att("mesh", name, "");
    let a = e.attachment.as_mut().unwrap();
    a.uvs = Some(uvs.to_vec());
    a.triangles = Some(triangles.to_vec());
    a.hull = (uvs.len() / 2) as i32;
    e
}

/// 영역의 원본 좌표 (x, y)(잘라내기 전)가 페이지의 어디인가. 잘려 나간 곳이면 None.
fn page_spot(out: &AtlasOutput, name: &str, x: i32, y: i32) -> Option<(i32, i32)> {
    let lines: Vec<&str> = out.atlas_text.lines().collect();
    let at = lines.iter().position(|l| *l == name).unwrap();
    let field = |key: &str| -> Option<Vec<i32>> {
        lines[at + 1..].iter().take_while(|l| l.contains(':')).find_map(|l| l.strip_prefix(key)).map(|v| v.split(',').map(|n| n.parse().unwrap()).collect())
    };
    let b = field("bounds:").unwrap();
    let (left, top) = match field("offsets:") {
        Some(o) => (o[0], o[3] - o[1] - b[3]),
        None => (0, 0),
    };
    let deg = field("rotate:").map_or(0, |v| v[0]);
    let (lx, ly) = (x - left, y - top);
    if lx < 0 || ly < 0 || lx >= b[2] || ly >= b[3] {
        return None;
    }
    // 런타임(MeshAttachment.updateRegion)이 rotate 를 읽는 규칙
    Some(match deg {
        90 => (b[0] + ly, b[1] + b[2] - 1 - lx),
        180 => (b[0] + b[2] - 1 - lx, b[1] + b[3] - 1 - ly),
        270 => (b[0] + b[3] - 1 - ly, b[1] + lx),
        _ => (b[0] + lx, b[1] + ly),
    })
}

/// 페이지에서 영역의 원본 좌표 (x, y)(잘라내기 전) 픽셀을 꺼낸다.
fn page_pixel(out: &AtlasOutput, name: &str, x: i32, y: i32) -> [u8; 4] {
    let (px, py) = page_spot(out, name, x, y).unwrap_or_else(|| panic!("{name} ({x},{y}) 가 영역 밖"));
    let page = &out.pages[0];
    let d = (py as usize * page.width as usize + px as usize) * 4;
    page.rgba[d..d + 4].try_into().unwrap()
}

#[test]
fn polygons_pack_mesh_shapes() {
    let dir = temp_dir("poly");
    let n = 100usize;
    let imgs: Vec<Vec<u8>> = (0..5).map(|i| image(n, n, 0, 10 + i as u8)).collect();
    for (i, px) in imgs.iter().enumerate() {
        write(&dir, &format!("t{i}"), n as u32, n as u32, px);
    }
    // t0·t2: 왼쪽 위 삼각형, t1: 오른쪽 아래 삼각형, t3: 왼쪽 위 삼각형이지만 영역 첨부도 쓴다 → 사각형, t4: 영역 첨부
    let ul = [0.0, 0.0, 1.0, 0.0, 0.0, 1.0];
    let lr = [1.0, 0.0, 1.0, 1.0, 0.0, 1.0];
    let shapes = [Some(ul), Some(lr), Some(ul), None, None];
    let m = model(vec![
        mesh_att("t0", &ul, &[0, 1, 2]),
        mesh_att("t1", &lr, &[0, 1, 2]),
        mesh_att("t2", &ul, &[0, 1, 2]),
        mesh_att("t3", &ul, &[0, 1, 2]),
        att("region", "t3b", "t3"),
        att("region", "t4", ""),
    ]);
    let rect = AtlasSettings { pma: false, ..AtlasSettings::default() };
    let a = pack(&m, &dir, "r", &rect).unwrap();
    let area = |o: &AtlasOutput| o.pages.iter().map(|p| p.width as u64 * p.height as u64).sum::<u64>();

    for duplicate_padding in [false, true] {
        let poly = AtlasSettings { packing: "polygons".into(), duplicate_padding, ..rect.clone() };
        let b = pack(&m, &dir, "p", &poly).unwrap();
        assert_eq!(b.pages.len(), 1);
        assert!(area(&b) * 10 < area(&a) * 9, "다각형 {} · 직사각형 {}", area(&b), area(&a));
        let page = &b.pages[0];
        let (pw, ph) = (page.width as i32, page.height as i32);

        // 메시가 걸친 픽셀(사각형 영역은 전체)은 그대로 있고, 영역끼리 겹치지 않는다
        let hulls: Vec<Vec<u8>> = shapes.iter().map(|t| t.and_then(|t| polygon::raster_tris(n, n, &[t])).unwrap_or_else(|| vec![1; n * n])).collect();
        let mut label = vec![0u8; (pw * ph) as usize];
        for (i, hull) in hulls.iter().enumerate() {
            let name = format!("t{i}");
            for y in 0..n {
                for x in (0..n).filter(|&x| hull[y * n + x] != 0) {
                    let s = (y * n + x) * 4;
                    assert_eq!(page_pixel(&b, &name, x as i32, y as i32), imgs[i][s..s + 4], "{name} ({x},{y})");
                    let (px, py) = page_spot(&b, &name, x as i32, y as i32).unwrap();
                    let at = &mut label[(py * pw + px) as usize];
                    assert_eq!(*at, 0, "{name} ({x},{y}) 가 다른 영역과 겹친다");
                    *at = i as u8 + 1;
                }
            }
        }

        // 여백(2): 사각형끼리는 빈 픽셀 2줄 이상, 모양이 낀 쌍은 1줄 이상
        for y in 0..ph {
            for x in 0..pw {
                let me = label[(y * pw + x) as usize];
                if me == 0 {
                    continue;
                }
                for ny in (y - 2).max(0)..=(y + 2).min(ph - 1) {
                    for nx in (x - 2).max(0)..=(x + 2).min(pw - 1) {
                        let other = label[(ny * pw + nx) as usize];
                        if other == 0 || other == me {
                            continue;
                        }
                        let shape = shapes[me as usize - 1].is_some() || shapes[other as usize - 1].is_some();
                        let far = (nx - x).abs().max((ny - y).abs()) == 2;
                        assert!(shape && far, "영역 {me} ({x},{y}) 와 영역 {other} ({nx},{ny}) 가 여백보다 가깝다");
                    }
                }
            }
        }

        // 모양 바깥 한 줄(선형 필터가 읽는 줄)은 제 픽셀이거나 비어 있다: 남의 픽셀(다른 모양의 바깥 줄 · duplicate padding)이 아니다
        let mut own = 0;
        for (i, hull) in hulls.iter().enumerate().filter(|(i, _)| shapes[*i].is_some()) {
            let wide = polygon::widen(hull, n, n, 1);
            for y in 0..n {
                for x in (0..n).filter(|&x| wide[y * n + x] != 0 && hull[y * n + x] == 0) {
                    let got = page_pixel(&b, &format!("t{i}"), x as i32, y as i32);
                    let s = (y * n + x) * 4;
                    if got == imgs[i][s..s + 4] {
                        own += 1;
                    } else {
                        assert_eq!(got, [0; 4], "t{i} ({x},{y}) 바깥 줄에 남의 픽셀");
                    }
                }
            }
        }
        assert!(own > 0);

        // 두 영역의 바깥 한 줄이 만나는 빈 픽셀은 비어 있어야 한다(둘 다 읽으므로 어느 쪽 픽셀도 두지 않는다)
        let mut shared = 0;
        for y in 0..ph {
            for x in 0..pw {
                if label[(y * pw + x) as usize] != 0 {
                    continue;
                }
                let mut near: Vec<u8> = Vec::new();
                for ny in (y - 1).max(0)..=(y + 1).min(ph - 1) {
                    for nx in (x - 1).max(0)..=(x + 1).min(pw - 1) {
                        let l = label[(ny * pw + nx) as usize];
                        if l != 0 && !near.contains(&l) {
                            near.push(l);
                        }
                    }
                }
                if near.len() >= 2 {
                    shared += 1;
                    let d = ((y * pw + x) * 4) as usize;
                    assert_eq!(page.rgba[d..d + 4], [0; 4], "({x},{y}) 는 영역 {near:?} 가 함께 읽는 픽셀인데 비어 있지 않다");
                }
            }
        }
        assert!(shared > 0, "모양이 빈 픽셀 한 줄까지 붙은 곳이 없다");
    }

    // 모르는 packing 이름은 직사각형과 같다
    let other = pack(&m, &dir, "r", &AtlasSettings { packing: "rectangles".into(), ..rect.clone() }).unwrap();
    assert_eq!(other.atlas_text, a.atlas_text);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn polygon_shapes_from_model() {
    let quad = [0.0, 0.0, 1.0, 0.0, 1.0, 1.0, 0.0, 1.0];
    let mut linked = att("linkedmesh", "child", "img/child");
    linked.attachment.as_mut().unwrap().parent = Some("parent".into());
    let mut lost = att("linkedmesh", "lost", "img/lost");
    lost.attachment.as_mut().unwrap().parent = Some("nope".into());
    let m = model(vec![
        mesh_att("parent", &quad, &[0, 1, 2, 2, 3, 0]),
        linked,
        lost,
        mesh_att("both", &quad, &[0, 1, 2]),
        att("region", "both2", "both"),
        mesh_att("bad", &quad, &[0, 1, 9]),
    ]);
    let shapes = mesh_shapes(&m);
    assert_eq!(shapes["parent"].len(), 2);
    assert_eq!(shapes["img/child"], shapes["parent"]);
    assert!(!shapes.contains_key("img/lost") && !shapes.contains_key("both") && !shapes.contains_key("bad"));

    // 이미지 전체를 덮는 메시는 사각형으로, 일부만 덮으면 모양으로
    assert!(polygon::raster_tris(8, 8, &shapes["parent"]).is_none());
    let half = polygon::raster_tris(8, 8, &[[0.0, 0.0, 0.5, 0.0, 0.0, 0.5]]).unwrap();
    assert_eq!(polygon::mask_bounds(&half, 8, 8), Some((0, 0, 3, 3)));
    assert!(half[0] == 1 && half[3 * 8 + 3] == 0 && half[4] == 0);
}

/// 메시 범위는 Spine 처럼 f32 로, y 는 아래에서 재서 구한다(Cat 프로젝트의 UV 값과 Spine 4.2 가 자른 범위).
#[test]
fn mesh_crop_matches_spine() {
    let f = |bits: u32| f32::from_bits(bits) as f64;
    // u0 ≤ u ≤ u1, v0 ≤ v ≤ v1 를 덮는 사각형(삼각형 둘)
    let quad = |u0: f64, u1: f64, v0: f64, v1: f64| vec![[u0, v0, u1, v0, u1, v1], [u0, v0, u1, v1, u0, v1]];
    // 3017: u 최대 0.98571432 × 70 은 f64 로는 69 를 조금 넘지만 f32 로는 69 → 오른쪽 끝 68(포함)
    let neck17 = quad(f(0x3c6a0ea1), f(0x3f7c57c6), 0.0, 1.0);
    let m = polygon::raster_tris_scaled(70, 54, None, &neck17).unwrap();
    assert_eq!(polygon::mask_bounds(&m, 70, 54), Some((1, 0, 68, 53)));
    // 3033: v 최대 0.95833337 은 아래에서 재면 (1 − v)·48 = 1.9999981 → 아래 끝 46(포함)
    let neck33 = quad(0.0, 1.0, f(0x3d2aaaab), f(0x3f755556));
    let m = polygon::raster_tris_scaled(46, 48, None, &neck33).unwrap();
    assert_eq!(polygon::mask_bounds(&m, 46, 48), Some((0, 2, 45, 46)));
    let m = polygon::raster_tris_scaled(23, 24, Some((23.0, 24.0)), &neck33).unwrap();
    assert_eq!(polygon::mask_bounds(&m, 23, 24), Some((0, 1, 22, 23)));
    // 배율 크기가 반올림되면(140×106 의 1.6배 = 224×169.6 → 170) 위쪽 경계는 반올림 전 높이로 아래에서 잰다
    let body = quad(0.0, 1.0, 0.004526, 0.991143);
    let m = polygon::raster_tris_scaled(224, 170, Some((224.0, 169.6)), &body).unwrap();
    assert_eq!(polygon::mask_bounds(&m, 224, 170).map(|b| (b.1, b.3)), Some((1, 168)));
    let m = polygon::raster_tris_scaled(308, 233, Some((308.0, 233.2)), &body).unwrap();
    assert_eq!(polygon::mask_bounds(&m, 308, 233).map(|b| (b.1, b.3)), Some((0, 230)));
}

/// spine-csharp 3.8 `Atlas.Load` 를 그대로 옮긴 파서: 키 이름을 보지 않고 줄을 정해진 순서로만 읽는다.
/// (페이지 이름, 폭, 높이, [(영역 이름, rotate 값, x, y, w, h, orig w, orig h, offset x, offset y, index)])
#[allow(clippy::type_complexity)]
fn parse_like_38(text: &str) -> Vec<(String, i32, i32, Vec<(String, String, i32, i32, i32, i32, i32, i32, i32, i32, i32)>)> {
    const FORMATS: [&str; 7] = ["Alpha", "Intensity", "LuminanceAlpha", "RGB565", "RGBA4444", "RGB888", "RGBA8888"];
    const FILTERS: [&str; 7] = ["Nearest", "Linear", "MipMap", "MipMapNearestNearest", "MipMapLinearNearest", "MipMapNearestLinear", "MipMapLinearLinear"];
    fn tuple(line: &str) -> Vec<String> {
        let colon = line.find(':').unwrap_or_else(|| panic!("Invalid line: {line}"));
        let mut out = Vec::new();
        let mut last = colon + 1;
        for _ in 0..3 {
            let Some(comma) = line[last..].find(',') else { break };
            out.push(line[last..last + comma].trim().to_string());
            last += comma + 1;
        }
        out.push(line[last..].trim().to_string());
        out
    }
    let value = |line: &str| line[line.find(':').unwrap_or_else(|| panic!("Invalid line: {line}")) + 1..].trim().to_string();
    let int = |s: &str| s.parse::<i32>().unwrap_or_else(|_| panic!("숫자가 아님: {s}"));
    let mut lines = text.lines();
    let mut pages = Vec::new();
    let mut in_page = false;
    while let Some(line) = lines.next() {
        if line.trim().is_empty() {
            in_page = false;
        } else if !in_page {
            let mut t = tuple(lines.next().unwrap());
            assert_eq!(t.len(), 2, "size 줄이 아님");
            let (w, h) = (int(&t[0]), int(&t[1]));
            t = tuple(lines.next().unwrap());
            assert!(FORMATS.contains(&t[0].as_str()), "format 이 아님: {}", t[0]);
            t = tuple(lines.next().unwrap());
            assert!(t.len() == 2 && FILTERS.contains(&t[0].as_str()) && FILTERS.contains(&t[1].as_str()), "filter 가 아님: {t:?}");
            let repeat = value(lines.next().unwrap());
            assert!(["x", "y", "xy", "none"].contains(&repeat.as_str()), "repeat 가 아님: {repeat}");
            pages.push((line.to_string(), w, h, Vec::new()));
            in_page = true;
        } else {
            let rotate = value(lines.next().unwrap());
            assert!(rotate == "true" || rotate == "false" || rotate.parse::<i32>().is_ok(), "rotate 가 아님: {rotate}");
            let xy = tuple(lines.next().unwrap());
            let size = tuple(lines.next().unwrap());
            let orig = tuple(lines.next().unwrap());
            assert_eq!(orig.len(), 2, "split 은 쓰지 않는다");
            let offset = tuple(lines.next().unwrap());
            let index = int(&value(lines.next().unwrap()));
            pages.last_mut().unwrap().3.push((
                line.to_string(),
                rotate,
                int(&xy[0]),
                int(&xy[1]),
                int(&size[0]),
                int(&size[1]),
                int(&orig[0]),
                int(&orig[1]),
                int(&offset[0]),
                int(&offset[1]),
                index,
            ));
        }
    }
    pages
}

#[test]
fn pretty_print_like_spine() {
    // Spine 4.2 의 Pretty print 글자: 머리말·영역 줄을 탭으로 들이고 `: ` · `, ` 로 띄운다. 레거시 출력에는 쓰지 않는다.
    let dir = temp_dir("pretty");
    let m = many(&dir);
    let s = AtlasSettings {
        use_indexes: true,
        filter_mag: TextureFilter::MipMap,
        wrap_x: TextureWrap::Repeat,
        format: PixelFormat::RGBA4444,
        pretty_print: true,
        ..AtlasSettings::default()
    }
    .with_max_size(128);
    let plain = pack(&m, &dir, "g", &AtlasSettings { pretty_print: false, ..s.clone() }).unwrap().atlas_text;
    let t = pack(&m, &dir, "g", &s).unwrap().atlas_text;
    assert!(t.starts_with("g.png\n\tsize: "), "{t}");
    assert!(t.contains("\n\tformat: RGBA4444\n\tfilter: Linear, MipMap\n\trepeat: x\n\tpma: true\n"), "{t}");
    assert!(t.contains("\nimg\n\tindex: 0\n\tbounds: "), "{t}");
    // 글자만 다르고 값은 같다
    let squash = |v: &str| v.replace(['\t', ' '], "");
    assert_eq!(squash(&t), squash(&plain));
    let legacy = AtlasSettings { legacy_output: true, ..s.clone() };
    let lt = pack(&m, &dir, "g", &legacy).unwrap().atlas_text;
    assert_eq!(lt, pack(&m, &dir, "g", &AtlasSettings { pretty_print: false, ..legacy }).unwrap().atlas_text);
    assert!(!lt.contains('\t'));
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn legacy_output_reads_like_38() {
    let dir = temp_dir("legacy");
    let mut entries = Vec::new();
    let mut sources: HashMap<String, (usize, usize, usize, Vec<u8>)> = HashMap::new();
    for i in 0..10usize {
        let (w, h, b) = (40 + (i * 13) % 50, 30 + (i * 7) % 40, i % 3);
        let px = image(w, h, b, i as u8 * 9);
        write(&dir, &format!("part_{i}"), w as u32, h as u32, &px);
        entries.push(att("region", &format!("part_{i}"), &format!("part_{i}")));
        sources.insert(format!("part_{i}"), (w, h, b, px));
    }
    let m = model(entries);
    let base = AtlasSettings { pma: false, use_indexes: true, wrap_x: TextureWrap::Repeat, ..AtlasSettings::default() }.with_max_size(128);
    let new = pack(&m, &dir, "l", &base).unwrap();
    let old = pack(&m, &dir, "l", &AtlasSettings { legacy_output: true, ..base.clone() }).unwrap();
    assert!(old.pages.len() > 1, "여러 페이지를 시험해야 한다");
    // 그림은 같고 글만 다르다
    assert_eq!(old.pages.iter().map(|p| &p.rgba).collect::<Vec<_>>(), new.pages.iter().map(|p| &p.rgba).collect::<Vec<_>>());

    let pages = parse_like_38(&old.atlas_text);
    assert_eq!(pages.len(), old.pages.len());
    let mut seen = 0;
    for (p, (name, w, h, regions)) in pages.iter().enumerate() {
        let page = &old.pages[p];
        assert_eq!((name.as_str(), *w as u32, *h as u32), (page.name.as_str(), page.width, page.height));
        for (rname, rotate, x, y, rw, rh, ow, oh, ox, oy, index) in regions {
            // use_indexes: part_3 → 이름 part, index 3
            assert_eq!(rname, "part");
            let (sw, sh, b, px) = &sources[&format!("part_{index}")];
            assert_eq!((*ow as usize, *oh as usize), (*sw, *sh));
            assert_eq!((*rw as usize, *rh as usize), (sw - 2 * b, sh - 2 * b));
            // offset 은 왼쪽 · 아래 기준
            assert_eq!((*ox as usize, *oy as usize), (*b, *b));
            let rot = rotate == "true";
            assert!(rot || rotate == "false", "사각형 영역은 90도만 돈다: {rotate}");
            let top = *oh - *oy - *rh;
            for ly in 0..*rh {
                for lx in 0..*rw {
                    let s = (((ly + top) * *ow + lx + *ox) * 4) as usize;
                    let (gx, gy) = if rot { (x + ly, y + rw - 1 - lx) } else { (x + lx, y + ly) };
                    let d = ((gy * *w + gx) * 4) as usize;
                    assert_eq!(page.rgba[d..d + 4], px[s..s + 4], "part_{index} ({lx},{ly})");
                }
            }
            seen += 1;
        }
    }
    assert_eq!(seen, 10);
    // Spine 3.8 편집기의 글자: 머리말의 쉼표 뒤에 빈칸이 없다. 나머지는 같고, 3.8 파서는 둘 다 같은 값으로 읽는다
    let tight = pack(&m, &dir, "l", &AtlasSettings { legacy_output: true, legacy_editor38: true, ..base.clone() }).unwrap();
    let first = &old.pages[0];
    assert!(old.atlas_text.contains(&format!("size: {}, {}\n", first.width, first.height)) && old.atlas_text.contains("filter: Linear, Linear\n"));
    assert!(tight.atlas_text.contains(&format!("size: {},{}\n", first.width, first.height)) && tight.atlas_text.contains("filter: Linear,Linear\n"));
    assert_eq!(tight.atlas_text.replace(",", ", ").replace(",  ", ", "), old.atlas_text);
    assert_eq!(parse_like_38(&tight.atlas_text), pages);
    // 새 형식은 3.8 파서로 읽히지 않는다(머리말의 filter 줄을 format 으로 읽다가 멈춘다)
    assert!(std::panic::catch_unwind(|| parse_like_38(&new.atlas_text)).is_err());
    let _ = std::fs::remove_dir_all(&dir);
}

/// atlas 텍스트 → [(페이지 이름, 머리말 줄(size 빼고), 영역 이름)].
fn page_layout(text: &str) -> Vec<(String, Vec<String>, Vec<String>)> {
    let mut pages: Vec<(String, Vec<String>, Vec<String>)> = Vec::new();
    let mut fresh = true;
    for line in text.lines() {
        if line.trim().is_empty() {
            fresh = true;
        } else if fresh {
            pages.push((line.to_string(), Vec::new(), Vec::new()));
            fresh = false;
        } else if let Some(p) = pages.last_mut() {
            let is_key = line.contains(':') && !line.starts_with(' ');
            if is_key && p.2.is_empty() {
                if !line.starts_with("size:") {
                    p.1.push(line.to_string());
                }
            } else if !line.starts_with(' ') && !["bounds:", "offsets:", "rotate:", "index:"].iter().any(|k| line.starts_with(k)) {
                p.2.push(line.to_string());
            }
        }
    }
    pages
}

fn folder_tree(tag: &str) -> PathBuf {
    let dir = temp_dir(tag);
    let px = image(12, 10, 1, 9);
    for n in ["root", "a/x", "a/c/y", "a/d/z", "b/w", "a b/v", "e/skip", "e/f/skip2"] {
        write(&dir, n, 12, 10, &px);
    }
    std::fs::write(dir.join("a/pack.json"), r#"{"filterMin":"Nearest","filterMag":"Nearest","paddingX":8}"#).unwrap();
    std::fs::write(dir.join("a/c/pack.json"), r#"{"format":"RGBA4444"}"#).unwrap();
    std::fs::write(dir.join("e/pack.json"), r#"{"ignore":true}"#).unwrap();
    dir
}

/// Spine 4.2.43 명령줄 패커와 대조한 규칙: 폴더 안 pack.json 은 부모 설정을 물려받아 덮고, 설정이 있는 폴더는 따로 페이지를 쓰며,
/// 페이지 머리말은 그 폴더 설정을 따른다. 폴더 순서는 깊이 우선(이름 Ordinal 순).
#[test]
fn folder_pack_json() {
    let dir = folder_tree("folderpack");
    let m = SkeletonModel::default();
    let base = AtlasSettings { pma: false, rotation: false, ..Default::default() };
    let run = |combine: bool| {
        let s = AtlasSettings { combine_subdirectories: combine, ..base.clone() };
        let sources = folder_sources(&m, &dir, &s).unwrap();
        assert!(sources.iter().all(|src| !src.name.starts_with("e/")), "ignore 폴더는 빠진다");
        let mut out = pack_sources_scales(&sources, "t", &s).unwrap();
        assert_eq!(out.len(), 1);
        out.remove(0).1.output.atlas_text
    };
    let names = |l: &[(String, Vec<String>, Vec<String>)]| l.iter().map(|p| p.2.clone()).collect::<Vec<_>>();
    let lin = vec!["filter:Linear,Linear".to_string()];

    // 합치기를 끄면 폴더마다 페이지: 이미지 폴더 자신, a, a/c, a/d, "a b", b (a 의 하위 폴더가 "a b" 보다 먼저)
    let text = run(false);
    let l = page_layout(&text);
    assert_eq!(names(&l), [vec!["root"], vec!["a/x"], vec!["a/c/y"], vec!["a/d/z"], vec!["a b/v"], vec!["b/w"]], "{text}");
    assert_eq!(l[0].1, lin);
    assert!(l[1].1.is_empty(), "Nearest 면 filter 줄이 없다: {text}");
    assert_eq!(l[2].1, ["format:RGBA4444"], "a/c 는 a 의 Nearest 를 물려받는다: {text}");
    assert!(l[3].1.is_empty(), "pack.json 없는 a/d 는 a 의 설정: {text}");
    assert_eq!(l[4].1, lin);
    // 여백 8 은 a 와 그 아래에만
    let rect = |n: &str| rects(&text).into_iter().find(|r| r.0 == n).unwrap();
    assert_eq!((rect("a/x").1, rect("a/c/y").1, rect("b/w").1), (8, 8, 2));

    // 합치기를 켜면 pack.json 이 없는 하위 폴더는 부모 페이지에, 있는 폴더(a, a/c)는 따로
    let text = run(true);
    let l = page_layout(&text);
    assert_eq!(names(&l), [vec!["a b/v", "b/w", "root"], vec!["a/d/z", "a/x"], vec!["a/c/y"]], "{text}");
    assert_eq!(l[0].1, lin);
    assert!(l[1].1.is_empty());
    assert_eq!(l[2].1, ["format:RGBA4444"]);
    assert_eq!(l.iter().map(|p| p.0.as_str()).collect::<Vec<_>>(), ["t.png", "t_2.png", "t_3.png"]);

    // pack.json 이 합치기를 켜고 끌 수도 있다(그 폴더 아래에만)
    std::fs::write(dir.join("a/pack.json"), r#"{"combineSubdirectories":true}"#).unwrap();
    let l = page_layout(&run(false));
    assert_eq!(names(&l), [vec!["root"], vec!["a/d/z", "a/x"], vec!["a/c/y"], vec!["a b/v"], vec!["b/w"]]);

    // 이미지 폴더 자신의 pack.json 도 준 설정 위에 덮는다
    std::fs::write(dir.join("pack.json"), r#"{"wrapX":"Repeat"}"#).unwrap();
    let l = page_layout(&run(false));
    assert!(l.iter().all(|p| p.1.contains(&"repeat:x".to_string())), "{l:?}");
    let _ = std::fs::remove_dir_all(&dir);
}

/// 폴더마다 배율이 다르면 그 폴더는 자기 접미사의 atlas 에만 들어간다(Spine 도 같다). Legacy output 도 페이지마다.
#[test]
fn folder_pack_json_scales_and_legacy() {
    let dir = temp_dir("folderscale");
    let px = image(20, 16, 0, 3);
    for n in ["root", "half/h", "old/o"] {
        write(&dir, n, 20, 16, &px);
    }
    std::fs::write(dir.join("half/pack.json"), r#"{"scale":[1,0.5],"scaleSuffix":["","@h"]}"#).unwrap();
    std::fs::write(dir.join("old/pack.json"), r#"{"legacyOutput":true}"#).unwrap();
    let s = AtlasSettings { pma: false, ..Default::default() };
    let sources = folder_sources(&SkeletonModel::default(), &dir, &s).unwrap();
    let out = pack_sources_scales(&sources, "t", &s).unwrap();
    assert_eq!(out.iter().map(|o| o.0.as_str()).collect::<Vec<_>>(), ["", "@h"]);
    let full = &out[0].1.output.atlas_text;
    let l = page_layout(full);
    assert_eq!(l.iter().map(|p| p.2.clone()).collect::<Vec<_>>(), [vec!["root"], vec!["half/h"], vec!["old/o"]], "{full}");
    assert!(full.contains("bounds:2,2,20,16\n\nt_3.png\nsize: 24, 20\nformat: RGBA8888\nfilter: Linear, Linear\nrepeat: none\nold/o\n  rotate: false\n"), "{full}");
    let half = &out[1].1.output.atlas_text;
    assert!(half.starts_with("t@h.png\nsize:14,12\nfilter:Linear,Linear\nscale:0.5\nhalf/h\n"), "{half}");
    assert_eq!(out[1].1.output.pages.len(), 1);
    let _ = std::fs::remove_dir_all(&dir);
}

fn decode_jpeg(bytes: &[u8]) -> (usize, usize, Vec<u8>) {
    let mut d = zune_jpeg::JpegDecoder::new(zune_jpeg::zune_core::bytestream::ZCursor::new(bytes));
    let px = d.decode().unwrap();
    let (w, h) = d.dimensions().unwrap();
    (w, h, px)
}

/// JPEG 의 양자화 표(DQT 두 개, 지그재그 순서 그대로).
fn dqt(bytes: &[u8]) -> Vec<Vec<u8>> {
    let mut v = Vec::new();
    let mut i = 2;
    while i + 4 <= bytes.len() && bytes[i] == 0xFF {
        let len = u16::from_be_bytes([bytes[i + 2], bytes[i + 3]]) as usize;
        if bytes[i + 1] == 0xDB {
            v.push(bytes[i + 5..i + 5 + 64].to_vec());
        }
        if bytes[i + 1] == 0xDA {
            break;
        }
        i += 2 + len;
    }
    v
}

#[test]
fn jpg_output_flattens_on_black() {
    // Spine 4.2 명령줄 패커로 확인한 것: JPG 는 PMA · 블리드 설정과 상관없이 같은 파일(검정 위에 얹은 RGB)이고,
    // atlas 는 `이름.jpg` 와 설정 그대로의 `format:` · `pma:` 줄을 쓴다.
    let dir = temp_dir("jpg");
    let a = image(40, 20, 3, 1);
    write(&dir, "a", 40, 20, &a);
    for i in 0..4 {
        write(&dir, &format!("big{i}"), 60, 60, &image(60, 60, 0, i));
    }
    let mut entries = vec![att("region", "a", "")];
    entries.extend((0..4).map(|i| att("region", &format!("big{i}"), "")));
    let m = model(entries);
    let jpg = AtlasSettings { output_format: "jpg".into(), ..AtlasSettings::default() }.with_max_size(128);
    let pma = pack(&m, &dir, "j", &jpg).unwrap();
    let straight = pack(&m, &dir, "j", &AtlasSettings { pma: false, ..jpg.clone() }).unwrap();
    let bleed = pack(&m, &dir, "j", &AtlasSettings { pma: false, bleed: true, format: PixelFormat::RGBA4444, ..jpg.clone() }).unwrap();
    assert!(pma.pages.len() > 1);
    assert!(pma.atlas_text.starts_with("j.jpg\nsize:") && pma.atlas_text.contains("\n\nj_2.jpg\n"), "{}", pma.atlas_text);
    assert_eq!(pma.pages[1].name, "j_2.jpg");
    assert!(pma.atlas_text.contains("pma:true") && !straight.atlas_text.contains("pma:"));
    assert!(bleed.atlas_text.contains("format:RGBA4444\n"));
    for ((p, s), b) in pma.pages.iter().zip(&straight.pages).zip(&bleed.pages) {
        assert_eq!(p.rgba, s.rgba);
        assert_eq!(p.rgba, b.rgba);
        assert!(p.rgba.chunks_exact(4).all(|px| px[3] == 255));
    }
    // 영역 a 의 픽셀 = 미리 곱한 RGB(버림), 영역 밖 투명 픽셀은 검정
    let png = pack(&m, &dir, "j", &AtlasSettings::default().with_max_size(128)).unwrap();
    for (j, p) in pma.pages.iter().zip(&png.pages) {
        for (x, y) in j.rgba.chunks_exact(4).zip(p.rgba.chunks_exact(4)) {
            assert_eq!(&x[..3], &y[..3]);
        }
    }
    assert_eq!(&pma.pages[0].rgba[..4], &[0, 0, 0, 255]);

    // 인코딩: 크기 그대로 풀리고, 풀어 낸 그림이 원본과 가깝다
    let bytes = encode_page(&pma.pages[0], &jpg).unwrap();
    assert_eq!(&bytes[..4], &[0xFF, 0xD8, 0xFF, 0xE0]);
    assert_eq!(&bytes[6..11], b"JFIF\0");
    let (w, h, px) = decode_jpeg(&bytes);
    assert_eq!((w as u32, h as u32), (pma.pages[0].width, pma.pages[0].height));
    let err: u64 = px.chunks_exact(3).zip(pma.pages[0].rgba.chunks_exact(4)).map(|(d, s)| (0..3).map(|c| d[c].abs_diff(s[c]) as u64).sum::<u64>()).sum();
    assert!((err as f64 / (w * h * 3) as f64) < 3.0, "평균 오차 {}", err as f64 / (w * h * 3) as f64);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn merge_pack_json_inherits() {
    let base = AtlasSettings { padding_x: 7, grid: true, ..Default::default() }.normalize();
    let s = merge_pack_json(&base, r#"{"format":"RGB565","unknownKey":1}"#).unwrap();
    assert_eq!((s.padding_x, s.grid, s.format), (7, true, PixelFormat::RGB565));
    assert!(!merge_pack_json(&base, r#"{"packing":"rectangles"}"#).unwrap().grid);
    assert!(!merge_pack_json(&base, r#"{"grid":false}"#).unwrap().grid);
    let poly = merge_pack_json(&AtlasSettings::default(), r#"{"packing":"polygons"}"#).unwrap();
    assert!(poly.polygons());
    assert!(merge_pack_json(&poly, r#"{"ignore":true}"#).unwrap().polygons());
    assert!(merge_pack_json(&base, "[1]").is_err());
    assert!(merge_pack_json(&base, r#"{"paddingX":"x"}"#).is_err());
}

/// atlas 안 영역 순서: Spine 4.2.43 처럼 이름 Ordinal 순(대소문자 구분), 같은 이름은 index 오름차순, index 없는 것은 뒤에.
#[test]
fn region_order_is_ordinal() {
    let dir = temp_dir("ordinal");
    let names = ["B_1", "Line_root", "bird", "fr", "fr_1", "fr_10", "fr_2", "x10", "x9", "레이어 3", "레이어_10"];
    for (i, n) in names.iter().enumerate() {
        // 파일 이름은 NFD 로(macOS 가 돌려주는 모양): 영역 이름은 NFC 로 나와야 한다
        let nfd: String = unicode_normalization::UnicodeNormalization::nfd(*n).collect();
        write(&dir, &nfd, 6, 6, &image(6, 6, 0, i as u8 * 20));
    }
    let s = AtlasSettings { use_indexes: true, pma: false, ..Default::default() };
    let sources = folder_sources(&SkeletonModel::default(), &dir, &s).unwrap();
    let text = pack_sources_scales(&sources, "t", &s).unwrap().remove(0).1.output.atlas_text;
    let order: Vec<&str> = text.lines().skip(3).filter(|l| !l.starts_with("bounds:")).collect();
    assert_eq!(
        order,
        ["B", "index:1", "Line_root", "bird", "fr", "index:1", "fr", "index:2", "fr", "index:10", "fr", "x10", "x9", "레이어", "index:10", "레이어 3"],
        "{text}"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn jpeg_quality_tables_like_spine() {
    // Spine 4.2 가 jpegQuality 0.9 · 0.5 · 0.33 으로 쓴 JPG 의 DQT 첫 여덟 값(휘도, 색차)
    let page = AtlasPage { name: "x.jpg".into(), width: 17, height: 9, rgba: vec![255; 17 * 9 * 4] };
    for (q, luma, chroma) in [
        (0.9, [3, 2, 2, 3, 2, 2, 3, 3], [3, 4, 4, 5, 4, 5, 9, 5]),
        (0.5, [16, 11, 12, 14, 12, 10, 16, 14], [17, 18, 18, 24, 21, 24, 47, 26]),
        (0.33, [24, 17, 18, 21, 18, 15, 24, 21], [26, 27, 27, 36, 32, 36, 71, 39]),
    ] {
        let bytes = encode_jpeg(&page, q).unwrap();
        let t = dqt(&bytes);
        assert_eq!(t.len(), 2);
        assert_eq!(&t[0][..8], &luma, "품질 {q}");
        assert_eq!(&t[1][..8], &chroma, "품질 {q}");
        // SOF0: Y 2×2, Cb · Cr 1×1(4:2:0). 품질이 높아도 4:4:4 로 바꾸지 않는다
        let sof = bytes.windows(2).position(|w| w == [0xFF, 0xC0]).unwrap();
        assert_eq!((bytes[sof + 11], bytes[sof + 14], bytes[sof + 17]), (0x22, 0x11, 0x11));
    }
    let (w, h, px) = decode_jpeg(&encode_jpeg(&page, 1.0).unwrap());
    assert_eq!((w, h), (17, 9));
    assert!(px.iter().all(|&v| v >= 250));
}

#[test]
fn output_format_and_quality_settings() {
    let s: AtlasSettings = serde_json::from_str(r#"{"outputFormat":"jpg","jpegQuality":0.75}"#).unwrap();
    assert!(s.jpeg() && s.jpeg_quality == 0.75 && s.page_extension() == "jpg");
    let back = serde_json::to_string(&s).unwrap();
    assert!(back.contains("\"outputFormat\":\"jpg\"") && back.contains("\"jpegQuality\":0.75"), "{back}");
    assert_eq!(AtlasSettings::default().jpeg_quality, 0.9);
    assert_eq!(AtlasSettings::default().page_extension(), "png");
    assert!(AtlasSettings { output_format: "JPEG".into(), ..AtlasSettings::default() }.jpeg());
    assert!(AtlasSettings { output_format: "webp".into(), ..AtlasSettings::default() }.check().is_err());
    assert!(AtlasSettings { output_format: "jpg".into(), jpeg_quality: 1.5, ..AtlasSettings::default() }.check().is_err());
    assert!(AtlasSettings { output_format: "jpg".into(), jpeg_quality: 0.0, ..AtlasSettings::default() }.check().is_ok());
}
