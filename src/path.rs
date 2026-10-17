//! Pathfinding on a tile grid: A* moving 8 ways without cutting corners,
//! and straightening the result into waypoints a box can travel between.
//! What's passable is up to the caller (e.g. what the player knows).

use std::cmp::Reverse;
use std::collections::{BinaryHeap, HashMap};

use crate::layer::TILE;

const STRAIGHT: u32 = 10;
const DIAGONAL: u32 = 14;

/// The 8 neighbors of `t` reachable in one step: diagonals only when both
/// tiles beside the step are passable too, so paths never cut a corner.
fn steps(passable: &impl Fn([i32; 2]) -> bool, t: [i32; 2]) -> Vec<([i32; 2], u32)> {
    let mut out = Vec::with_capacity(8);
    for dy in -1..=1 {
        for dx in -1..=1 {
            if (dx, dy) == (0, 0) {
                continue;
            }
            let n = [t[0] + dx, t[1] + dy];
            if !passable(n) {
                continue;
            }
            if dx != 0 && dy != 0 {
                if passable([t[0] + dx, t[1]]) && passable([t[0], t[1] + dy]) {
                    out.push((n, DIAGONAL));
                }
            } else {
                out.push((n, STRAIGHT));
            }
        }
    }
    out
}

/// Octile distance, consistent with the step costs.
fn estimate(a: [i32; 2], b: [i32; 2]) -> u32 {
    let (dx, dy) = ((a[0] - b[0]).unsigned_abs(), (a[1] - b[1]).unsigned_abs());
    STRAIGHT * dx.max(dy) + (DIAGONAL - STRAIGHT) * dx.min(dy)
}

/// The shortest path of tiles from `start` to `goal`, both included. The
/// start needn't be passable (the walker is already there).
pub fn find(
    passable: impl Fn([i32; 2]) -> bool,
    start: [i32; 2],
    goal: [i32; 2],
) -> Option<Vec<[i32; 2]>> {
    if start != goal && !passable(goal) {
        return None;
    }
    let mut open = BinaryHeap::new();
    let mut cost: HashMap<[i32; 2], u32> = HashMap::new();
    let mut came_from: HashMap<[i32; 2], [i32; 2]> = HashMap::new();
    cost.insert(start, 0);
    open.push(Reverse((estimate(start, goal), start)));
    while let Some(Reverse((_, t))) = open.pop() {
        if t == goal {
            let mut path = vec![t];
            while let Some(&prev) = came_from.get(path.last().unwrap()) {
                path.push(prev);
            }
            path.reverse();
            return Some(path);
        }
        let here = cost[&t];
        for (n, step) in steps(&passable, t) {
            let c = here + step;
            if cost.get(&n).is_none_or(|&old| c < old) {
                cost.insert(n, c);
                came_from.insert(n, t);
                open.push(Reverse((c + estimate(n, goal), n)));
            }
        }
    }
    None
}

/// Of the tiles reachable from `start`, the one nearest `goal` (by
/// straight-line distance), for a goal that can't itself be reached.
pub fn nearest_reachable(
    passable: impl Fn([i32; 2]) -> bool,
    start: [i32; 2],
    goal: [i32; 2],
) -> Option<[i32; 2]> {
    let mut seen = vec![start];
    let mut stack = vec![start];
    let mut seen_set: std::collections::HashSet<[i32; 2]> = [start].into();
    while let Some(t) = stack.pop() {
        for (n, _) in steps(&passable, t) {
            if seen_set.insert(n) {
                seen.push(n);
                stack.push(n);
            }
        }
    }
    let d2 = |t: [i32; 2]| (t[0] - goal[0]).pow(2) + (t[1] - goal[1]).pow(2);
    seen.into_iter()
        .filter(|t| *t != start)
        .min_by_key(|t| d2(*t))
}

/// Pixel center of tile `t`.
pub fn center(t: [i32; 2]) -> [f32; 2] {
    [(t[0] as f32 + 0.5) * TILE, (t[1] as f32 + 0.5) * TILE]
}

/// Tiles a box of `size` centered at `p` overlaps.
pub fn tiles_under(p: [f32; 2], size: [f32; 2]) -> Vec<[i32; 2]> {
    let lo = |k: usize| ((p[k] - size[k] / 2.0) / TILE).floor() as i32;
    let hi = |k: usize| ((p[k] + size[k] / 2.0 - 1e-3) / TILE).floor() as i32;
    (lo(1)..=hi(1))
        .flat_map(|y| (lo(0)..=hi(0)).map(move |x| [x, y]))
        .collect()
}

/// Can a box of `size` travel in a straight line with its center going
/// from `a` to `b`, touching only passable tiles?
pub fn clear(
    passable: &impl Fn([i32; 2]) -> bool,
    a: [f32; 2],
    b: [f32; 2],
    size: [f32; 2],
) -> bool {
    let len = (b[0] - a[0]).hypot(b[1] - a[1]);
    let n = len.ceil().max(1.0) as usize;
    (0..=n).all(|i| {
        let t = i as f32 / n as f32;
        let p = [a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t];
        tiles_under(p, size).into_iter().all(passable)
    })
}

/// Straighten a tile path walked by a box of `size` whose center starts at
/// `from`: keep only the waypoints (tile centers) needed so that each
/// straight stretch keeps the whole box on passable tiles.
pub fn smooth(
    passable: impl Fn([i32; 2]) -> bool,
    from: [f32; 2],
    tiles: &[[i32; 2]],
    size: [f32; 2],
) -> Vec<[f32; 2]> {
    let clear = |a: [f32; 2], b: [f32; 2]| clear(&passable, a, b, size);
    let points: Vec<[f32; 2]> = tiles.iter().skip(1).map(|t| center(*t)).collect();
    let mut out = Vec::new();
    let mut at = from;
    let mut i = 0;
    while i < points.len() {
        // The farthest point reachable in a straight line from here; the
        // next tile center at least, since the tile path got there.
        let j = (i..points.len())
            .rev()
            .find(|&j| clear(at, points[j]))
            .unwrap_or(i);
        out.push(points[j]);
        at = points[j];
        i = j + 1;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Passable where the map has `.`.
    fn grid(rows: &'static [&'static str]) -> impl Fn([i32; 2]) -> bool {
        move |[x, y]: [i32; 2]| {
            y >= 0
                && (y as usize) < rows.len()
                && x >= 0
                && (x as usize) < rows[0].len()
                && rows[y as usize].as_bytes()[x as usize] == b'.'
        }
    }

    #[test]
    fn finds_shortest_paths_without_cutting_corners() {
        let open = grid(&["......", "......", "......"]);
        let path = find(&open, [0, 0], [5, 2]).unwrap();
        assert_eq!(path.len(), 6, "two diagonals and three straight: {path:?}");

        // The diagonal from (1,1) to (2,0) would clip the corner of (1,0).
        let wall = grid(&[".#...", "...#.", "....."]);
        let path = find(&wall, [0, 0], [4, 0]).unwrap();
        for w in path.windows(2) {
            let [a, b] = [w[0], w[1]];
            if a[0] != b[0] && a[1] != b[1] {
                assert!(
                    wall([b[0], a[1]]) && wall([a[0], b[1]]),
                    "cut a corner: {path:?}"
                );
            }
        }
        assert_eq!(*path.last().unwrap(), [4, 0]);
    }

    #[test]
    fn unreachable_goals() {
        let split = grid(&["..#..", "..#..", "..#.."]);
        assert_eq!(find(&split, [0, 0], [4, 0]), None);
        assert_eq!(nearest_reachable(&split, [0, 0], [4, 0]), Some([1, 0]));
        assert_eq!(find(&split, [0, 0], [2, 0]), None, "goal itself blocked");
    }

    #[test]
    fn smoothing_cuts_straight_where_the_box_fits() {
        let open = grid(&["......", "......", "......"]);
        let path = find(&open, [0, 0], [5, 2]).unwrap();
        let points = smooth(&open, center([0, 0]), &path, [8.0, 8.0]);
        assert_eq!(points, vec![center([5, 2])], "one straight line");

        // Around a wall: the straight line through it isn't taken.
        let wall = grid(&["......", "..##..", "......"]);
        let path = find(&wall, [0, 1], [5, 1]).unwrap();
        let points = smooth(&wall, center([0, 1]), &path, [8.0, 8.0]);
        assert!(points.len() >= 2, "{points:?}");
        assert_eq!(*points.last().unwrap(), center([5, 1]));
    }
}
