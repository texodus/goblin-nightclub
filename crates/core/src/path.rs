//! Pathfinding on a tile grid: A* moving 8 ways without cutting corners,
//! and straightening the result into waypoints a box can travel between.
//! What's solid is up to the caller (e.g. what the player knows): a
//! `ground` function gives each tile's [`Solid`] shape, `None` where the
//! walker may stand (open, or never seen), `Full` where it may not, and a
//! `Slope` for a half tile, which the box may overlap on its open side but
//! never stand on (an 8-pixel box doesn't fit in half a 10-pixel tile).

use std::cmp::Reverse;
use std::collections::{BinaryHeap, HashMap};

use crate::layer::{Corner, Solid, TILE};

const STRAIGHT: u32 = 10;
const DIAGONAL: u32 = 14;

/// Can the walker stand on tile `t`: is it known open?
fn standable(ground: &impl Fn([i32; 2]) -> Solid, t: [i32; 2]) -> bool {
    ground(t) == Solid::None
}

/// The corner of a tile at offsets `[x, y]`, each 0 or 1.
fn corner_at(x: i32, y: i32) -> Corner {
    match (x, y) {
        (0, 0) => Corner::NW,
        (1, 0) => Corner::NE,
        (0, 1) => Corner::SW,
        _ => Corner::SE,
    }
}

/// Stepping diagonally between two tiles, the box clips the corner `c` of
/// each tile beside the step, at the vertex all four share. That's fine if
/// the tile is open there: wholly open, or a slope whose solid half is the
/// far one (its face runs between the other two corners, missing the
/// vertex).
fn corner_clear(solid: Solid, c: Corner) -> bool {
    match solid {
        Solid::None => true,
        Solid::Full => false,
        Solid::Slope(s) => s == c.opposite(),
    }
}

/// The 8 neighbors of `t` reachable in one step: diagonals only when the
/// corners of both tiles beside the step are clear too (see
/// [`corner_clear`]), so paths never cut a corner.
fn steps(ground: &impl Fn([i32; 2]) -> Solid, t: [i32; 2]) -> Vec<([i32; 2], u32)> {
    let mut out = Vec::with_capacity(8);
    for dy in -1..=1 {
        for dx in -1..=1 {
            if (dx, dy) == (0, 0) {
                continue;
            }
            let n = [t[0] + dx, t[1] + dy];
            if !standable(ground, n) {
                continue;
            }
            if dx != 0 && dy != 0 {
                // The side tiles, and their corners at the shared vertex.
                let beside_x = ground([t[0] + dx, t[1]]);
                let beside_y = ground([t[0], t[1] + dy]);
                let (px, py) = ((dx > 0) as i32, (dy > 0) as i32);
                if corner_clear(beside_x, corner_at(1 - px, py))
                    && corner_clear(beside_y, corner_at(px, 1 - py))
                {
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
/// start needn't be standable (the walker is already there); the goal
/// must be.
pub fn find(
    ground: impl Fn([i32; 2]) -> Solid,
    start: [i32; 2],
    goal: [i32; 2],
) -> Option<Vec<[i32; 2]>> {
    if start != goal && !standable(&ground, goal) {
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
        for (n, step) in steps(&ground, t) {
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

/// Every tile reachable from `start` (itself included), nearest the point
/// `to` first (by its center).
pub fn reachable(
    ground: impl Fn([i32; 2]) -> Solid,
    start: [i32; 2],
    to: [f32; 2],
) -> Vec<[i32; 2]> {
    let mut seen = vec![start];
    let mut stack = vec![start];
    let mut seen_set: std::collections::HashSet<[i32; 2]> = [start].into();
    while let Some(t) = stack.pop() {
        for (n, _) in steps(&ground, t) {
            if seen_set.insert(n) {
                seen.push(n);
                stack.push(n);
            }
        }
    }
    let d2 = |t: [i32; 2]| {
        let c = center(t);
        (c[0] - to[0]).powi(2) + (c[1] - to[1]).powi(2)
    };
    seen.sort_by(|a, b| d2(*a).total_cmp(&d2(*b)));
    seen
}

/// Of the tiles reachable from `start` (other than `start`), the one
/// whose center is nearest the point `to`.
pub fn nearest_reachable(
    ground: impl Fn([i32; 2]) -> Solid,
    start: [i32; 2],
    to: [f32; 2],
) -> Option<[i32; 2]> {
    reachable(ground, start, to)
        .into_iter()
        .find(|t| *t != start)
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

/// Can a box of `size` be centered at `p`: does it overlap nothing solid?
pub fn fits(ground: &impl Fn([i32; 2]) -> Solid, p: [f32; 2], size: [f32; 2]) -> bool {
    let pos = [p[0] - size[0] / 2.0, p[1] - size[1] / 2.0];
    !tiles_under(p, size)
        .into_iter()
        .any(|t| ground(t).hits_box(t, pos, size))
}

/// Can a box of `size` travel in a straight line with its center going
/// from `a` to `b`, touching nothing solid? Slopes count by their shape:
/// the box may pass over their open half.
pub fn clear(
    ground: &impl Fn([i32; 2]) -> Solid,
    a: [f32; 2],
    b: [f32; 2],
    size: [f32; 2],
) -> bool {
    let len = (b[0] - a[0]).hypot(b[1] - a[1]);
    let n = len.ceil().max(1.0) as usize;
    (0..=n).all(|i| {
        let t = i as f32 / n as f32;
        let p = [a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t];
        fits(ground, p, size)
    })
}

/// The farthest point along the line from `from` toward `to` a box of
/// `size` can travel in a straight line (see [`clear`]), in steps of a
/// pixel; `None` if it can't get a pixel of the way.
pub fn approach(
    ground: &impl Fn([i32; 2]) -> Solid,
    from: [f32; 2],
    to: [f32; 2],
    size: [f32; 2],
) -> Option<[f32; 2]> {
    let len = (to[0] - from[0]).hypot(to[1] - from[1]);
    let n = len.floor() as usize;
    (1..=n).rev().find_map(|i| {
        let t = i as f32 / len;
        let p = [
            from[0] + (to[0] - from[0]) * t,
            from[1] + (to[1] - from[1]) * t,
        ];
        clear(ground, from, p, size).then_some(p)
    })
}

/// Straighten a path of `points` walked by a box of `size` whose center
/// starts at `from`: keep only the waypoints needed so that each straight
/// stretch keeps the whole box off anything solid. The last point is
/// always kept.
pub fn smooth(
    ground: impl Fn([i32; 2]) -> Solid,
    from: [f32; 2],
    points: &[[f32; 2]],
    size: [f32; 2],
) -> Vec<[f32; 2]> {
    let clear = |a: [f32; 2], b: [f32; 2]| clear(&ground, a, b, size);
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

    const BOX: [f32; 2] = [8.0, 8.0];

    /// The centers of a tile path, after its first tile.
    fn centers(path: &[[i32; 2]]) -> Vec<[f32; 2]> {
        path.iter().skip(1).map(|t| center(*t)).collect()
    }

    /// Ground from a map: `.` open, `#` solid, `◸ ◹ ◺ ◿` solid in that
    /// corner; outside, solid.
    fn grid(rows: &'static [&'static str]) -> impl Fn([i32; 2]) -> Solid {
        move |[x, y]: [i32; 2]| {
            if y < 0 || (y as usize) >= rows.len() || x < 0 {
                return Solid::Full;
            }
            match rows[y as usize].chars().nth(x as usize) {
                Some('.') => Solid::None,
                Some('◸') => Solid::Slope(Corner::NW),
                Some('◹') => Solid::Slope(Corner::NE),
                Some('◺') => Solid::Slope(Corner::SW),
                Some('◿') => Solid::Slope(Corner::SE),
                _ => Solid::Full,
            }
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
                    wall([b[0], a[1]]) == Solid::None && wall([a[0], b[1]]) == Solid::None,
                    "cut a corner: {path:?}"
                );
            }
        }
        assert_eq!(*path.last().unwrap(), [4, 0]);
    }

    /// Stepping from (0,1) up to (1,0) clips the bottom-right corner of
    /// (0,0): fine past a slope solid in its top-left, not past one whose
    /// face runs through that corner, nor a wall.
    #[test]
    fn diagonal_steps_pass_open_slope_corners() {
        let past = grid(&["◸.", ".."]);
        assert_eq!(find(&past, [0, 1], [1, 0]).unwrap().len(), 2, "diagonal");
        for blocked in [
            grid(&["◹.", ".."]),
            grid(&["◿.", ".."]),
            grid(&["#.", ".."]),
        ] {
            assert_eq!(find(&blocked, [0, 1], [1, 0]).unwrap().len(), 3, "around");
        }
        // Half tiles are never stood on, and never the goal.
        assert_eq!(find(&past, [1, 1], [0, 0]), None);
        assert_eq!(
            nearest_reachable(&past, [1, 1], center([0, 0])),
            Some([1, 0])
        );
        // A point over the slope's open half may still fit the box.
        assert!(fits(&past, [9.0, 9.0], BOX), "its far corner");
        assert!(!fits(&past, [5.0, 5.0], BOX), "its middle");
        assert!(fits(&past, center([1, 1]), BOX));
    }

    #[test]
    fn unreachable_goals() {
        let split = grid(&["..#..", "..#..", "..#.."]);
        assert_eq!(find(&split, [0, 0], [4, 0]), None);
        assert_eq!(
            nearest_reachable(&split, [0, 0], center([4, 0])),
            Some([1, 0])
        );
        // By the point, not its tile: two points in the wall at (2,0) and
        // (2,1), each nearer a different tile's center.
        assert_eq!(nearest_reachable(&split, [0, 0], [22.0, 8.0]), Some([1, 0]));
        assert_eq!(
            nearest_reachable(&split, [0, 0], [22.0, 12.0]),
            Some([1, 1])
        );
        assert_eq!(find(&split, [0, 0], [2, 0]), None, "goal itself blocked");
    }

    #[test]
    fn sweeps_follow_the_shapes() {
        // Up the diagonal past (0,0)'s open corner, or not.
        let past = grid(&["◸..", "...", "..."]);
        assert!(clear(&past, center([0, 1]), center([1, 0]), BOX));
        let face = grid(&["◹..", "...", "..."]);
        assert!(!clear(&face, center([0, 1]), center([1, 0]), BOX));
        // Along a 45-degree shore (faces running with it, solid above and
        // to the right) the box rides over the open halves.
        let shore = grid(&["◹◹◹◹", ".◹◹◹", "..◹◹", "...◹", "...."]);
        assert!(clear(&shore, center([0, 1]), center([3, 4]), BOX));
        assert!(!clear(&shore, center([0, 1]), center([2, 1]), BOX));
        // Approaching a wall stops a pixel short of it, not at the goal.
        let wall = grid(&["...#"]);
        let stop = approach(&wall, center([1, 0]), center([3, 0]), BOX).unwrap();
        assert!((stop[0] - 26.0).abs() < 1e-3, "{stop:?}");
        assert_eq!(
            approach(&wall, center([2, 0]), center([3, 0]), BOX),
            Some([26.0, 5.0]),
            "a pixel, until it touches"
        );
        assert_eq!(approach(&wall, [26.0, 5.0], center([3, 0]), BOX), None);
        assert_eq!(
            approach(&wall, center([0, 0]), center([1, 0]), BOX),
            Some(center([1, 0]))
        );
    }

    #[test]
    fn smoothing_cuts_straight_where_the_box_fits() {
        let open = grid(&["......", "......", "......"]);
        let path = find(&open, [0, 0], [5, 2]).unwrap();
        let points = smooth(&open, center([0, 0]), &centers(&path), BOX);
        assert_eq!(points, vec![center([5, 2])], "one straight line");

        // Around a wall: the straight line through it isn't taken.
        let wall = grid(&["......", "..##..", "......"]);
        let path = find(&wall, [0, 1], [5, 1]).unwrap();
        let points = smooth(&wall, center([0, 1]), &centers(&path), BOX);
        assert!(points.len() >= 2, "{points:?}");
        assert_eq!(*points.last().unwrap(), center([5, 1]));
        // A final point off any center is reached straight, not via the
        // last tile's center.
        let mut to_point = centers(&path);
        to_point.push([56.0, 12.0]);
        let points = smooth(&wall, center([0, 1]), &to_point, BOX);
        assert_eq!(*points.last().unwrap(), [56.0, 12.0]);
        assert!(!points.contains(&center([5, 1])), "{points:?}");

        // Along a shore: one leg, over the slopes' open halves.
        let shore = grid(&["◹◹◹◹", ".◹◹◹", "..◹◹", "...◹", "...."]);
        let path = find(&shore, [0, 1], [3, 4]).unwrap();
        assert_eq!(path.len(), 4, "diagonal all the way: {path:?}");
        let points = smooth(&shore, center([0, 1]), &centers(&path), BOX);
        assert_eq!(points, vec![center([3, 4])]);
    }
}
