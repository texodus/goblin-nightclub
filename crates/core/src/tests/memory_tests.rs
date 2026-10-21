use super::*;

/// A wall in column 4 (x 40..50), open around its ends.
fn level() -> Layer {
    Layer::from_ascii(&[
        "..........", //
        "....#.....", //
        "....#.....", //
        "....#.....", //
        "..........", //
    ])
}

fn look(level: &Layer, from: [f32; 2]) -> Vision {
    let mut v = Vision::new(level, f32::INFINITY);
    v.update(level, from);
    v
}

const LEFT: [f32; 2] = [15.0, 25.0];

#[test]
fn marker_survives_partial_view_and_follows_resighting() {
    let level = level();
    let mut memory = Memory::default();
    let viewer = look(&level, LEFT);

    // Just past the wall's top-left corner (40, 10): from LEFT, only
    // points above y = 25 - 0.6 (x - 15) are visible, which here is
    // just the box's top-left sample point.
    let mut npc = [Entity::npc(1, [52.0, 0.5])];
    memory.update(&level, &viewer, &npc);
    assert!(memory.is_visible(1), "partly visible counts as visible");

    // A few pixels further it's out of sight, while its old spot is
    // still partly visible, so the marker stays.
    npc[0].pos = [58.0, 0.5];
    memory.update(&level, &viewer, &npc);
    assert!(!memory.is_visible(1));
    assert_eq!(memory.stale().count(), 1);

    // Spotted somewhere else: the marker follows.
    npc[0].pos = [22.0, 40.0];
    memory.update(&level, &viewer, &npc);
    assert!(memory.is_visible(1));
    assert_eq!(memory.stale().count(), 0);
    assert_eq!(memory.markers[&1].pos, [22.0, 40.0]);
}
