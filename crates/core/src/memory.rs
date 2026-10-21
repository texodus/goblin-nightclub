//! What a viewer remembers about entities it can't currently see.

use std::collections::{HashMap, HashSet};

use crate::entity::{Entity, EntityId, Kind, sample_points};
use crate::layer::Layer;
use crate::vision::Vision;

/// An entity as it was when last seen.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct StaleMarker {
    pub kind: Kind,
    pub pos: [f32; 2],
    pub size: [f32; 2],
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
    pub fn update<'a>(
        &mut self,
        level: &Layer,
        vision: &Vision,
        entities: impl IntoIterator<Item = &'a Entity>,
    ) {
        // Each point seen costs a raycast, so stop at the first that
        // decides it.
        let seen = |p: &[f32; 2]| vision.can_see(level, *p);

        self.visible.clear();
        for e in entities {
            if sample_points(e.pos, e.size).iter().any(seen) {
                self.visible.insert(e.id);
                self.markers.insert(
                    e.id,
                    StaleMarker {
                        kind: e.kind,
                        pos: e.pos,
                        size: e.size,
                    },
                );
            }
        }

        let visible = &self.visible;
        self.markers
            .retain(|id, m| visible.contains(id) || !sample_points(m.pos, m.size).iter().all(seen));
    }

    /// Nothing is visible (the layer isn't in view at all); markers stay.
    pub fn clear_visible(&mut self) {
        self.visible.clear();
    }

    pub fn is_visible(&self, id: EntityId) -> bool {
        self.visible.contains(&id)
    }

    /// Move every marker by `by` (pixels), as when the level it's on is
    /// made again offset; those then outside `within` (top-left and size)
    /// are forgotten.
    pub fn shift(&mut self, by: [f32; 2], within: [[f32; 2]; 2]) {
        let [[x0, y0], [w, h]] = within;
        self.visible.clear();
        self.markers.retain(|_, m| {
            m.pos = [m.pos[0] + by[0], m.pos[1] + by[1]];
            (x0..x0 + w).contains(&m.pos[0]) && (y0..y0 + h).contains(&m.pos[1])
        });
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
#[path = "tests/memory_tests.rs"]
mod tests;
