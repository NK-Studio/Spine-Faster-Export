//! Packing: Polygons — 메시만 쓰는 이미지는 메시가 덮는 모양(삼각형의 합)만 자리를 차지하게 묶는다.
//!
//! 런타임은 메시의 삼각형 안쪽 텍셀만 읽으므로, 그 바깥은 영역 사각형 안이라도 다른 이미지가 써도 된다.
//! 영역 첨부(region)가 쓰는 이미지는 사각형 전체를 그리므로 사각형 그대로 둔다.
//! atlas 텍스트는 직사각형 패킹과 같다(`bounds` 사각형이 서로 겹칠 수 있을 뿐이다).
//!
//! 배치는 칸(한 변 c 픽셀) 단위 비트맵으로 한다: 모양을 여백만큼 오른쪽·아래로 넓혀 칸에 올리고,
//! 위→아래·왼쪽→오른쪽으로 훑어 처음 겹치지 않는 자리에 놓는다(돌려 본 방향 중 아래 끝이 더 위인 쪽).
//!
//! 자리 차지와 옮길 픽셀은 따로 본다:
//! - 자리 차지: 삼각형이 걸친 픽셀(hull 발자국). 발자국은 실제 hull 보다 변마다 1픽셀 가까이 넓으므로,
//!   모양은 여백을 1픽셀 덜 넓힌다(발자국 사이 빈 픽셀이 `여백 - 1` 이상 → hull 사이 거리는 `여백 ± 1`).
//!   Spine 이 만든 atlas 를 재 보면 다각형 모양은 이렇게 사각형보다 1픽셀 가깝게 놓인다
//!   (사각형끼리, 그리고 이미지를 꽉 채우는 메시는 여백 그대로). 사각형 영역은 여백을 다 넓힌다.
//! - 옮길 픽셀: 발자국은 언제나 제 픽셀이다(발자국끼리는 겹치지 않는다). 선형 필터가 읽는 바깥 한 줄은
//!   그 픽셀을 읽는 영역이 하나일 때만 그 영역의 픽셀로 채운다(`lib.rs` 의 `blit`). 그래서 여백이 2 이상이면
//!   어느 영역도 남의 픽셀을 읽지 않는다(읽는 것은 제 픽셀이거나 빈 픽셀이다).
//!
//! 페이지는 Spine 처럼 최대 폭을 채우고 높이를 쓴 만큼만 잡는다.

use rayon::prelude::*;
use spine_model::SpineResult;

use super::{natural, page_limits, page_size, too_big, AtlasSettings, Geo, Page, Placed, Region};

/// 메시 삼각형(UV, 0~1, 왼쪽 위 원점)을 w×h 비트맵에 칠한다. 삼각형이 조금이라도 걸친 픽셀은 모두 칠한다(hull 발자국).
/// 칠한 곳이 없거나 이미지 전체를 덮으면 None(사각형으로 묶는다).
pub(crate) fn raster_tris(w: usize, h: usize, tris: &[[f64; 6]]) -> Option<Vec<u8>> {
    if w == 0 || h == 0 || tris.is_empty() {
        return None;
    }
    let mut mask = vec![0u8; w * h];
    for t in tris {
        let p = [(t[0] * w as f64, t[1] * h as f64), (t[2] * w as f64, t[3] * h as f64), (t[4] * w as f64, t[5] * h as f64)];
        if p.iter().any(|v| !v.0.is_finite() || !v.1.is_finite()) {
            continue;
        }
        let ymin = p.iter().map(|v| v.1).fold(f64::INFINITY, f64::min);
        let ymax = p.iter().map(|v| v.1).fold(f64::NEG_INFINITY, f64::max);
        let r0 = ymin.floor().max(0.0) as usize;
        let r1 = (ymax.ceil() as i64).clamp(0, h as i64) as usize; // 끝 제외
        for row in r0..r1.max(r0 + 1).min(h) {
            // 줄 [row, row+1] 로 자른 삼각형의 x 범위
            let (y0, y1) = (row as f64, row as f64 + 1.0);
            let (mut lo, mut hi) = (f64::INFINITY, f64::NEG_INFINITY);
            for k in 0..3 {
                let (a, b) = (p[k], p[(k + 1) % 3]);
                if a.1 >= y0 && a.1 <= y1 {
                    lo = lo.min(a.0);
                    hi = hi.max(a.0);
                }
                for yy in [y0, y1] {
                    if (a.1 - yy) * (b.1 - yy) < 0.0 {
                        let x = a.0 + (yy - a.1) / (b.1 - a.1) * (b.0 - a.0);
                        lo = lo.min(x);
                        hi = hi.max(x);
                    }
                }
            }
            if lo > hi {
                continue;
            }
            let x0 = (lo.floor() as i64).clamp(0, w as i64 - 1) as usize;
            let x1 = ((hi.ceil() as i64) - 1).clamp(x0 as i64, w as i64 - 1) as usize;
            mask[row * w + x0..=row * w + x1].fill(1);
        }
    }
    let count = mask.iter().filter(|&&m| m != 0).count();
    (count > 0 && count < w * h).then_some(mask)
}

/// 비트맵(w×h)을 사방으로 n 줄 넓힌 것(배열 밖으로는 나가지 않는다).
pub(crate) fn widen(mask: &[u8], w: usize, h: usize, n: i32) -> Vec<u8> {
    let mut mask = mask.to_vec();
    for _ in 0..n {
        dilate(&mut mask, w, h);
    }
    mask
}

/// 모양 바깥으로 함께 옮기는 픽셀 줄 수: 선형 필터가 읽는 한 줄(여백이 2 이상일 때만 — 이웃 모양과 겹치지 않는다).
pub(crate) fn shape_margin(padding_x: i32, padding_y: i32) -> i32 {
    if padding_x.min(padding_y) >= 2 {
        1
    } else {
        0
    }
}

/// 놓는 방향 번호(0도 · 90도 · 180도 · 270도) → (rotated, flip).
pub(crate) fn orientation(o: usize) -> (bool, bool) {
    (o % 2 == 1, o >= 2)
}

/// 영역 안 (x, y) 가 페이지 위 영역 사각형의 어디인가(런타임이 `rotate:` 를 읽는 것과 같은 규칙).
#[inline]
pub(crate) fn turn(w: usize, h: usize, rotated: bool, flip: bool, x: usize, y: usize) -> (usize, usize) {
    match (rotated, flip) {
        (false, false) => (x, y),
        (true, false) => (y, w - 1 - x),
        (false, true) => (w - 1 - x, h - 1 - y),
        (true, true) => (h - 1 - y, x),
    }
}

/// 3×3 으로 한 픽셀 넓힌다.
fn dilate(mask: &mut [u8], w: usize, h: usize) {
    let src = mask.to_vec();
    for y in 0..h {
        for x in 0..w {
            if src[y * w + x] != 0 {
                continue;
            }
            let hit = (y.saturating_sub(1)..(y + 2).min(h)).any(|ny| (x.saturating_sub(1)..(x + 2).min(w)).any(|nx| src[ny * w + nx] != 0));
            if hit {
                mask[y * w + x] = 1;
            }
        }
    }
}

/// 비트맵의 테두리 상자 (left, top, right, bottom), 모두 포함. 비었으면 None.
pub(crate) fn mask_bounds(mask: &[u8], w: usize, h: usize) -> Option<(i32, i32, i32, i32)> {
    let (mut l, mut t, mut r, mut b) = (w, h, 0usize, 0usize);
    let mut any = false;
    for y in 0..h {
        let row = &mask[y * w..(y + 1) * w];
        let Some(first) = row.iter().position(|&m| m != 0) else { continue };
        let last = row.iter().rposition(|&m| m != 0).unwrap();
        if !any {
            t = y;
        }
        any = true;
        b = y;
        l = l.min(first);
        r = r.max(last);
    }
    any.then_some((l as i32, t as i32, r as i32, b as i32))
}

/// 한 줄에서 칠해진 구간 [x0, x1] 을 차례로 준다.
pub(crate) fn runs(row: &[u8]) -> impl Iterator<Item = (usize, usize)> + '_ {
    let mut x = 0;
    std::iter::from_fn(move || {
        while x < row.len() && row[x] == 0 {
            x += 1;
        }
        if x >= row.len() {
            return None;
        }
        let start = x;
        while x < row.len() && row[x] != 0 {
            x += 1;
        }
        Some((start, x - 1))
    })
}

/// 칸 단위 모양(여백 포함). 비트는 줄마다 u64 로.
struct CellMask {
    /// 칸 줄 수.
    h: i32,
    wpr: usize,
    bits: Vec<u64>,
    /// 페이지 위 픽셀 크기(회전 반영, 여백 제외).
    pw: i32,
    ph: i32,
    /// 첫 줄에서 처음 칠해진 칸(빠르게 건너뛰는 데 쓴다).
    first: i32,
    /// 칠해진 칸 수.
    cells: i64,
}

fn fill_bits(row: &mut [u64], x0: i32, x1: i32) {
    let (w0, w1) = ((x0 / 64) as usize, (x1 / 64) as usize);
    let lo = !0u64 << (x0 % 64);
    let hi = !0u64 >> (63 - x1 % 64);
    if w0 == w1 {
        row[w0] |= lo & hi;
    } else {
        row[w0] |= lo;
        row[w0 + 1..w1].fill(!0);
        row[w1] |= hi;
    }
}

fn cell_mask(r: &Region, o: usize, g: Geo, c: i32) -> CellMask {
    let (rotated, flip) = orientation(o);
    let (pw, ph) = if rotated { (r.h, r.w) } else { (r.w, r.h) };
    let cw = (pw + g.px + c - 1) / c;
    let ch = (ph + g.py + c - 1) / c;
    let wpr = (cw as usize + 63) / 64;
    let mut bits = vec![0u64; wpr * ch as usize];
    // 페이지 위 사각형 [x0, x1]×[y0, y1](여백 포함)을 칸에 칠한다
    let mut rect = |x0: i32, x1: i32, y0: i32, y1: i32| {
        for row in y0 / c..=y1 / c {
            fill_bits(&mut bits[row as usize * wpr..(row as usize + 1) * wpr], x0 / c, x1 / c);
        }
    };
    match &r.mask {
        None => rect(0, pw + g.px - 1, 0, ph + g.py - 1),
        Some(m) => {
            let (w, h) = (r.w as usize, r.h as usize);
            for (y, row) in m.chunks_exact(w).enumerate() {
                for (a, b) in runs(row) {
                    let (p, q) = (turn(w, h, rotated, flip, a, y), turn(w, h, rotated, flip, b, y));
                    let (x0, x1) = (p.0.min(q.0) as i32, p.0.max(q.0) as i32);
                    let (y0, y1) = (p.1.min(q.1) as i32, p.1.max(q.1) as i32);
                    // 모양은 여백을 1픽셀 덜 넓힌다(파일 머리말)
                    rect(x0, x1 + (g.px - 1).max(0), y0, y1 + (g.py - 1).max(0));
                }
            }
        }
    }
    let first = (0..cw).find(|&x| bits[(x / 64) as usize] >> (x % 64) & 1 != 0).unwrap_or(0);
    let cells = bits.iter().map(|b| b.count_ones() as i64).sum();
    CellMask { h: ch, wpr, bits, pw, ph, first, cells }
}

/// 페이지의 칸 점유 비트맵.
struct Grid {
    wpr: usize,
    bits: Vec<u64>,
}

impl Grid {
    fn new(cols: i32, rows: i32) -> Grid {
        let wpr = (cols as usize + 63) / 64 + 1;
        Grid { wpr, bits: vec![0u64; wpr * rows as usize] }
    }

    fn collides(&self, m: &CellMask, cx: i32, cy: i32) -> bool {
        let (wo, sh) = ((cx / 64) as usize, (cx % 64) as u32);
        for j in 0..m.h as usize {
            let grow = &self.bits[(cy as usize + j) * self.wpr..(cy as usize + j + 1) * self.wpr];
            let mrow = &m.bits[j * m.wpr..(j + 1) * m.wpr];
            for (k, &mw) in mrow.iter().enumerate() {
                if mw == 0 {
                    continue;
                }
                if grow[wo + k] & (mw << sh) != 0 || (sh != 0 && grow[wo + k + 1] & (mw >> (64 - sh)) != 0) {
                    return true;
                }
            }
        }
        false
    }

    fn put(&mut self, m: &CellMask, cx: i32, cy: i32) {
        let (wo, sh) = ((cx / 64) as usize, (cx % 64) as u32);
        for j in 0..m.h as usize {
            let base = (cy as usize + j) * self.wpr + wo;
            for (k, &mw) in m.bits[j * m.wpr..(j + 1) * m.wpr].iter().enumerate() {
                if mw == 0 {
                    continue;
                }
                self.bits[base + k] |= mw << sh;
                if sh != 0 {
                    self.bits[base + k + 1] |= mw >> (64 - sh);
                }
            }
        }
    }

    /// cy 줄에서 x 이상의 첫 빈칸(limit 를 넘으면 limit + 1).
    fn next_clear(&self, cy: i32, x: i32, limit: i32) -> i32 {
        let row = &self.bits[cy as usize * self.wpr..(cy as usize + 1) * self.wpr];
        let mut x = x;
        while x <= limit {
            let word = !row[(x / 64) as usize] >> (x % 64);
            if word != 0 {
                return x + word.trailing_zeros() as i32;
            }
            x = (x / 64 + 1) * 64;
        }
        limit + 1
    }
}

/// 한 페이지에 차례로 놓는다. (놓인 것, 못 놓은 것)
fn pack_page(order: &[usize], masks: &[Option<Vec<CellMask>>], width: i32, height: i32, g: Geo, c: i32) -> (Vec<Placed>, Vec<usize>) {
    let (inner_w, inner_h) = (width - 2 * g.ex, height - 2 * g.ey);
    let mut grid = Grid::new((inner_w + g.px) / c + 3, (inner_h + g.py) / c + 3);
    let mut placed = Vec::new();
    let mut left = Vec::new();
    for &idx in order {
        let pair = masks[idx].as_ref().unwrap();
        // (아래 끝, 칸 x, 칸 y, 방향)
        let mut best: Option<(i32, i32, i32, usize)> = None;
        for (o, m) in pair.iter().enumerate() {
            if m.pw > inner_w || m.ph > inner_h {
                continue;
            }
            let (max_cx, max_cy) = ((inner_w - m.pw) / c, (inner_h - m.ph) / c);
            'scan: for cy in 0..=max_cy {
                if best.is_some_and(|b| cy * c + m.ph >= b.0) {
                    break;
                }
                let mut cx = 0;
                loop {
                    cx = grid.next_clear(cy, cx + m.first, max_cx + m.first) - m.first;
                    if cx > max_cx {
                        break;
                    }
                    if !grid.collides(m, cx, cy) {
                        best = Some((cy * c + m.ph, cx, cy, o));
                        break 'scan;
                    }
                    cx += 1;
                }
            }
        }
        match best {
            Some((_, cx, cy, o)) => {
                grid.put(&pair[o], cx, cy);
                let (rotated, flip) = orientation(o);
                placed.push(Placed { idx, x: g.ex + cx * c, y: g.ey + cy * c, rotated, flip });
            }
            None => left.push(idx),
        }
    }
    (placed, left)
}

/// 모양대로 묶는다. 영역의 x·y·rotated 를 채우고 페이지를 돌려준다.
pub(crate) fn pack_polygons(regions: &mut [Region], packed: &[usize], s: &AtlasSettings) -> SpineResult<Vec<Page>> {
    let g = Geo::of(s);
    let (max_w, max_h) = page_limits(s);
    let mut todo: Vec<usize> = packed.to_vec();
    todo.sort_by(|&a, &b| {
        let (ra, rb) = (&regions[a], &regions[b]);
        rb.w.max(rb.h)
            .cmp(&ra.w.max(ra.h))
            .then(rb.w.min(rb.h).cmp(&ra.w.min(ra.h)))
            .then_with(|| natural::ordinal_cmp(&ra.name, &rb.name))
    });

    // 칸 크기: 페이지 한 변이 정해 둔 칸 수를 넘지 않게(칸이 작을수록 촘촘하고 느리다)
    let pixels: i64 = todo
        .iter()
        .map(|&i| match &regions[i].mask {
            Some(m) => m.iter().filter(|&&v| v != 0).count() as i64,
            None => regions[i].w as i64 * regions[i].h as i64,
        })
        .sum();
    let side = ((pixels as f64).sqrt() as i32).min(max_w.max(max_h)).max(1);
    let target = if s.fast { 256 } else { 640 };
    let c = ((side + target - 1) / target).max(1);

    let regs: &[Region] = regions;
    // 방향: 회전을 끄면 0도만, 사각형은 0도·90도, 모양(메시 전용)은 180도·270도까지(Spine 도 메시 이미지만 그렇게 돌린다)
    let mut masks: Vec<Option<Vec<CellMask>>> = (0..regs.len()).map(|_| None).collect();
    let built: Vec<(usize, Vec<CellMask>)> = todo
        .par_iter()
        .map(|&i| {
            let n = if !g.rotation { 1 } else if regs[i].mask.is_some() { 4 } else { 2 };
            (i, (0..n).map(|o| cell_mask(&regs[i], o, g, c)).collect())
        })
        .collect();
    for (i, pair) in built {
        masks[i] = Some(pair);
    }

    let mut pages = Vec::new();
    while !todo.is_empty() {
        let regs: &[Region] = regions;
        // Spine 처럼 최대 폭을 채우고 높이를 줄인다. 대신 놓는 순서를 여러 가지로 시험한다.
        let cells = |i: usize| masks[i].as_ref().unwrap()[0].cells;
        let long = |i: usize| regs[i].w.max(regs[i].h);
        let short = |i: usize| regs[i].w.min(regs[i].h);
        let orders: Vec<Vec<usize>> = (0..if s.fast { 1 } else { 4 })
            .map(|kind| {
                let mut o = todo.clone(); // todo 는 긴 변 순(같으면 이름 순)이고 정렬은 안정적이다
                match kind {
                    1 => o.sort_by_key(|&i| std::cmp::Reverse(short(i))),
                    2 => o.sort_by_key(|&i| std::cmp::Reverse(cells(i))),
                    3 => o.sort_by_key(|&i| std::cmp::Reverse(long(i) as i64 + short(i) as i64)),
                    _ => {}
                }
                o
            })
            .collect();
        let trials: Vec<(Vec<Placed>, Vec<usize>)> = orders.par_iter().map(|o| pack_page(o, &masks, max_w, max_h, g, c)).collect();

        let mut best: Option<(usize, i64, i32, i32, usize)> = None; // (못 놓은 수, 넓이, w, h, 시험 번호)
        for (t, (placed, left)) in trials.iter().enumerate() {
            if placed.is_empty() {
                continue;
            }
            let used_w = placed.iter().map(|p| p.x + if p.rotated { regs[p.idx].h } else { regs[p.idx].w }).max().unwrap() + g.ex;
            let used_h = placed.iter().map(|p| p.y + if p.rotated { regs[p.idx].w } else { regs[p.idx].h }).max().unwrap() + g.ey;
            let (pw, ph) = page_size(used_w, used_h, s, max_w, max_h);
            let page_area = pw as i64 * ph as i64;
            let better = match best {
                None => true,
                Some((bl, ba, ..)) => left.len() < bl || (left.len() == bl && page_area < ba),
            };
            if better {
                best = Some((left.len(), page_area, pw, ph, t));
            }
        }
        let Some((_, _, bw, bh, t)) = best else {
            return Err(too_big(regions, todo[0], max_w, max_h));
        };
        let (placed, left) = trials.into_iter().nth(t).unwrap();
        for p in &placed {
            let r = &mut regions[p.idx];
            r.x = p.x;
            r.y = p.y;
            r.rotated = p.rotated;
            r.flip = p.flip;
        }
        pages.push(Page { w: bw, h: bh, regions: placed.iter().map(|p| p.idx).collect() });
        todo = left;
    }
    Ok(pages)
}
