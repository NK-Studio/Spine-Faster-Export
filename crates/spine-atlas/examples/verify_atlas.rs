//! 내보낸 atlas 를 런타임처럼 읽어, 첨부가 그릴 픽셀이 원본 이미지와 같은지 본다(다각형 패킹 검증용).
//! PMA · 블리드 · 배율 · duplicate padding 없이 내보낸 결과에 쓴다(픽셀을 그대로 비교한다).
//!
//!   cargo run --release -p spine-atlas --example verify_atlas -- <내보낸 폴더> <프로젝트 폴더>
//!
//! 영역 첨부: 영역 사각형 전체. 메시: 삼각형이 걸친 픽셀 모두.
//! 그리고 그 바로 바깥 한 줄(선형 필터가 함께 읽는 줄)에 남의 픽셀이 없는지: 제 픽셀이거나 비어 있어야 한다.

use std::collections::HashMap;
use std::path::Path;

use serde_json::Value;
use spine_atlas::decode_png_file;

struct Region {
    page: usize,
    b: [i64; 4],
    /// (left, bottom, 원본 w, 원본 h)
    o: Option<[i64; 4]>,
    /// rotate: 0 · 90 · 180 · 270
    rot: i64,
}

struct Img {
    w: i64,
    h: i64,
    px: Vec<u8>,
}

fn load(p: &Path) -> Option<Img> {
    let (w, h, px) = decode_png_file(p).ok()?;
    Some(Img { w: w as i64, h: h as i64, px })
}

fn parse_atlas(text: &str, dir: &Path) -> (Vec<Img>, HashMap<String, Region>) {
    let mut pages = Vec::new();
    let mut regions = HashMap::new();
    let mut name: Option<String> = None;
    let mut new_page = true;
    for line in text.lines() {
        if line.is_empty() {
            new_page = true;
            continue;
        }
        let nums = |v: &str| -> Vec<i64> { v.split(',').map(|n| n.trim().parse().unwrap()).collect() };
        if let Some((k, v)) = line.split_once(':') {
            let Some(r) = name.as_ref().and_then(|n| regions.get_mut(n)) else { continue };
            let r: &mut Region = r;
            match k {
                "bounds" => r.b = nums(v).try_into().unwrap(),
                "offsets" => r.o = Some(nums(v).try_into().unwrap()),
                "rotate" => r.rot = if v == "true" { 90 } else { v.parse().unwrap_or(0) },
                _ => {}
            }
        } else if new_page {
            pages.push(load(&dir.join(line)).expect("페이지 PNG"));
            new_page = false;
            name = None;
        } else {
            regions.insert(line.to_string(), Region { page: pages.len() - 1, b: [0; 4], o: None, rot: 0 });
            name = Some(line.to_string());
        }
    }
    (pages, regions)
}

/// 영역의 왼쪽 위(원본 좌표).
fn origin(r: &Region) -> (i64, i64) {
    match r.o {
        Some(o) => (o[0], o[3] - o[1] - r.b[3]),
        None => (0, 0),
    }
}

/// 원본 (x, y) 가 페이지의 어디인가와, 그곳이 영역 안인가. 영역 밖 좌표도 같은 규칙으로 이어서 계산한다.
fn spot(r: &Region, x: i64, y: i64) -> ((i64, i64), bool) {
    let [bx, by, bw, bh] = r.b;
    let (left, top) = origin(r);
    let (lx, ly) = (x - left, y - top);
    let at = match r.rot {
        90 => (bx + ly, by + bw - 1 - lx),
        180 => (bx + bw - 1 - lx, by + bh - 1 - ly),
        270 => (bx + bh - 1 - ly, by + lx),
        _ => (bx + lx, by + ly),
    };
    (at, lx >= 0 && ly >= 0 && lx < bw && ly < bh)
}

/// 원본 (x, y) 가 페이지의 어디인가(영역 밖이면 None).
fn locate(r: &Region, x: i64, y: i64) -> Option<(i64, i64)> {
    let (at, inside) = spot(r, x, y);
    inside.then_some(at)
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let (out, project) = (Path::new(&args[0]), Path::new(&args[1]));
    let mut jsons: Vec<_> = std::fs::read_dir(out).unwrap().flatten().map(|e| e.path()).filter(|p| p.extension().is_some_and(|e| e == "json")).collect();
    jsons.sort();
    let (mut total_px, mut bad_px, mut outside, mut atts, mut meshes, mut missing) = (0u64, 0u64, 0u64, 0, 0, 0);
    let (mut ring_px, mut foreign) = (0u64, 0u64);
    let (mut area, mut overlaps) = (0i64, 0);
    for jp in &jsons {
        let v: Value = serde_json::from_str(&std::fs::read_to_string(jp).unwrap()).unwrap();
        // 스켈레톤 이름의 atlas 가 없으면 폴더에 하나뿐인 atlas(Single atlas)
        let single = || std::fs::read_dir(out).unwrap().flatten().map(|e| e.path()).find(|p| p.extension().is_some_and(|e| e == "atlas"));
        let atlas_path = Some(jp.with_extension("atlas")).filter(|p| p.is_file()).or_else(single);
        let Some(text) = atlas_path.and_then(|p| std::fs::read_to_string(p).ok()) else { continue };
        let (pages, regions) = parse_atlas(&text, out);
        area += pages.iter().map(|p| p.w * p.h).sum::<i64>();
        // bounds 사각형이 겹치는 쌍(다각형 패킹에서만 생긴다)
        let rects: Vec<(usize, i64, i64, i64, i64)> = regions
            .values()
            .map(|r| {
                let (w, h) = if r.rot == 90 || r.rot == 270 { (r.b[3], r.b[2]) } else { (r.b[2], r.b[3]) };
                (r.page, r.b[0], r.b[1], w, h)
            })
            .collect();
        for (i, a) in rects.iter().enumerate() {
            for b in &rects[i + 1..] {
                if a.0 == b.0 && a.1 < b.1 + b.3 && b.1 < a.1 + a.3 && a.2 < b.2 + b.4 && b.2 < a.2 + a.4 {
                    overlaps += 1;
                }
            }
        }
        let images = project.join(v["skeleton"]["images"].as_str().unwrap_or("./images/"));
        let mut sources: HashMap<String, Option<Img>> = HashMap::new();
        for skin in v["skins"].as_array().into_iter().flatten() {
            for (_slot, entries) in skin["attachments"].as_object().into_iter().flatten() {
                for (key, a) in entries.as_object().into_iter().flatten() {
                    let kind = a["type"].as_str().unwrap_or("region");
                    if kind != "region" && kind != "mesh" && kind != "linkedmesh" {
                        continue;
                    }
                    // 연결 메시는 원본 메시의 UV · 삼각형을 쓴다(적힌 스킨, 없으면 모든 스킨에서 같은 슬롯의 이름)
                    let geo = if kind == "linkedmesh" {
                        let parent = a["parent"].as_str().or(a["source"].as_str()).unwrap_or("");
                        let named = a["skin"].as_str();
                        let all = v["skins"].as_array().unwrap();
                        let find = |strict: bool| all.iter().filter(|s| !strict || s["name"].as_str() == named.or(skin["name"].as_str())).map(|s| &s["attachments"][_slot.as_str()][parent]).find(|p| p["type"] == "mesh");
                        match find(true).or_else(|| find(false)) {
                            Some(p) => p,
                            None => panic!("{key}: 원본 메시 {parent} 를 찾지 못했습니다"),
                        }
                    } else {
                        a
                    };
                    let name = a["path"].as_str().or(a["name"].as_str()).unwrap_or(key).to_string();
                    let Some(src) = sources.entry(name.clone()).or_insert_with(|| load(&images.join(format!("{name}.png")))).as_ref() else {
                        missing += 1;
                        continue;
                    };
                    let r = regions.get(&name).unwrap_or_else(|| panic!("{}: atlas 에 {name} 이 없습니다", jp.display()));
                    let page = &pages[r.page];
                    atts += 1;
                    let mut check = |x: i64, y: i64| {
                        if x < 0 || y < 0 || x >= src.w || y >= src.h {
                            return;
                        }
                        total_px += 1;
                        let s = ((y * src.w + x) * 4) as usize;
                        match locate(r, x, y) {
                            Some((px, py)) => {
                                let d = ((py * page.w + px) * 4) as usize;
                                // 완전 투명 픽셀은 색을 따지지 않는다
                                let same = page.px[d + 3] == src.px[s + 3] && (src.px[s + 3] == 0 || page.px[d..d + 3] == src.px[s..s + 3]);
                                if !same {
                                    if bad_px < 5 {
                                        eprintln!("다름: {} {name} ({x},{y}) 원본 {:?} 페이지 {:?}", jp.file_name().unwrap().to_string_lossy(), &src.px[s..s + 4], &page.px[d..d + 4]);
                                    }
                                    bad_px += 1;
                                }
                            }
                            // 잘려 나간 곳은 투명해야 한다(영역 첨부). 메시는 삼각형 안이 잘리면 안 된다.
                            None => {
                                if kind == "mesh" || src.px[s + 3] > 3 {
                                    if outside < 5 {
                                        eprintln!("영역 밖: {} {name} ({x},{y}) 알파 {}", jp.file_name().unwrap().to_string_lossy(), src.px[s + 3]);
                                    }
                                    outside += 1;
                                }
                            }
                        }
                    };
                    // 바깥 한 줄: 페이지의 그 자리는 비어 있거나 이 이미지의 픽셀이어야 한다
                    let mut ring_check = |x: i64, y: i64| {
                        let ((px, py), _) = spot(r, x, y);
                        if px < 0 || py < 0 || px >= page.w || py >= page.h {
                            return;
                        }
                        ring_px += 1;
                        let d = ((py * page.w + px) * 4) as usize;
                        let own = x >= 0 && y >= 0 && x < src.w && y < src.h && {
                            let s = ((y * src.w + x) * 4) as usize;
                            page.px[d..d + 4] == src.px[s..s + 4]
                        };
                        if page.px[d + 3] != 0 && !own {
                            if foreign < 5 {
                                eprintln!("바깥 줄에 남의 픽셀: {} {name} ({x},{y}) 페이지 {:?}", jp.file_name().unwrap().to_string_lossy(), &page.px[d..d + 4]);
                            }
                            foreign += 1;
                        }
                    };
                    if kind == "region" {
                        for y in 0..src.h {
                            for x in 0..src.w {
                                check(x, y);
                            }
                        }
                        let ((left, top), [_, _, bw, bh]) = (origin(r), r.b);
                        for y in top - 1..=top + bh {
                            for x in left - 1..=left + bw {
                                if x < left || y < top || x >= left + bw || y >= top + bh {
                                    ring_check(x, y);
                                }
                            }
                        }
                        continue;
                    }
                    meshes += 1;
                    let mut read: std::collections::HashSet<(i64, i64)> = Default::default();
                    let uvs: Vec<f64> = geo["uvs"].as_array().unwrap().iter().map(|n| n.as_f64().unwrap()).collect();
                    let tri: Vec<usize> = geo["triangles"].as_array().unwrap().iter().map(|n| n.as_u64().unwrap() as usize).collect();
                    for t in tri.chunks_exact(3) {
                        let p: Vec<(f64, f64)> = t.iter().map(|&i| (uvs[i * 2] * src.w as f64, uvs[i * 2 + 1] * src.h as f64)).collect();
                        let edge = |a: (f64, f64), b: (f64, f64), q: (f64, f64)| (b.0 - a.0) * (q.1 - a.1) - (b.1 - a.1) * (q.0 - a.0);
                        let (x0, x1) = (p.iter().map(|v| v.0).fold(f64::MAX, f64::min).floor() as i64, p.iter().map(|v| v.0).fold(f64::MIN, f64::max).ceil() as i64);
                        let (y0, y1) = (p.iter().map(|v| v.1).fold(f64::MAX, f64::min).floor() as i64, p.iter().map(|v| v.1).fold(f64::MIN, f64::max).ceil() as i64);
                        let sign = if edge(p[0], p[1], p[2]) < 0.0 { -1.0 } else { 1.0 };
                        for y in y0..y1 {
                            for x in x0..x1 {
                                // 픽셀 네 모서리(삼각형 안쪽) · 중심(변 위 포함) 중 하나라도 삼각형 안이면 본다
                                let pts = [(x as f64, y as f64), (x as f64 + 1.0, y as f64), (x as f64, y as f64 + 1.0), (x as f64 + 1.0, y as f64 + 1.0)];
                                let within = |q: (f64, f64), eps: f64| (0..3).all(|k| sign * edge(p[k], p[(k + 1) % 3], q) >= eps);
                                if (pts.iter().any(|&q| within(q, 1e-9)) || within((x as f64 + 0.5, y as f64 + 0.5), 0.0)) && read.insert((x, y)) {
                                    check(x, y);
                                }
                            }
                        }
                    }
                    let mut ring: std::collections::HashSet<(i64, i64)> = Default::default();
                    for &(x, y) in &read {
                        for (nx, ny) in (-1..=1).flat_map(|dy| (-1..=1).map(move |dx| (x + dx, y + dy))) {
                            if !read.contains(&(nx, ny)) && ring.insert((nx, ny)) {
                                ring_check(nx, ny);
                            }
                        }
                    }
                }
            }
        }
    }
    println!(
        "스켈레톤 {} · 첨부 {atts}(메시 {meshes}) · 본 픽셀 {total_px} · 다른 픽셀 {bad_px} · 영역 밖 {outside} · 바깥 줄 {ring_px} 중 남의 픽셀 {foreign} · 없는 이미지 {missing} · 페이지 넓이 합 {area} · 겹치는 bounds 쌍 {overlaps}",
        jsons.len()
    );
    if bad_px + outside + foreign > 0 {
        std::process::exit(1);
    }
}
