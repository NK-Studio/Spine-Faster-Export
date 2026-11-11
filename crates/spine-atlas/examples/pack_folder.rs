//! 이미지 폴더를 Spine 명령줄 패커(`Spine -i <폴더> -o <출력> -p <pack.json>`)처럼 묶는다. Spine 결과와 대조할 때 쓴다.
//!
//! cargo run --release -p spine-atlas --example pack_folder -- <images> <out> <pack.json> [--compare <Spine 출력 폴더>]
//! cargo run --release -p spine-atlas --example pack_folder -- --diff <그림 a> <그림 b>   (PNG · JPG 두 장의 픽셀 차이만)
//!
//! atlas 이름은 Spine 처럼 pack.json 의 파일 이름(확장자 뺀 것)이다. `--compare` 를 주면 atlas 글자가 같은지,
//! 페이지(PNG · JPG)를 풀어 픽셀 차이(채널별 최대 · 평균)를 적는다.

use std::path::{Path, PathBuf};

use spine_atlas::{decode_png_file, encode_page, folder_sources, pack_sources_scales, AtlasSettings};
use spine_model::SkeletonModel;

fn decode(path: &Path) -> (usize, usize, Vec<u8>) {
    if path.extension().is_some_and(|e| e.eq_ignore_ascii_case("png")) {
        let (w, h, px) = decode_png_file(path).unwrap();
        return (w as usize, h as usize, px);
    }
    let data = std::fs::read(path).unwrap();
    let opts = zune_jpeg::zune_core::options::DecoderOptions::default().jpeg_set_out_colorspace(zune_jpeg::zune_core::colorspace::ColorSpace::RGBA);
    let mut d = zune_jpeg::JpegDecoder::new_with_options(zune_jpeg::zune_core::bytestream::ZCursor::new(&data), opts);
    let px = d.decode().unwrap();
    let (w, h) = d.dimensions().unwrap();
    (w, h, px)
}

/// 두 그림의 픽셀 차이(채널별 최대 · 평균). 크기가 다르면 그 말을.
fn diff(a: &Path, b: &Path) -> String {
    let ((w1, h1, p1), (w2, h2, p2)) = (decode(a), decode(b));
    if (w1, h1) != (w2, h2) {
        return format!("크기 다름 {w1}×{h1} / {w2}×{h2}");
    }
    let (mut max, mut sum, mut n) = ([0u8; 4], [0u64; 4], 0u64);
    for (x, y) in p1.chunks_exact(4).zip(p2.chunks_exact(4)) {
        for c in 0..4 {
            let d = x[c].abs_diff(y[c]);
            max[c] = max[c].max(d);
            sum[c] += d as u64;
        }
        n += 1;
    }
    let avg: Vec<String> = sum.iter().map(|s| format!("{:.2}", *s as f64 / n as f64)).collect();
    format!("픽셀 차이 최대 {:?} 평균 [{}]", max, avg.join(", "))
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args[0] == "--diff" {
        println!("{}", diff(Path::new(&args[1]), Path::new(&args[2])));
        return;
    }
    let images = PathBuf::from(&args[0]);
    let out = PathBuf::from(&args[1]);
    let pack_json = PathBuf::from(&args[2]);
    let compare = args.iter().position(|a| a == "--compare").map(|i| PathBuf::from(&args[i + 1]));
    let s: AtlasSettings = serde_json::from_str::<AtlasSettings>(&std::fs::read_to_string(&pack_json).unwrap()).unwrap().normalize();
    let name = pack_json.file_stem().unwrap().to_string_lossy().into_owned();
    let sources = folder_sources(&SkeletonModel::default(), &images, s.combine_subdirectories).unwrap();
    std::fs::create_dir_all(&out).unwrap();
    for (suffix, packed) in pack_sources_scales(&sources, &name, &s).unwrap() {
        let atlas_name = format!("{name}{suffix}{}", s.atlas_extension);
        std::fs::write(out.join(&atlas_name), &packed.output.atlas_text).unwrap();
        for p in &packed.output.pages {
            std::fs::write(out.join(&p.name), encode_page(p, &s).unwrap()).unwrap();
        }
        let Some(cmp) = &compare else { continue };
        let theirs = std::fs::read_to_string(cmp.join(&atlas_name)).unwrap_or_default();
        println!("{atlas_name}: atlas {}", if theirs == packed.output.atlas_text { "같음" } else { "다름" });
        if theirs != packed.output.atlas_text {
            println!("--- Spine\n{theirs}--- 이 앱\n{}", packed.output.atlas_text);
        }
        for p in &packed.output.pages {
            let (a, b) = (out.join(&p.name), cmp.join(&p.name));
            if !b.is_file() {
                println!("  {}: Spine 쪽에 없음", p.name);
                continue;
            }
            let same_bytes = std::fs::read(&a).unwrap() == std::fs::read(&b).unwrap();
            println!("  {}: 바이트 {} · {}", p.name, if same_bytes { "같음" } else { "다름" }, diff(&a, &b));
        }
    }
}
