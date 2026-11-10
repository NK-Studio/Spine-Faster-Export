//! PNG 를 읽어 RGBA 를 raw 파일로 쓴다(파이썬 read_png 와 비교용). decode_dump <in.png> <out.raw>
fn main() {
    let a: Vec<String> = std::env::args().skip(1).collect();
    let (w, h, rgba) = spine_atlas::decode_png_file(std::path::Path::new(&a[0])).unwrap();
    let mut out = Vec::with_capacity(8 + rgba.len());
    out.extend_from_slice(&w.to_be_bytes());
    out.extend_from_slice(&h.to_be_bytes());
    out.extend_from_slice(&rgba);
    std::fs::write(&a[1], out).unwrap();
}
