//! What a viewer remembers about entities it can't currently see.

use std::collections::{HashMap, HashSet};

use crate::entity::{Entity, EntityId, Kind, sample_points};
use crate::level::Level;
use crate::vision::Vision;

/// An entity as it was when last seen.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct StaleMarker {
    pub kind: Kind,
    pub pos: [f32; 2],
    pub size: [f32; 2],
    /// Game time (seconds) of the sighting.
    pub seen_at: f32,
}

#[derive(Default)]
pub struct Memory {
    /// Last sighting of every entity seen and not since ruled out.
    markers: HashMap<EntityId, StaleMarker>,
    /// Entities visible this frame.
    visible: HashSet<EntityId>,
}

impl Memory {
    /// Refresh from the current view. An entity is visible if any of its
    /// sample points is; visible entities overwrite their marker. A marker
    /// for an unseen entity is dropped once its whole spot is in view, i.e.
    /// it's been seen not to be there. Requiring the whole spot (rather than
    /// any point) keeps the marker when an entity steps just out of sight
    /// and its last position is still partly visible.
    pub fn update(&mut self, level: &Level, vision: &Vision, entities: &[Entity], now: f32) {
        let seen = |pos, size| sample_points(pos, size).map(|p| vision.can_see(level, p));

        self.visible.clear();
        for e in entities {
            if seen(e.pos, e.size).contains(&true) {
                self.visible.insert(e.id);
                self.markers.insert(
                    e.id,
                    StaleMarker {
                        kind: e.kind,
                        pos: e.pos,
                        size: e.size,
                        seen_at: now,
                    },
                );
            }
        }

        let visible = &self.visible;
        self.markers
            .retain(|id, m| visible.contains(id) || seen(m.pos, m.size).contains(&false));
    }

    pub fn is_visible(&self, id: EntityId) -> bool {
        self.visible.contains(&id)
    }

    /// Markers for entities not currently visible.
    pub fn stale(&self) -> impl Iterator<Item = (EntityId, &StaleMarker)> {
        self.markers
            .iter()
            .filter(|(id, _)| !self.visible.contains(id))
            .map(|(id, m)| (*id, m))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A wall in column 4 (x 40..50), open around its ends.
    fn level() -> Level {
        Level::from_ascii(&[
            "..........", //
            "....#.....", //
            "....#.....", //
            "....#.....", //
            "..........", //
        ])
    }

    fn look(level: &Level, from: [f32; 2]) -> Vision {
        let mut v = Vision::new(level, f32::INFINITY);
        v.update(level, from);
        v
    }

    const LEFT: [f32; 2] = [15.0, 25.0];
    const RIGHT: [f32; 2] = [85.0, 25.0];

    #[test]
    fn marker_kept_while_spot_out_of_view() {
        let level = level();
        let mut memory = Memory::default();
        let npc = [Entity::npc(1, [22.0, 22.0])];

        // Seen: marker recorded, not stale.
        memory.update(&level, &look(&level, LEFT), &npc, 1.0);
        assert!(memory.is_visible(1));
        assert_eq!(memory.stale().count(), 0);

        // Viewer goes behind the wall: stale marker at the last seen spot.
        memory.update(&level, &look(&level, RIGHT), &npc, 2.0);
        assert!(!memory.is_visible(1));
        let (_, m) = memory.stale().next().expect("stale marker");
        assert_eq!((m.pos, m.seen_at), ([22.0, 22.0], 1.0));
    }

    #[test]
    fn marker_cleared_when_spot_seen_empty() {
        let level = level();
        let mut memory = Memory::default();
        let mut npc = [Entity::npc(1, [22.0, 22.0])];
        memory.update(&level, &look(&level, LEFT), &npc, 1.0);
        memory.update(&level, &look(&level, RIGHT), &npc, 2.0);
        assert_eq!(memory.stale().count(), 1);

        // Meanwhile it moved behind the wall; back on the left, its old
        // spot is in plain view and empty.
        npc[0].pos = [72.0, 22.0];
        memory.update(&level, &look(&level, LEFT), &npc, 3.0);
        assert!(!memory.is_visible(1));
        assert_eq!(memory.stale().count(), 0);
    }

    #[test]
    fn marker_survives_partial_view_and_follows_resighting() {
        let level = level();
        let mut memory = Memory::default();
        let viewer = look(&level, LEFT);

        // Just past the wall's top-left corner (40, 10): from LEFT, only
        // points above y = 25 - 0.6 (x - 15) are visible, which here is
        // just the box's top-left sample point.
        let mut npc = [Entity::npc(1, [52.0, 0.5])];
        memory.update(&level, &viewer, &npc, 1.0);
        assert!(memory.is_visible(1), "partly visible counts as visible");

        // A few pixels further it's out of sight, while its old spot is
        // still partly visible, so the marker stays.
        npc[0].pos = [58.0, 0.5];
        memory.update(&level, &viewer, &npc, 2.0);
        assert!(!memory.is_visible(1));
        assert_eq!(memory.stale().count(), 1);

        // Spotted somewhere else: the marker follows.
        npc[0].pos = [22.0, 40.0];
        memory.update(&level, &viewer, &npc, 3.0);
        assert!(memory.is_visible(1));
        assert_eq!(memory.stale().count(), 0);
        assert_eq!(memory.markers[&1].pos, [22.0, 40.0]);
    }
}
