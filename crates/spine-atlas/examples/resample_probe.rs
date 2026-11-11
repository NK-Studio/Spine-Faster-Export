//! 배율 리샘플링 대조용.
//!
//! gen <폴더>: 표본 PNG 를 만든다.
//! cmp <원본 폴더> <Spine 출력 폴더> <atlas 이름> <방식>: atlas 의 영역마다 Spine 픽셀과 이 앱의 resize 결과를 비교한다.

use std::collections::HashMap;
use std::path::Path;

fn write_png(path: &Path, w: u32, h: u32, rgba: &[u8]) {
    let f = std::fs::File::create(path).unwrap();
    let mut e = png::Encoder::new(std::io::BufWriter::new(f), w, h);
    e.set_color(png::ColorType::Rgba);
    e.set_depth(png::BitDepth::Eight);
    e.write_header().unwrap().write_image_data(rgba).unwrap();
}

struct Lcg(u64);
impl Lcg {
    fn next(&mut self) -> u8 {
        self.0 = self.0.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        (self.0 >> 33) as u8
    }
}

fn gen(dir: &Path) {
    std::fs::create_dir_all(dir).unwrap();
    let mut r = Lcg(7);
    // 불투명 잡음
    let (w, h) = (37u32, 23u32);
    let px: Vec<u8> = (0..w * h).flat_map(|_| [r.next(), r.next(), r.next(), 255]).collect();
    write_png(&dir.join("a.png"), w, h, &px);
    // 알파까지 잡음
    let (w, h) = (50u32, 41u32);
    let px: Vec<u8> = (0..w * h).flat_map(|_| [r.next(), r.next(), r.next(), r.next()]).collect();
    write_png(&dir.join("b.png"), w, h, &px);
    // 부드러운 기울기 + 단단한 원 테두리(가장자리 투명)
    let (w, h) = (64u32, 48u32);
    let mut px = Vec::new();
    for y in 0..h {
        for x in 0..w {
            let (dx, dy) = (x as f32 - 31.5, y as f32 - 23.5);
            let inside = dx * dx + dy * dy < 22.0 * 22.0;
            px.extend_from_slice(&[(x * 4) as u8, (y * 5) as u8, 128, if inside { 255 } else { 0 }]);
        }
    }
    write_png(&dir.join("c.png"), w, h, &px);
    // 1픽셀 줄무늬(나머지 투명)
    let (w, h) = (31u32, 17u32);
    let mut px = Vec::new();
    for y in 0..h {
        for x in 0..w {
            let on = (x + y) % 3 == 0;
            px.extend_from_slice(&[255, if on { 255 } else { 0 }, 0, if on { 255 } else { 90 }]);
        }
    }
    write_png(&dir.join("d.png"), w, h, &px);
}

/// atlas(새 형식)에서 영역 이름 → (페이지, x, y, w, h).
fn regions(atlas: &str) -> HashMap<String, (String, i32, i32, i32, i32)> {
    let mut out = HashMap::new();
    let mut page = String::new();
    let mut name: Option<String> = None;
    let mut new_page = true;
    for line in atlas.lines() {
        let t = line.trim();
        if t.is_empty() {
            new_page = true;
            continue;
        }
        if !t.contains(':') {
            if new_page {
                page = t.to_string();
                new_page = false;
            } else {
                name = Some(t.to_string());
            }
            continue;
        }
        new_page = false;
        if let (Some(n), Some(v)) = (&name, t.strip_prefix("bounds:")) {
            let v: Vec<i32> = v.split(',').map(|s| s.trim().parse().unwrap()).collect();
            out.insert(n.clone(), (page.clone(), v[0], v[1], v[2], v[3]));
        }
    }
    out
}

fn main() {
    let a: Vec<String> = std::env::args().skip(1).collect();
    match a[0].as_str() {
        "gen" => gen(Path::new(&a[1])),
        // gen2 <폴더>: 가로세로 비가 큰 이미지, 아주 넓은·높은 이미지(알파까지 잡음)
        "gen2" => {
            let dir = Path::new(&a[1]);
            std::fs::create_dir_all(dir).unwrap();
            let mut r = Lcg(11);
            for (i, (w, h)) in [(5u32, 100u32), (100, 5), (3, 200), (300, 7), (1500, 3), (3, 1500), (333, 211), (1300, 41), (2, 2), (9, 1)].into_iter().enumerate() {
                let px: Vec<u8> = (0..w * h).flat_map(|_| [r.next(), r.next(), r.next(), r.next().saturating_add(64)]).collect();
                write_png(&dir.join(format!("g{i}.png")), w, h, &px);
            }
        }
        // genmap <폴더> <최대 폭>: 폭 s, 높이 1 이미지들. 칸 번호를 R(하위)·G(상위)에 넣는다.
        "genmap" => {
            let dir = Path::new(&a[1]);
            std::fs::create_dir_all(dir).unwrap();
            for s in 1..=a[2].parse::<u32>().unwrap() {
                let px: Vec<u8> = (0..s).flat_map(|x| [(x & 255) as u8, (x >> 8) as u8, 0, 255]).collect();
                write_png(&dir.join(format!("m{s:03}.png")), s, 1, &px);
            }
        }
        // map <Spine 출력 폴더> <atlas 이름>: 영역마다 "s d 고른 칸들"
        "map" => {
            let (out, name) = (Path::new(&a[1]), &a[2]);
            let atlas = std::fs::read_to_string(out.join(name)).unwrap();
            let mut rs: Vec<_> = regions(&atlas).into_iter().collect();
            rs.sort();
            for (rn, (pg, x, y, w, _)) in rs {
                let (pw, _, pp) = spine_atlas::decode_png_file(&out.join(&pg)).unwrap();
                let s: u32 = rn[1..].parse().unwrap();
                let idx: Vec<String> = (0..w as usize)
                    .map(|i| {
                        let o = (y as usize * pw as usize + x as usize + i) * 4;
                        (pp[o] as u32 | (pp[o + 1] as u32) << 8).to_string()
                    })
                    .collect();
                println!("{s} {w} {}", idx.join(" "));
            }
        }
        "cmp" => {
            let (src, out, name, method) = (Path::new(&a[1]), Path::new(&a[2]), &a[3], &a[4]);
            let scale: f32 = a[5].parse().unwrap();
            let atlas = std::fs::read_to_string(out.join(name)).unwrap();
            let mut pages: HashMap<String, (u32, u32, Vec<u8>)> = HashMap::new();
            let mut names: Vec<_> = regions(&atlas).into_iter().collect();
            names.sort();
            for (rn, (pg, x, y, w, h)) in names {
                let (pw, _, pp) = pages.entry(pg.clone()).or_insert_with(|| spine_atlas::decode_png_file(&out.join(&pg)).unwrap());
                let pw = *pw as usize;
                let spine: Vec<u8> = (0..h as usize)
                    .flat_map(|j| pp[((y as usize + j) * pw + x as usize) * 4..((y as usize + j) * pw + (x + w) as usize) * 4].to_vec())
                    .collect();
                let (sw, sh, srgba) = spine_atlas::decode_png_file(&src.join(format!("{rn}.png"))).unwrap();
                let (nw, nh, mine) = spine_atlas::resize_for_probe(sw as usize, sh as usize, &srgba, scale, method);
                if (nw as i32, nh as i32) != (w, h) {
                    println!("{rn}: 크기 다름 spine {w}x{h} mine {nw}x{nh}");
                    continue;
                }
                let mut diff = [0u32; 4];
                let mut maxd = 0i32;
                let mut first = None;
                for (i, (p, q)) in spine.chunks_exact(4).zip(mine.chunks_exact(4)).enumerate() {
                    // 알파 0 픽셀은 RGB 를 보지 않는다
                    let n = if p[3] == 0 && q[3] == 0 { 0 } else { 4 };
                    for c in 0..n.max(1) {
                        let c = if n == 0 { 3 } else { c };
                        let d = (p[c] as i32 - q[c] as i32).abs();
                        if d > 0 {
                            diff[c] += 1;
                            if first.is_none() {
                                first = Some((i % w as usize, i / w as usize, p.to_vec(), q.to_vec()));
                            }
                        }
                        maxd = maxd.max(d);
                    }
                }
                println!("{rn} {w}x{h}: 다른 칸 RGBA {:?} 최대 {maxd} 처음 {:?}", diff, first);
            }
        }
        // cut <Spine 출력 폴더> <atlas 이름> <저장 폴더>: 영역을 PNG 로 잘라 저장
        "cut" => {
            let (out, name, dst) = (Path::new(&a[1]), &a[2], Path::new(&a[3]));
            std::fs::create_dir_all(dst).unwrap();
            let atlas = std::fs::read_to_string(out.join(name)).unwrap();
            for (rn, (pg, x, y, w, h)) in regions(&atlas) {
                let (pw, _, pp) = spine_atlas::decode_png_file(&out.join(&pg)).unwrap();
                let pw = pw as usize;
                let px: Vec<u8> = (0..h as usize)
                    .flat_map(|j| pp[((y as usize + j) * pw + x as usize) * 4..((y as usize + j) * pw + (x + w) as usize) * 4].to_vec())
                    .collect();
                write_png(&dst.join(format!("{rn}.png")), w as u32, h as u32, &px);
            }
        }
        // diff <a.png> <b.png>: 다른 칸 수(알파 0 끼리는 RGB 무시)
        "diff" => {
            let (aw, ah, ap) = spine_atlas::decode_png_file(Path::new(&a[1])).unwrap();
            let (bw, bh, bp) = spine_atlas::decode_png_file(Path::new(&a[2])).unwrap();
            if (aw, ah) != (bw, bh) {
                println!("크기 다름 {aw}x{ah} {bw}x{bh}");
                return;
            }
            let (mut n, mut maxd, mut first) = (0, 0, None);
            for (i, (p, q)) in ap.chunks_exact(4).zip(bp.chunks_exact(4)).enumerate() {
                let d = if p[3] == 0 && q[3] == 0 { 0 } else { (0..4).map(|c| (p[c] as i32 - q[c] as i32).abs()).max().unwrap() };
                if d > 0 {
                    n += 1;
                    maxd = maxd.max(d);
                    first.get_or_insert((i % aw as usize, i / aw as usize, p.to_vec(), q.to_vec()));
                }
            }
            println!("다른 칸 {n}/{} 최대 {maxd} 처음 {:?}", aw * ah, first);
        }
        // cmp2 <폴더 A> <atlas A> <폴더 B> <atlas B>: 영역 이름마다 atlas 줄(bounds 빼고)과 픽셀을 비교
        "cmp2" => {
            let load = |dir: &str, name: &str| {
                let dir = Path::new(dir);
                let text = std::fs::read_to_string(dir.join(name)).unwrap();
                let mut map: HashMap<String, (String, Vec<String>, Vec<u8>, i32, i32)> = HashMap::new();
                let mut pages: HashMap<String, (u32, u32, Vec<u8>)> = HashMap::new();
                let (mut page, mut cur, mut new_page) = (String::new(), None::<String>, true);
                let mut lines: Vec<String> = Vec::new();
                let mut bounds = (0, 0, 0, 0);
                let mut flush = |cur: &Option<String>, page: &str, lines: &mut Vec<String>, b: (i32, i32, i32, i32), map: &mut HashMap<_, _>| {
                    if let Some(n) = cur {
                        let (pw, _, pp) = pages.entry(page.to_string()).or_insert_with(|| spine_atlas::decode_png_file(&dir.join(page)).unwrap());
                        let pw = *pw as usize;
                        let (x, y, w, h) = b;
                        let px: Vec<u8> = (0..h as usize).flat_map(|j| pp[((y as usize + j) * pw + x as usize) * 4..((y as usize + j) * pw + (x + w) as usize) * 4].to_vec()).collect();
                        map.insert(n.clone(), (page.to_string(), std::mem::take(lines), px, w, h));
                    }
                };
                for line in text.lines() {
                    let t = line.trim();
                    if t.is_empty() {
                        flush(&cur, &page, &mut lines, bounds, &mut map);
                        cur = None;
                        new_page = true;
                        continue;
                    }
                    if !t.contains(':') {
                        if new_page {
                            page = t.to_string();
                            new_page = false;
                        } else {
                            flush(&cur, &page, &mut lines, bounds, &mut map);
                            cur = Some(t.to_string());
                        }
                        continue;
                    }
                    new_page = false;
                    if cur.is_none() {
                        continue;
                    }
                    if let Some(v) = t.strip_prefix("bounds:") {
                        let v: Vec<i32> = v.split(',').map(|s| s.trim().parse().unwrap()).collect();
                        bounds = (v[0], v[1], v[2], v[3]);
                        lines.push(format!("size {}x{}", v[2], v[3]));
                    } else {
                        lines.push(t.replace(" ", ""));
                    }
                }
                flush(&cur, &page, &mut lines, bounds, &mut map);
                map
            };
            let (a_, b_) = (load(&a[1], &a[2]), load(&a[3], &a[4]));
            let mut names: Vec<&String> = a_.keys().collect();
            names.sort();
            let (mut same, mut bad) = (0, 0);
            for n in names {
                let Some(b) = b_.get(n) else {
                    println!("{n}: B 에 없음");
                    bad += 1;
                    continue;
                };
                let a = &a_[n];
                let px_diff = a.2.chunks_exact(4).zip(b.2.chunks_exact(4)).filter(|(p, q)| p != q && !(p[3] == 0 && q[3] == 0)).count();
                if a.1 != b.1 || a.2.len() != b.2.len() || px_diff > 0 {
                    println!("{n}: 줄 A {:?} B {:?} 다른 칸 {px_diff}", a.1, b.1);
                    bad += 1;
                } else {
                    same += 1;
                }
            }
            println!("같음 {same} 다름 {bad} (B 에만 {})", b_.keys().filter(|k| !a_.contains_key(*k)).count());
        }
        _ => panic!("gen | cmp | cut | diff | cmp2"),
    }
}
