//! 이미지 폴더의 모든 PNG 를 영역으로 하는 모델을 만들어 묶는다(파이썬 PoC 와 비교용).
//! 이름을 바이트 순으로 정렬해 세 번째마다(0, 3, 6 …) 메시로 둔다(파이썬 비교 스크립트와 같은 규칙).
//!
//! cargo run --release -p spine-atlas --example pack_dir -- <images> <out> <name> [--no-pma] [--bleed] [--alias] [--max-size N] [--compare 파이썬출력폴더]

use std::path::{Path, PathBuf};
use std::time::Instant;

use spine_atlas::{decode_png_file, encode_page, pack, AtlasSettings};
use spine_model::{ModelAttachment, ModelSkin, ModelSkinEntry, SkeletonModel};

fn walk(dir: &Path, base: &Path, out: &mut Vec<String>) {
    for e in std::fs::read_dir(dir).unwrap() {
        let p = e.unwrap().path();
        if p.is_dir() {
            walk(&p, base, out);
        } else if p.extension().map_or(false, |e| e == "png") {
            let rel = p.strip_prefix(base).unwrap().with_extension("");
            out.push(rel.to_string_lossy().replace('\\', "/"));
        }
    }
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let images = PathBuf::from(&args[0]);
    let out = PathBuf::from(&args[1]);
    let name = &args[2];
    let mut s = AtlasSettings::default();
    let mut compare: Option<PathBuf> = None;
    let mut i = 3;
    while i < args.len() {
        match args[i].as_str() {
            "--no-pma" => s.pma = false,
            "--bleed" => s.bleed = true,
            "--alias" => s.alias = true,
            "--max-size" => {
                i += 1;
                s = s.with_max_size(args[i].parse().unwrap());
            }
            "--compare" => {
                i += 1;
                compare = Some(PathBuf::from(&args[i]));
            }
            a => panic!("모르는 인자 {}", a),
        }
        i += 1;
    }
    let mut names = Vec::new();
    walk(&images, &images, &mut names);
    names.sort();
    let entries = names
        .iter()
        .enumerate()
        .map(|(i, n)| ModelSkinEntry {
            slot: Some("s".into()),
            key: Some(n.clone()),
            attachment: Some(ModelAttachment {
                name: Some(n.clone()),
                r#type: Some(if i % 3 == 0 { "mesh" } else { "region" }.into()),
                ..Default::default()
            }),
        })
        .collect();
    let model = SkeletonModel { skins: vec![ModelSkin { attachments: entries, ..Default::default() }], ..Default::default() };

    let t0 = Instant::now();
    let atlas = pack(&model, &images, name, &s).unwrap();
    let t1 = Instant::now();
    std::fs::create_dir_all(&out).unwrap();
    std::fs::write(out.join(format!("{}.atlas", name)), &atlas.atlas_text).unwrap();
    let pngs: Vec<Vec<u8>> = std::thread::scope(|sc| {
        let hs: Vec<_> = atlas.pages.iter().map(|p| sc.spawn(|| encode_page(p, &s).unwrap())).collect();
        hs.into_iter().map(|h| h.join().unwrap()).collect()
    });
    let t2 = Instant::now();
    for (p, bytes) in atlas.pages.iter().zip(&pngs) {
        std::fs::write(out.join(&p.name), bytes).unwrap();
    }
    let t3 = Instant::now();
    let sizes: Vec<String> = atlas.pages.iter().map(|p| format!("{}x{}", p.width, p.height)).collect();
    println!(
        "영역 {}, 페이지 {:?}, pack {:.1}ms, encode {:.1}ms, write {:.1}ms",
        names.len(),
        sizes,
        (t1 - t0).as_secs_f64() * 1e3,
        (t2 - t1).as_secs_f64() * 1e3,
        (t3 - t2).as_secs_f64() * 1e3
    );

    if let Some(py) = compare {
        let py_text = std::fs::read_to_string(py.join(format!("{}.atlas", name))).unwrap();
        let text_ok = py_text == atlas.atlas_text;
        let mut pix_ok = true;
        for p in &atlas.pages {
            let (w, h, rgba) = decode_png_file(&py.join(&p.name)).unwrap();
            if (w, h) != (p.width, p.height) || rgba != p.rgba {
                pix_ok = false;
                let diff = rgba.iter().zip(&p.rgba).filter(|(a, b)| a != b).count();
                println!("  {} 픽셀 다름: 파이썬 {}x{}, 바이트 차이 {}", p.name, w, h, diff);
            }
            // 우리 PNG 도 다시 읽어 같은지
            let (_, _, again) = decode_png_file(&std::path::Path::new(&out).join(&p.name)).unwrap();
            assert!(again == p.rgba, "인코드 왕복 실패");
        }
        println!("비교: atlas {}, 픽셀 {}", if text_ok { "같음" } else { "다름" }, if pix_ok { "같음" } else { "다름" });
        if !text_ok {
            std::process::exit(1);
        }
    }
}
