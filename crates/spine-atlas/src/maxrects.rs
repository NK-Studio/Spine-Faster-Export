//! 사각형 배치: Spine 의 텍스처 패커와 같은 자리에 놓는다.
//!
//! libGDX 의 `MaxRectsPacker`(Nathan Sweet, Apache License 2.0, Jukka Jylänki 의 MaxRects 를 옮긴 것)를 바탕으로,
//! Spine 4.2 명령줄 패커의 결과와 좌표까지 대조해 맞춘 규칙을 따른다.
//! - Fast 는 Contact point 규칙 하나로, 영역을 정렬한 순서대로 넣는다(넣지 못하면 거기서 멈춘다).
//! - Fast 가 아니면 다섯 규칙(Best short side · Best long side · Best area · Bottom left · Contact point)으로
//!   매번 가장 잘 맞는 영역을 골라 넣고, 규칙 사이에서는 페이지 이미지가 작은 것(같으면 뒤 규칙)을 고른다.
//! - 페이지 크기는 폭·높이를 이진 탐색하고, 크기 사이에서는 실제로 쓴 넓이가 작은 것(같으면 나중 것)을 고른다.
//! - 빈칸 정리(prune)는 예전 libGDX 처럼 모든 빈칸 쌍을 차례로 비교한다.
//!
//! 좌표는 libGDX 처럼 아래가 0 이고, 내보낼 때 atlas 의 위 기준으로 뒤집는다.

/// 패커 설정(필요한 것만).
#[derive(Clone, Copy, Debug)]
pub(crate) struct Settings {
    pub px: i32,
    pub py: i32,
    pub max_w: i32,
    pub max_h: i32,
    pub min_w: i32,
    pub min_h: i32,
    pub edge_padding: bool,
    pub duplicate_padding: bool,
    pub rotation: bool,
    pub fast: bool,
    pub square: bool,
    pub pot: bool,
    pub mod4: bool,
}

/// 놓인 영역: atlas 좌표(위 기준, 가장자리 여백 포함).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Placed {
    pub idx: usize,
    pub x: i32,
    pub y: i32,
    pub rotated: bool,
}

/// 페이지: 이미지 크기와 놓인 영역.
#[derive(Clone, Debug)]
pub(crate) struct PageOut {
    pub w: i32,
    pub h: i32,
    pub placed: Vec<Placed>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct R {
    x: i32,
    y: i32,
    w: i32,
    h: i32,
}

/// 넣을 영역(오른쪽·위 여백 포함 크기).
#[derive(Clone, Copy, Debug)]
struct Item {
    idx: usize,
    w: i32,
    h: i32,
}

#[derive(Clone, Copy, Debug)]
struct Node {
    x: i32,
    y: i32,
    w: i32,
    h: i32,
    s1: i32,
    s2: i32,
    rotated: bool,
}

impl Node {
    #[cfg(test)]
    fn empty() -> Node {
        Node { x: 0, y: 0, w: 0, h: 0, s1: 0, s2: 0, rotated: false }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Method {
    BestShortSideFit,
    BestLongSideFit,
    BestAreaFit,
    BottomLeft,
    ContactPoint,
}

const ALL: [Method; 5] = [Method::BestShortSideFit, Method::BestLongSideFit, Method::BestAreaFit, Method::BottomLeft, Method::ContactPoint];
const FAST: [Method; 1] = [Method::ContactPoint];

/// 한 번 시험한 결과.
#[derive(Clone, Debug)]
struct Trial {
    /// (영역, 놓인 자리) — 자리는 여백 포함 크기.
    out: Vec<(usize, Node)>,
    remaining: Vec<Item>,
    w: i32,
    h: i32,
}

fn next_pot(v: i32) -> i32 {
    if v <= 0 {
        1
    } else {
        (v as u32).next_power_of_two() as i32
    }
}

fn round4(v: i32) -> i32 {
    if v % 4 == 0 {
        v
    } else {
        v + 4 - v % 4
    }
}

/// 크기 이진 탐색(libGDX 의 BinarySearch 와 같다).
struct Search {
    pot: bool,
    mod4: bool,
    min: i32,
    max: i32,
    fuzz: i32,
    low: i32,
    high: i32,
    cur: i32,
}

impl Search {
    fn new(min: i32, max: i32, fuzz: i32, pot: bool, mod4: bool) -> Search {
        let (min, max) = if pot {
            (next_pot(min).trailing_zeros() as i32, next_pot(max).trailing_zeros() as i32)
        } else if mod4 {
            (round4(min), round4(max))
        } else {
            (min, max)
        };
        Search { pot, mod4, min, max, fuzz: if pot { 0 } else { fuzz }, low: 0, high: 0, cur: 0 }
    }

    fn value(&self) -> i32 {
        if self.pot {
            1i32.checked_shl(self.cur as u32).unwrap_or(i32::MAX)
        } else if self.mod4 {
            round4(self.cur)
        } else {
            self.cur
        }
    }

    fn reset(&mut self) -> i32 {
        self.low = self.min;
        self.high = self.max;
        self.cur = ((self.low as u32).wrapping_add(self.high as u32) >> 1) as i32;
        self.value()
    }

    /// `failed`: 이 크기에 다 들어가지 않았다. -1 이면 끝.
    fn next(&mut self, failed: bool) -> i32 {
        if self.low >= self.high {
            return -1;
        }
        if failed {
            self.low = self.cur + 1;
        } else {
            self.high = self.cur - 1;
        }
        self.cur = ((self.low as u32).wrapping_add(self.high as u32) >> 1) as i32;
        if (self.low - self.high).abs() < self.fuzz {
            return -1;
        }
        self.value()
    }
}

/// MaxRects 한 판(libGDX 를 그대로 옮긴 것). 테스트에서 [`Quick`] 과 대조한다.
#[cfg(test)]
struct Bin {
    w: i32,
    h: i32,
    rotation: bool,
    px: i32,
    py: i32,
    used: Vec<R>,
    free: Vec<R>,
}

#[cfg(test)]
impl Bin {
    fn new(w: i32, h: i32, s: &Settings) -> Bin {
        Bin { w, h, rotation: s.rotation, px: s.px, py: s.py, used: Vec::new(), free: vec![R { x: 0, y: 0, w, h }] }
    }

    fn score(&self, it: &Item, m: Method) -> Node {
        let (w, h) = (it.w, it.h);
        let rw = h - self.py + self.px;
        let rh = w - self.px + self.py;
        let rot = self.rotation;
        let mut n = match m {
            Method::BestShortSideFit => self.bssf(w, h, rw, rh, rot),
            Method::BottomLeft => self.bl(w, h, rw, rh, rot),
            Method::ContactPoint => {
                let mut n = self.cp(w, h, rw, rh, rot);
                n.s1 = -n.s1;
                n
            }
            Method::BestLongSideFit => self.blsf(w, h, rw, rh, rot),
            Method::BestAreaFit => self.baf(w, h, rw, rh, rot),
        };
        if n.h == 0 {
            n.s1 = i32::MAX;
            n.s2 = i32::MAX;
        }
        n
    }

    fn bl(&self, w: i32, h: i32, rw: i32, rh: i32, rot: bool) -> Node {
        let mut b = Node::empty();
        b.s1 = i32::MAX;
        for f in &self.free {
            if f.w >= w && f.h >= h {
                let top = f.y + h;
                if top < b.s1 || (top == b.s1 && f.x < b.s2) {
                    b = Node { x: f.x, y: f.y, w, h, s1: top, s2: f.x, rotated: false };
                }
            }
            if rot && f.w >= rw && f.h >= rh {
                let top = f.y + rh;
                if top < b.s1 || (top == b.s1 && f.x < b.s2) {
                    b = Node { x: f.x, y: f.y, w: rw, h: rh, s1: top, s2: f.x, rotated: true };
                }
            }
        }
        b
    }

    fn bssf(&self, w: i32, h: i32, rw: i32, rh: i32, rot: bool) -> Node {
        let mut b = Node::empty();
        b.s1 = i32::MAX;
        for f in &self.free {
            if f.w >= w && f.h >= h {
                let (lh, lv) = ((f.w - w).abs(), (f.h - h).abs());
                let (short, long) = (lh.min(lv), lh.max(lv));
                if short < b.s1 || (short == b.s1 && long < b.s2) {
                    b = Node { x: f.x, y: f.y, w, h, s1: short, s2: long, rotated: false };
                }
            }
            if rot && f.w >= rw && f.h >= rh {
                let (lh, lv) = ((f.w - rw).abs(), (f.h - rh).abs());
                let (short, long) = (lh.min(lv), lh.max(lv));
                if short < b.s1 || (short == b.s1 && long < b.s2) {
                    b = Node { x: f.x, y: f.y, w: rw, h: rh, s1: short, s2: long, rotated: true };
                }
            }
        }
        b
    }

    fn blsf(&self, w: i32, h: i32, rw: i32, rh: i32, rot: bool) -> Node {
        let mut b = Node::empty();
        b.s2 = i32::MAX;
        for f in &self.free {
            if f.w >= w && f.h >= h {
                let (lh, lv) = ((f.w - w).abs(), (f.h - h).abs());
                let (short, long) = (lh.min(lv), lh.max(lv));
                if long < b.s2 || (long == b.s2 && short < b.s1) {
                    b = Node { x: f.x, y: f.y, w, h, s1: short, s2: long, rotated: false };
                }
            }
            if rot && f.w >= rw && f.h >= rh {
                let (lh, lv) = ((f.w - rw).abs(), (f.h - rh).abs());
                let (short, long) = (lh.min(lv), lh.max(lv));
                if long < b.s2 || (long == b.s2 && short < b.s1) {
                    b = Node { x: f.x, y: f.y, w: rw, h: rh, s1: short, s2: long, rotated: true };
                }
            }
        }
        b
    }

    fn baf(&self, w: i32, h: i32, rw: i32, rh: i32, rot: bool) -> Node {
        let mut b = Node::empty();
        b.s1 = i32::MAX;
        for f in &self.free {
            // 돌린 경우에도 넓이 여유는 돌리기 전 크기로 잰다(libGDX 와 같다)
            let area = (f.w as i64 * f.h as i64 - w as i64 * h as i64).clamp(i32::MIN as i64, i32::MAX as i64) as i32;
            if f.w >= w && f.h >= h {
                let short = (f.w - w).abs().min((f.h - h).abs());
                if area < b.s1 || (area == b.s1 && short < b.s2) {
                    b = Node { x: f.x, y: f.y, w, h, s1: area, s2: short, rotated: false };
                }
            }
            if rot && f.w >= rw && f.h >= rh {
                let short = (f.w - rw).abs().min((f.h - rh).abs());
                if area < b.s1 || (area == b.s1 && short < b.s2) {
                    b = Node { x: f.x, y: f.y, w: rw, h: rh, s1: area, s2: short, rotated: true };
                }
            }
        }
        b
    }

    fn contact(&self, x: i32, y: i32, w: i32, h: i32) -> i32 {
        let mut score = 0;
        if x == 0 || x + w == self.w {
            score += h;
        }
        if y == 0 || y + h == self.h {
            score += w;
        }
        for r in &self.used {
            if r.x == x + w || r.x + r.w == x {
                score += common(r.y, r.y + r.h, y, y + h);
            }
            if r.y == y + h || r.y + r.h == y {
                score += common(r.x, r.x + r.w, x, x + w);
            }
        }
        score
    }

    fn cp(&self, w: i32, h: i32, rw: i32, rh: i32, rot: bool) -> Node {
        let mut b = Node::empty();
        b.s1 = -1;
        for f in &self.free {
            if f.w >= w && f.h >= h {
                let s = self.contact(f.x, f.y, w, h);
                if s > b.s1 {
                    b = Node { x: f.x, y: f.y, w, h, s1: s, s2: 0, rotated: false };
                }
            }
            if rot && f.w >= rw && f.h >= rh {
                let s = self.contact(f.x, f.y, rw, rh);
                if s > b.s1 {
                    b = Node { x: f.x, y: f.y, w: rw, h: rh, s1: s, s2: 0, rotated: true };
                }
            }
        }
        b
    }

    fn place(&mut self, n: &Node) {
        let u = R { x: n.x, y: n.y, w: n.w, h: n.h };
        let mut i = 0;
        let mut count = self.free.len();
        while i < count {
            if self.split(i, u) {
                self.free.remove(i);
                count -= 1;
            } else {
                i += 1;
            }
        }
        self.prune();
        self.used.push(u);
    }

    /// 겹치면 빈칸을 쪼개 뒤에 붙이고 true.
    fn split(&mut self, i: usize, u: R) -> bool {
        let f = self.free[i];
        if u.x >= f.x + f.w || u.x + u.w <= f.x || u.y >= f.y + f.h || u.y + u.h <= f.y {
            return false;
        }
        if u.x < f.x + f.w && u.x + u.w > f.x {
            if u.y > f.y && u.y < f.y + f.h {
                self.free.push(R { h: u.y - f.y, ..f });
            }
            if u.y + u.h < f.y + f.h {
                self.free.push(R { y: u.y + u.h, h: f.y + f.h - (u.y + u.h), ..f });
            }
        }
        if u.y < f.y + f.h && u.y + u.h > f.y {
            if u.x > f.x && u.x < f.x + f.w {
                self.free.push(R { w: u.x - f.x, ..f });
            }
            if u.x + u.w < f.x + f.w {
                self.free.push(R { x: u.x + u.w, w: f.x + f.w - (u.x + u.w), ..f });
            }
        }
        true
    }

    fn prune(&mut self) {
        let free = &mut self.free;
        let mut i = 0;
        while i < free.len() {
            let mut j = i + 1;
            let mut removed_i = false;
            while j < free.len() {
                let (a, b) = (free[i], free[j]);
                if inside(a, b) {
                    free.remove(i);
                    removed_i = true;
                    break;
                }
                if inside(b, a) {
                    free.remove(j);
                } else {
                    j += 1;
                }
            }
            if !removed_i {
                i += 1;
            }
        }
    }

    fn result(&self) -> (i32, i32) {
        let mut w = 0;
        let mut h = 0;
        for r in &self.used {
            w = w.max(r.x + r.w);
            h = h.max(r.y + r.h);
        }
        (w, h)
    }

    /// Fast: 정한 순서대로 넣고, 못 넣으면 그 뒤는 모두 남긴다.
    fn insert_all(&mut self, items: &[Item], m: Method) -> Trial {
        let mut out = Vec::new();
        let mut remaining = Vec::new();
        for (k, it) in items.iter().enumerate() {
            let n = self.score(it, m);
            if n.h == 0 {
                remaining.extend_from_slice(&items[k..]);
                break;
            }
            self.place(&n);
            out.push((it.idx, n));
        }
        let (w, h) = self.result();
        Trial { out, remaining, w, h }
    }

    /// 매번 가장 잘 맞는 영역을 골라 넣는다.
    fn pack_best(&mut self, items: &[Item], m: Method) -> Trial {
        let mut rest: Vec<Item> = items.to_vec();
        let mut out = Vec::new();
        while !rest.is_empty() {
            let mut best: Option<(usize, Node)> = None;
            for (i, it) in rest.iter().enumerate() {
                let n = self.score(it, m);
                let better = match &best {
                    None => n.s1 < i32::MAX || n.s2 < i32::MAX,
                    Some((_, b)) => n.s1 < b.s1 || (n.s1 == b.s1 && n.s2 < b.s2),
                };
                if better {
                    best = Some((i, n));
                }
            }
            let Some((i, n)) = best else { break };
            self.place(&n);
            out.push((rest[i].idx, n));
            rest.remove(i);
        }
        let (w, h) = self.result();
        Trial { out, remaining: rest, w, h }
    }
}

// ---------------------------------------------------------------- 빠른 계산
//
// `Bin` 을 그대로 따르는 계산은 영역이 많으면(1천 개 넘게) 매우 느리다(Spine 도 Cat 에 20분 넘게 걸린다).
// `Quick` 은 같은 결과를 낸다(테스트에서 `Bin` 과 대조한다):
// - 영역마다 가장 좋은 자리를 기억해 두고, 놓을 때마다 바뀐 빈칸(사라진 것 · 새로 생긴 것)만 다시 본다.
//   빈칸 목록의 차례는 만든 순서와 같으므로(새 빈칸은 끝에 붙고 지울 때 차례는 그대로) 빈칸 번호로 동점을 가른다.
// - Contact point 는 맞닿은 길이를 가장자리 좌표로 찾은 이웃끼리만 더하고, 새 영역에 닿는 빈칸만 다시 잰다.
// - 모두 넣어야 하는 시험은 남은 넓이가 빈 넓이보다 크면 바로 그만둔다(결과를 쓰지 않는다).

use rayon::prelude::*;
use std::collections::HashMap;
use std::hash::{BuildHasherDefault, Hasher};

/// 좌표(i32) 열쇠용 가벼운 해시.
#[derive(Default)]
struct CoordHasher(u64);

impl Hasher for CoordHasher {
    fn finish(&self) -> u64 {
        self.0
    }
    fn write(&mut self, bytes: &[u8]) {
        for &b in bytes {
            self.0 = (self.0 ^ b as u64).wrapping_mul(0x100000001b3);
        }
    }
    fn write_i32(&mut self, v: i32) {
        self.0 = (v as u32 as u64).wrapping_mul(0x9E3779B97F4A7C15);
    }
}

type Map<V> = HashMap<i32, V, BuildHasherDefault<CoordHasher>>;

#[derive(Clone, Copy)]
struct Cache {
    has: bool,
    key: (i64, i64),
    fid: u32,
    rot: bool,
    node: Node,
}

impl Cache {
    const NONE: Cache = Cache { has: false, key: (0, 0), fid: 0, rot: false, node: Node { x: 0, y: 0, w: 0, h: 0, s1: i32::MAX, s2: i32::MAX, rotated: false } };

    fn pos(&self) -> (u32, bool) {
        (self.fid, self.rot)
    }
}

struct Quick {
    w: i32,
    h: i32,
    rotation: bool,
    px: i32,
    py: i32,
    free: Vec<R>,
    ids: Vec<u32>,
    next_id: u32,
    used_area: i64,
    left: Map<Vec<R>>,
    right: Map<Vec<R>>,
    bottom: Map<Vec<R>>,
    top: Map<Vec<R>>,
    used: Vec<R>,
    removed: Vec<u32>,
}

impl Quick {
    fn new(w: i32, h: i32, s: &Settings) -> Quick {
        Quick {
            w,
            h,
            rotation: s.rotation,
            px: s.px,
            py: s.py,
            free: vec![R { x: 0, y: 0, w, h }],
            ids: vec![0],
            next_id: 1,
            used_area: 0,
            left: Map::default(),
            right: Map::default(),
            bottom: Map::default(),
            top: Map::default(),
            used: Vec::new(),
            removed: Vec::new(),
        }
    }

    fn contact(&self, x: i32, y: i32, w: i32, h: i32) -> i32 {
        let mut score = 0;
        if x == 0 || x + w == self.w {
            score += h;
        }
        if y == 0 || y + h == self.h {
            score += w;
        }
        for map_key in [(&self.left, x + w), (&self.right, x)] {
            if let Some(v) = map_key.0.get(&map_key.1) {
                for r in v {
                    score += common(r.y, r.y + r.h, y, y + h);
                }
            }
        }
        for map_key in [(&self.bottom, y + h), (&self.top, y)] {
            if let Some(v) = map_key.0.get(&map_key.1) {
                for r in v {
                    score += common(r.x, r.x + r.w, x, x + w);
                }
            }
        }
        score
    }

    /// 빈칸 하나에 한 방향으로 놓을 때의 (비교 키, 자리). 작을수록 좋다.
    fn cand(&self, m: Method, f: R, w: i32, h: i32, it: &Item, rotated: bool) -> Option<((i64, i64), Node)> {
        if f.w < w || f.h < h {
            return None;
        }
        let node = |s1: i32, s2: i32| Node { x: f.x, y: f.y, w, h, s1, s2, rotated };
        Some(match m {
            Method::BestShortSideFit => {
                let (lh, lv) = (f.w - w, f.h - h);
                let (short, long) = (lh.min(lv), lh.max(lv));
                ((short as i64, long as i64), node(short, long))
            }
            Method::BestLongSideFit => {
                let (lh, lv) = (f.w - w, f.h - h);
                let (short, long) = (lh.min(lv), lh.max(lv));
                ((long as i64, short as i64), node(short, long))
            }
            Method::BestAreaFit => {
                let area = (f.w as i64 * f.h as i64 - it.w as i64 * it.h as i64).clamp(i32::MIN as i64, i32::MAX as i64) as i32;
                let short = (f.w - w).min(f.h - h);
                ((area as i64, short as i64), node(area, short))
            }
            Method::BottomLeft => {
                let top = f.y + h;
                ((top as i64, f.x as i64), node(top, f.x))
            }
            Method::ContactPoint => {
                let c = self.contact(f.x, f.y, w, h);
                ((-(c as i64), 0), node(-c, 0))
            }
        })
    }

    fn dims(&self, it: &Item) -> (i32, i32, i32, i32) {
        (it.w, it.h, it.h - self.py + self.px, it.w - self.px + self.py)
    }

    /// 빈칸 i 의 두 방향을 지금 기억과 견준다.
    fn offer(&self, m: Method, i: usize, it: &Item, c: &mut Cache) {
        let (w, h, rw, rh) = self.dims(it);
        let f = self.free[i];
        let id = self.ids[i];
        for (cw, ch, rot) in [(w, h, false), (rw, rh, true)] {
            if rot && !self.rotation {
                continue;
            }
            if let Some((key, node)) = self.cand(m, f, cw, ch, it, rot) {
                if !c.has || key < c.key || (key == c.key && (id, rot) < c.pos()) {
                    *c = Cache { has: true, key, fid: id, rot, node };
                }
            }
        }
    }

    fn full(&self, m: Method, it: &Item) -> Cache {
        let mut c = Cache::NONE;
        for i in 0..self.free.len() {
            self.offer(m, i, it, &mut c);
        }
        c
    }

    /// 자리를 차지한다. 사라진 빈칸 번호는 `removed` 에, 새 빈칸은 번호 `start` 부터.
    fn place(&mut self, n: &Node) -> u32 {
        let u = R { x: n.x, y: n.y, w: n.w, h: n.h };
        let start = self.next_id;
        self.removed.clear();
        let mut i = 0;
        let mut count = self.free.len();
        while i < count {
            if self.split(i, u) {
                self.removed.push(self.ids[i]);
                self.free.remove(i);
                self.ids.remove(i);
                count -= 1;
            } else {
                i += 1;
            }
        }
        self.prune(start);
        self.used.push(u);
        self.used_area += u.w as i64 * u.h as i64;
        self.left.entry(u.x).or_default().push(u);
        self.right.entry(u.x + u.w).or_default().push(u);
        self.bottom.entry(u.y).or_default().push(u);
        self.top.entry(u.y + u.h).or_default().push(u);
        start
    }

    fn push_free(&mut self, r: R) {
        self.free.push(r);
        self.ids.push(self.next_id);
        self.next_id += 1;
    }

    fn split(&mut self, i: usize, u: R) -> bool {
        let f = self.free[i];
        if u.x >= f.x + f.w || u.x + u.w <= f.x || u.y >= f.y + f.h || u.y + u.h <= f.y {
            return false;
        }
        if u.x < f.x + f.w && u.x + u.w > f.x {
            if u.y > f.y && u.y < f.y + f.h {
                self.push_free(R { h: u.y - f.y, ..f });
            }
            if u.y + u.h < f.y + f.h {
                self.push_free(R { y: u.y + u.h, h: f.y + f.h - (u.y + u.h), ..f });
            }
        }
        if u.y < f.y + f.h && u.y + u.h > f.y {
            if u.x > f.x && u.x < f.x + f.w {
                self.push_free(R { w: u.x - f.x, ..f });
            }
            if u.x + u.w < f.x + f.w {
                self.push_free(R { x: u.x + u.w, w: f.x + f.w - (u.x + u.w), ..f });
            }
        }
        true
    }

    /// 예전 libGDX 의 빈칸 정리(모든 쌍을 앞에서부터 비교)와 같은 결과를 새 빈칸만 보고 낸다.
    /// 놓기 전 빈칸끼리는 서로 품지 않고, 새 빈칸은 옛 빈칸을 쪼갠 조각이라 옛 빈칸을 품지도 같지도 않다.
    /// 그래서 지워지는 것은 새 빈칸뿐이다: 다른 빈칸 안에 들어가거나(같은 크기면 뒤의 것이 남는다).
    fn prune(&mut self, start: u32) {
        let first_new = self.ids.partition_point(|&id| id < start);
        if first_new == self.free.len() {
            return;
        }
        let mut drop = vec![false; self.free.len() - first_new];
        for (k, d) in drop.iter_mut().enumerate() {
            let i = first_new + k;
            let a = self.free[i];
            *d = self.free.iter().enumerate().any(|(j, &b)| j != i && inside(a, b) && (a != b || j > i));
        }
        if drop.iter().any(|&d| d) {
            let mut k = 0;
            let (free, ids, removed) = (&mut self.free, &mut self.ids, &mut self.removed);
            let mut idx = 0;
            free.retain(|_| {
                let keep = idx < first_new || !drop[idx - first_new];
                idx += 1;
                keep
            });
            ids.retain(|&id| {
                let keep = k < first_new || !drop[k - first_new];
                if !keep {
                    removed.push(id);
                }
                k += 1;
                keep
            });
        }
    }

    fn result(&self) -> (i32, i32) {
        let mut w = 0;
        let mut h = 0;
        for r in &self.used {
            w = w.max(r.x + r.w);
            h = h.max(r.y + r.h);
        }
        (w, h)
    }

    /// 가장 작게 차지할 넓이(돌리면 여백 때문에 넓이가 달라질 수 있다).
    fn min_area(&self, it: &Item) -> i64 {
        let (w, h, rw, rh) = self.dims(it);
        let a = w as i64 * h as i64;
        if self.rotation {
            a.min(rw as i64 * rh as i64)
        } else {
            a
        }
    }

    fn hopeless(&self, rest_area: i64) -> bool {
        rest_area > self.w as i64 * self.h as i64 - self.used_area
    }

    /// Fast: 정한 순서대로 넣고, 못 넣으면 그 뒤는 모두 남긴다.
    fn insert_all(&mut self, items: &[Item], m: Method, fully: bool) -> Trial {
        let mut out = Vec::new();
        let mut rest_area: i64 = if fully { items.iter().map(|it| self.min_area(it)).sum() } else { 0 };
        for (k, it) in items.iter().enumerate() {
            if fully && self.hopeless(rest_area) {
                return Trial { out, remaining: items[k..].to_vec(), w: 0, h: 0 };
            }
            let c = self.full(m, it);
            if !c.has {
                let (w, h) = self.result();
                return Trial { out, remaining: items[k..].to_vec(), w, h };
            }
            self.place(&c.node);
            out.push((it.idx, c.node));
            if fully {
                rest_area -= self.min_area(it);
            }
        }
        let (w, h) = self.result();
        Trial { out, remaining: Vec::new(), w, h }
    }

    /// 매번 가장 잘 맞는 영역을 골라 넣는다.
    fn pack_best(&mut self, items: &[Item], m: Method, fully: bool) -> Trial {
        let mut rest: Vec<Item> = items.to_vec();
        let mut caches: Vec<Cache> = rest.iter().map(|it| self.full(m, it)).collect();
        let mut out = Vec::new();
        let mut rest_area: i64 = if fully { rest.iter().map(|it| self.min_area(it)).sum() } else { 0 };
        let mut by_x: Map<Vec<usize>> = Map::default();
        let mut by_y: Map<Vec<usize>> = Map::default();
        let mut gone: Vec<bool> = Vec::new();
        let mut cands: Vec<usize> = Vec::new();
        while !rest.is_empty() {
            if fully && self.hopeless(rest_area) {
                return Trial { out, remaining: rest, w: 0, h: 0 };
            }
            // 전역 선택: (s1, s2) 가 가장 작은 것, 같으면 앞의 것
            let mut best: Option<usize> = None;
            for (i, c) in caches.iter().enumerate() {
                if !c.has {
                    continue;
                }
                let better = match best {
                    None => true,
                    Some(b) => {
                        let (n, bn) = (&c.node, &caches[b].node);
                        n.s1 < bn.s1 || (n.s1 == bn.s1 && n.s2 < bn.s2)
                    }
                };
                if better {
                    best = Some(i);
                }
            }
            let Some(bi) = best else { break };
            let node = caches[bi].node;
            let it = rest.remove(bi);
            caches.remove(bi);
            out.push((it.idx, node));
            if fully {
                rest_area -= self.min_area(&it);
            }
            let start = self.place(&node);
            if gone.len() < self.next_id as usize {
                gone.resize(self.next_id as usize, false);
            }
            for &id in &self.removed {
                gone[id as usize] = true;
            }
            let first_new = self.ids.partition_point(|&id| id < start);
            let u = R { x: node.x, y: node.y, w: node.w, h: node.h };
            if m == Method::ContactPoint {
                by_x.clear();
                by_y.clear();
                for (i, f) in self.free.iter().enumerate() {
                    by_x.entry(f.x).or_default().push(i);
                    by_y.entry(f.y).or_default().push(i);
                }
            }
            for (k, c) in caches.iter_mut().enumerate() {
                if !c.has {
                    continue; // 새 빈칸은 옛 빈칸 안에 있으므로 들어갈 곳이 생기지 않는다
                }
                let it = &rest[k];
                if gone[c.fid as usize] {
                    *c = self.full(m, it);
                    continue;
                }
                if m == Method::ContactPoint {
                    // 지금 자리의 점수도 늘었을 수 있다
                    let bi = self.ids.partition_point(|&id| id < c.fid);
                    let (w, h) = (c.node.w, c.node.h);
                    let f = self.free[bi];
                    let (key, node) = self.cand(m, f, w, h, it, c.rot).expect("기억한 자리는 그대로 맞는다");
                    c.key = key;
                    c.node = node;
                    let (w, h, rw, rh) = self.dims(it);
                    cands.clear();
                    for (cw, ch) in [(w, h), (rw, rh)] {
                        for (map, k2) in [(&by_x, u.x + u.w), (&by_x, u.x - cw), (&by_y, u.y + u.h), (&by_y, u.y - ch)] {
                            if let Some(v) = map.get(&k2) {
                                cands.extend(v.iter().copied().filter(|&i| i < first_new));
                            }
                        }
                    }
                    for &i in &cands {
                        self.offer(m, i, it, c);
                    }
                }
                for i in first_new..self.free.len() {
                    self.offer(m, i, it, c);
                }
            }
        }
        let (w, h) = self.result();
        Trial { out, remaining: rest, w, h }
    }
}

fn common(a0: i32, a1: i32, b0: i32, b1: i32) -> i32 {
    if a1 < b0 || b1 < a0 {
        0
    } else {
        a1.min(b1) - a0.max(b0)
    }
}

fn inside(a: R, b: R) -> bool {
    a.x >= b.x && a.y >= b.y && a.x + a.w <= b.x + b.w && a.y + a.h <= b.y + b.h
}

/// 가장자리 여백(이미지 둘레에 더하는 것). Duplicate padding 이면 반.
fn edge(s: &Settings) -> (i32, i32) {
    if !s.edge_padding {
        return (0, 0);
    }
    if s.duplicate_padding {
        (s.px / 2, s.py / 2)
    } else {
        (s.px, s.py)
    }
}

/// 페이지 이미지 크기: 배치 크기(여백을 뺀 것) + 가장자리 여백, 2의 거듭제곱 · 4의 배수 · 최소 크기.
fn image_size(page_w: i32, page_h: i32, s: &Settings) -> (i32, i32) {
    let (ex, ey) = edge(s);
    let (mut w, mut h) = (page_w + ex * 2, page_h + ey * 2);
    if s.pot {
        w = next_pot(w);
        h = next_pot(h);
    }
    if s.mod4 {
        w = round4(w);
        h = round4(h);
    }
    (w.max(s.min_w), h.max(s.min_h))
}

/// 규칙 사이: 이미지가 작은 쪽, 같으면 뒤 것.
fn image_area(t: &Trial, s: &Settings) -> i64 {
    let (w, h) = if s.square {
        let m = t.w.max(t.h);
        (m - s.px, m - s.py)
    } else {
        (t.w - s.px, t.h - s.py)
    };
    let (w, h) = image_size(w, h, s);
    w as i64 * h as i64
}

fn better_method(a: Option<Trial>, b: Trial, s: &Settings) -> Trial {
    match a {
        None => b,
        Some(a) => {
            if image_area(&a, s) < image_area(&b, s) {
                a
            } else {
                b
            }
        }
    }
}

/// 크기 사이: 실제로 쓴 넓이가 작은 쪽, 같으면 나중 것.
fn better_size(a: Option<Trial>, b: Option<Trial>) -> Option<Trial> {
    match (a, b) {
        (None, b) => b,
        (a, None) => a,
        (Some(a), Some(b)) => {
            if (a.w as i64 * a.h as i64) < (b.w as i64 * b.h as i64) {
                Some(a)
            } else {
                Some(b)
            }
        }
    }
}

fn pack_at(fully: bool, w: i32, h: i32, items: &[Item], s: &Settings) -> Option<Trial> {
    let methods: &[Method] = if s.fast { &FAST } else { &ALL };
    let mut best: Option<Trial> = None;
    let run = |m: Method| {
        let mut q = Quick::new(w, h, s);
        if s.fast {
            q.insert_all(items, m, fully)
        } else {
            q.pack_best(items, m, fully)
        }
    };
    let trials: Vec<Trial> = if methods.len() > 1 { methods.par_iter().map(|&m| run(m)).collect() } else { methods.iter().map(|&m| run(m)).collect() };
    for t in trials {
        if fully && !t.remaining.is_empty() {
            continue;
        }
        if t.out.is_empty() {
            continue;
        }
        best = Some(better_method(best, t, s));
    }
    best
}

/// 한 페이지. Err 는 최대 크기에 들어가지 않는 영역.
fn pack_page(items: &[Item], s: &Settings) -> Result<(Trial, i32, i32), usize> {
    let (px, py) = (s.px, s.py);
    let (mut max_w, mut max_h) = (s.max_w as f32, s.max_h as f32);
    if s.edge_padding {
        if s.duplicate_padding {
            max_w -= px as f32;
            max_h -= py as f32;
        } else {
            max_w -= (px * 2) as f32;
            max_h -= (py * 2) as f32;
        }
    }
    let (mut min_w, mut min_h) = (i32::MAX, i32::MAX);
    for it in items {
        let (w, h) = (it.w - px, it.h - py);
        min_w = min_w.min(w);
        min_h = min_h.min(h);
        let (wf, hf) = (w as f32, h as f32);
        let too_big = if s.rotation { (wf > max_w || hf > max_h) && (wf > max_h || hf > max_w) } else { wf > max_w || hf > max_h };
        if too_big {
            return Err(it.idx);
        }
    }
    min_w = min_w.max(s.min_w);
    min_h = min_h.max(s.min_h);
    let (mut adj_x, mut adj_y) = (px, py);
    if s.edge_padding {
        if s.duplicate_padding {
            adj_x -= px;
            adj_y -= py;
        } else {
            adj_x -= px * 2;
            adj_y -= py * 2;
        }
    }
    let fuzz = if s.fast { 25 } else { 15 };
    let mut best: Option<Trial> = None;
    if s.square {
        let min_size = min_w.max(min_h);
        let max_size = s.max_w.min(s.max_h);
        let mut search = Search::new(min_size, max_size, fuzz, s.pot, s.mod4);
        let mut size = search.reset();
        while size != -1 {
            let t = pack_at(true, size + adj_x, size + adj_y, items, s);
            let failed = t.is_none();
            best = better_size(best, t);
            size = search.next(failed);
        }
        let mut t = match best {
            Some(t) => t,
            None => pack_at(false, max_size + adj_x, max_size + adj_y, items, s).ok_or(items[0].idx)?,
        };
        let m = t.w.max(t.h);
        t.w = m;
        t.h = m;
        return Ok((t.clone(), m - px, m - py));
    }
    let mut ws = Search::new(min_w, s.max_w, fuzz, s.pot, s.mod4);
    let mut hs = Search::new(min_h, s.max_h, fuzz, s.pot, s.mod4);
    let mut width = ws.reset();
    let mut height = hs.reset();
    loop {
        let mut best_w: Option<Trial> = None;
        while width != -1 {
            let t = pack_at(true, width + adj_x, height + adj_y, items, s);
            let failed = t.is_none();
            best_w = better_size(best_w, t);
            width = ws.next(failed);
        }
        let failed = best_w.is_none();
        best = better_size(best, best_w);
        height = hs.next(failed);
        if height == -1 {
            break;
        }
        width = ws.reset();
    }
    let t = match best {
        Some(t) => t,
        None => pack_at(false, s.max_w + adj_x, s.max_h + adj_y, items, s).ok_or(items[0].idx)?,
    };
    let (w, h) = (t.w - px, t.h - py);
    Ok((t, w, h))
}

/// 영역들(돌리기 전 크기, 이름 순)을 페이지로 묶는다. Err 는 들어가지 않는 영역.
pub(crate) fn pack(sizes: &[(usize, i32, i32)], s: &Settings) -> Result<Vec<PageOut>, usize> {
    let mut items: Vec<Item> = sizes.iter().map(|&(idx, w, h)| Item { idx, w: w + s.px, h: h + s.py }).collect();
    if s.fast {
        if s.rotation {
            items.sort_by_key(|a| std::cmp::Reverse(a.w.max(a.h))); // 안정 정렬: 같으면 이름 순
        } else {
            items.sort_by_key(|a| std::cmp::Reverse(a.w));
        }
    }
    let (ex, ey) = edge(s);
    let mut pages = Vec::new();
    while !items.is_empty() {
        let (t, page_w, page_h) = pack_page(&items, s)?;
        if t.out.is_empty() {
            return Err(items[0].idx);
        }
        let (w, h) = image_size(page_w, page_h, s);
        let placed = t
            .out
            .iter()
            .map(|&(idx, n)| Placed { idx, x: ex + n.x, y: ey + page_h - n.y - (n.h - s.py), rotated: n.rotated })
            .collect();
        pages.push(PageOut { w, h, placed });
        items = t.remaining;
    }
    Ok(pages)
}

/// 격자 배치(libGDX 의 GridPacker 와 같다): 가장 큰 영역 크기의 칸에 이름 순으로 왼쪽→오른쪽, 위→아래.
/// 돌리지 않고, Square 는 쓰지 않는다(Spine 도 격자 페이지는 정사각형으로 맞추지 않는다).
pub(crate) fn pack_grid(sizes: &[(usize, i32, i32)], s: &Settings) -> Result<Vec<PageOut>, usize> {
    let (px, py) = (s.px, s.py);
    let (mut adj_x, mut adj_y) = (px, py);
    if s.edge_padding {
        if s.duplicate_padding {
            adj_x -= px;
            adj_y -= py;
        } else {
            adj_x -= px * 2;
            adj_y -= py * 2;
        }
    }
    let (max_w, max_h) = (s.max_w + adj_x, s.max_h + adj_y);
    let cell_w = sizes.iter().map(|v| v.1).max().unwrap_or(0) + px;
    let cell_h = sizes.iter().map(|v| v.2).max().unwrap_or(0) + py;
    let (ex, ey) = edge(s);
    let mut pages = Vec::new();
    let mut k = 0;
    while k < sizes.len() {
        let (mut x, mut y, mut pw, mut ph) = (0, 0, 0, 0);
        let mut spots = Vec::new();
        while k < sizes.len() {
            if x + cell_w > max_w {
                y += cell_h;
                if y > max_h - cell_h {
                    break;
                }
                x = 0;
            }
            spots.push((sizes[k], x, y));
            k += 1;
            x += cell_w;
            pw = pw.max(x);
            ph = ph.max(y + cell_h);
        }
        if spots.is_empty() {
            return Err(sizes[k].0);
        }
        let (page_w, page_h) = (pw - px, ph - py);
        let placed = spots
            .iter()
            .map(|&((idx, _, h), x, y)| {
                // 위 줄부터 채우도록 뒤집은 뒤(libGDX) atlas 의 위 기준으로 다시 뒤집는다
                let ry = ph - y - (h + py);
                Placed { idx, x: ex + x, y: ey + page_h - ry - h, rotated: false }
            })
            .collect();
        let (w, h) = image_size(page_w, page_h, s);
        pages.push(PageOut { w, h, placed });
    }
    Ok(pages)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn items(seed: u64, n: usize, lo: i32, hi: i32, pad: (i32, i32)) -> Vec<Item> {
        let mut x = seed.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        let mut next = move |m: i32| {
            x = x.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
            ((x >> 33) % m as u64) as i32
        };
        (0..n).map(|idx| Item { idx, w: lo + next(hi - lo) + pad.0, h: lo + next(hi - lo) + pad.1 }).collect()
    }

    fn settings(rotation: bool, px: i32, py: i32) -> Settings {
        Settings { px, py, max_w: 4096, max_h: 4096, min_w: 0, min_h: 0, edge_padding: true, duplicate_padding: false, rotation, fast: false, square: false, pot: false, mod4: false }
    }

    fn same(a: &Trial, b: &Trial) -> bool {
        let key = |t: &Trial| t.out.iter().map(|(i, n)| (*i, n.x, n.y, n.w, n.h, n.rotated)).collect::<Vec<_>>();
        key(a) == key(b) && a.remaining.iter().map(|i| i.idx).eq(b.remaining.iter().map(|i| i.idx)) && (a.w, a.h) == (b.w, b.h)
    }

    #[test]
    fn quick_matches_libgdx_port() {
        // 빠른 계산은 libGDX 를 그대로 옮긴 계산과 자리·순서·남은 것까지 같아야 한다
        let mut checked = 0;
        for seed in 0..40u64 {
            for &(rotation, px, py) in &[(false, 2, 2), (true, 2, 2), (true, 3, 1), (false, 0, 0)] {
                let s = settings(rotation, px, py);
                let n = 10 + (seed as usize % 5) * 12;
                let its = items(seed, n, 3 + (seed % 7) as i32, 40 + (seed % 5) as i32 * 30, (px, py));
                let area: i64 = its.iter().map(|i| i.w as i64 * i.h as i64).sum();
                let side = ((area as f64).sqrt() * (0.9 + (seed % 4) as f64 * 0.15)) as i32;
                for &(w, h) in &[(side, side), (side * 3 / 2, side * 2 / 3), (side / 2, side * 2)] {
                    for m in ALL {
                        let a = Bin::new(w, h, &s).pack_best(&its, m);
                        let b = Quick::new(w, h, &s).pack_best(&its, m, false);
                        assert!(same(&a, &b), "pack_best {m:?} seed {seed} rot {rotation} {w}x{h}");
                        checked += 1;
                    }
                    let a = Bin::new(w, h, &s).insert_all(&its, Method::ContactPoint);
                    let b = Quick::new(w, h, &s).insert_all(&its, Method::ContactPoint, false);
                    assert!(same(&a, &b), "insert_all seed {seed} rot {rotation} {w}x{h}");
                }
            }
        }
        assert!(checked > 2000);
    }

    #[test]
    fn search_like_libgdx() {
        // 16 ~ 512, 허용 폭 25: 264 → (성공) 139 → (실패) 201 → (실패) 232 → (성공) 216 → (실패) 끝
        let mut s = Search::new(16, 512, 25, false, false);
        assert_eq!(s.reset(), 264);
        assert_eq!(s.next(false), 139);
        assert_eq!(s.next(true), 201);
        assert_eq!(s.next(true), 232);
        assert_eq!(s.next(false), 216);
        assert_eq!(s.next(true), -1);
        let mut p = Search::new(100, 600, 15, true, false);
        assert_eq!(p.reset(), 256);
        assert_eq!(p.next(true), 512);
    }

    fn spine_base() -> Settings {
        Settings { px: 2, py: 2, max_w: 512, max_h: 512, min_w: 16, min_h: 16, edge_padding: true, duplicate_padding: false, rotation: false, fast: true, square: false, pot: false, mod4: false }
    }

    /// 크기는 이름 순, 기대값은 (페이지, x, y, 회전).
    fn check(s: &Settings, sizes: &[(i32, i32)], expect: &[(usize, i32, i32, bool)], pages: &[(i32, i32)]) {
        let input: Vec<(usize, i32, i32)> = sizes.iter().enumerate().map(|(i, &(w, h))| (i, w, h)).collect();
        let out = pack(&input, s).unwrap();
        assert_eq!(out.iter().map(|p| (p.w, p.h)).collect::<Vec<_>>(), pages);
        let mut got = vec![(0, 0, 0, false); sizes.len()];
        for (pi, p) in out.iter().enumerate() {
            for q in &p.placed {
                got[q.idx] = (pi, q.x, q.y, q.rotated);
            }
        }
        assert_eq!(got, expect);
    }

    // Spine 4.2 명령줄 패커가 낸 결과(합성 이미지, 최대 512, 여백 2, 가장자리 여백, 최소 16). (페이지, x, y, 회전)
    #[test]
    fn spine_fast_pages() {
        let s = Settings { fast: true, ..spine_base() };
        check(&s, &[(114, 238), (160, 103), (160, 182), (287, 216), (169, 239), (143, 219), (146, 67), (100, 133), (266, 58), (293, 209), (105, 206), (156, 181), (47, 114), (123, 31), (107, 219)], &[(1, 309, 93, false), (0, 291, 145, false), (1, 2, 218, false), (0, 2, 62, false), (0, 297, 250, false), (1, 164, 112, false), (1, 164, 333, false), (2, 218, 118, false), (0, 2, 2, false), (0, 2, 280, false), (2, 111, 45, false), (1, 2, 35, false), (2, 218, 2, false), (1, 2, 2, false), (2, 2, 32, false)], &[(468, 491), (425, 402), (320, 253)]);
    }

    #[test]
    fn spine_slow() {
        let s = Settings { fast: false, ..spine_base() };
        check(&s, &[(41, 84), (5, 87), (34, 59), (73, 50), (62, 71), (10, 29), (4, 5), (46, 33), (19, 36), (65, 48), (55, 90), (38, 4), (33, 8), (39, 19), (17, 86), (48, 59), (84, 56), (61, 85), (31, 24), (90, 60), (5, 18), (12, 9), (80, 38), (20, 18), (27, 4)], &[(0, 2, 23, false), (0, 78, 112, false), (0, 131, 23, false), (0, 91, 124, false), (0, 157, 190, false), (0, 59, 82, false), (0, 85, 174, false), (0, 167, 44, false), (0, 221, 225, false), (0, 166, 140, false), (0, 2, 109, false), (0, 65, 8, false), (0, 65, 14, false), (0, 2, 2, false), (0, 59, 113, false), (0, 186, 79, false), (0, 45, 24, false), (0, 94, 176, false), (0, 71, 86, false), (0, 2, 201, false), (0, 85, 181, false), (0, 45, 98, false), (0, 104, 84, false), (0, 43, 3, false), (0, 65, 2, false)], &[(242, 263)]);
    }

    #[test]
    fn spine_slow_rotation() {
        let s = Settings { fast: false, rotation: true, ..spine_base() };
        check(&s, &[(152, 47), (69, 77), (93, 138), (79, 47), (38, 45), (50, 27), (13, 103), (103, 81), (55, 43), (158, 57), (148, 122), (32, 84), (154, 31), (43, 21), (23, 148), (9, 69), (100, 153), (16, 69), (113, 123), (64, 81), (32, 79), (116, 61), (46, 50), (159, 102), (120, 120), (96, 101), (49, 30), (139, 148), (8, 105), (77, 102), (67, 115), (126, 63), (11, 51), (76, 109), (69, 119), (30, 123), (92, 17), (107, 33), (24, 16), (99, 27)], &[(0, 2, 350, false), (0, 2, 279, true), (0, 210, 117, true), (0, 158, 375, false), (0, 213, 424, true), (0, 96, 464, false), (0, 233, 489, true), (0, 2, 148, false), (0, 369, 402, false), (0, 81, 231, false), (1, 143, 2, true), (0, 2, 441, true), (0, 2, 399, false), (0, 435, 481, false), (0, 148, 464, true), (0, 109, 493, true), (0, 350, 82, true), (0, 338, 486, true), (0, 163, 2, true), (0, 156, 309, true), (0, 399, 447, true), (0, 239, 359, false), (0, 426, 399, true), (0, 2, 29, false), (1, 267, 30, false), (0, 107, 133, true), (0, 480, 453, true), (1, 2, 2, false), (0, 2, 494, true), (0, 352, 184, true), (0, 239, 290, true), (0, 357, 334, false), (0, 180, 491, true), (0, 241, 212, true), (0, 356, 263, true), (0, 88, 432, true), (0, 2, 475, false), (0, 260, 422, false), (0, 409, 486, false), (0, 298, 457, false)], &[(512, 504), (389, 152)]);
    }

    #[test]
    fn spine_slow_pot() {
        let s = Settings { fast: false, pot: true, ..spine_base() };
        check(&s, &[(152, 47), (69, 77), (93, 138), (79, 47), (38, 45), (50, 27), (13, 103), (103, 81), (55, 43), (158, 57), (148, 122), (32, 84), (154, 31), (43, 21), (23, 148), (9, 69), (100, 153), (16, 69), (113, 123), (64, 81), (32, 79), (116, 61), (46, 50), (159, 102), (120, 120), (96, 101), (49, 30), (139, 148), (8, 105), (77, 102), (67, 115), (126, 63), (11, 51), (76, 109), (69, 119), (30, 123), (92, 17), (107, 33), (24, 16), (99, 27)], &[(0, 265, 300, false), (0, 244, 3, false), (0, 2, 67, false), (0, 419, 300, false), (0, 372, 15, false), (0, 460, 51, false), (0, 488, 407, false), (0, 315, 81, false), (0, 315, 17, false), (0, 293, 349, false), (0, 143, 388, false), (0, 454, 426, false), (0, 265, 267, false), (0, 460, 28, false), (0, 104, 212, false), (0, 129, 291, false), (0, 2, 207, false), (0, 489, 229, false), (0, 97, 78, false), (0, 413, 184, false), (1, 149, 38, false), (0, 129, 203, false), (0, 412, 28, false), (0, 293, 408, false), (0, 143, 266, false), (0, 315, 164, false), (0, 421, 268, false), (0, 2, 362, false), (0, 479, 193, false), (0, 420, 80, false), (1, 80, 2, false), (0, 2, 2, false), (0, 499, 176, false), (1, 2, 8, false), (0, 244, 82, false), (0, 212, 78, false), (0, 315, 62, false), (0, 130, 43, false), (0, 265, 370, false), (0, 130, 14, false)], &[(512, 512), (256, 128)]);
    }
}
