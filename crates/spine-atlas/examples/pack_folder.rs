//! 이미지 폴더를 Spine 명령줄 패커(`Spine -i <폴더> -o <출력> -p <pack.json>`)처럼 묶는다. 폴더 안 `pack.json` 도 읽는다.
//! atlas 와 페이지 이름은 pack.json 파일 이름(확장자 뺀 것)을 쓴다. 설정 파일은 이 앱의 기본 설정에 덮어 읽는다.
//!
//! cargo run --release -p spine-atlas --example pack_folder -- <images> <out> <pack.json>

use std::path::PathBuf;

use spine_atlas::{encode_png, folder_sources, merge_pack_json, pack_sources_scales, AtlasSettings};
use spine_model::SkeletonModel;

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let (images, out, settings) = (PathBuf::from(&args[0]), PathBuf::from(&args[1]), PathBuf::from(&args[2]));
    let name = settings.file_stem().unwrap().to_string_lossy().into_owned();
    let root = merge_pack_json(&AtlasSettings::default(), &std::fs::read_to_string(&settings).unwrap()).unwrap();
    let sources = folder_sources(&SkeletonModel::default(), &images, &root).unwrap();
    std::fs::create_dir_all(&out).unwrap();
    for (suffix, packed) in pack_sources_scales(&sources, &name, &root).unwrap() {
        std::fs::write(out.join(format!("{name}{suffix}{}", root.atlas_extension)), &packed.output.atlas_text).unwrap();
        for page in &packed.output.pages {
            std::fs::write(out.join(&page.name), encode_png(page)).unwrap();
        }
    }
}
