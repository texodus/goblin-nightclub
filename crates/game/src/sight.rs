//! What the player sees: how far, indoors and out and looking down, and
//! what's seen and remembered of each layer in view.

use super::*;

/// How far the player can see, in pixels, indoors: under a floor or wall
/// of some layer above. `f32::INFINITY` is unlimited.
pub(super) const VIEW_RADIUS: f32 = 96.0;

const VIEW_RADIUS_OUTDOORS_FACTOR: f32 = 5.0;

/// How far outdoors: on the top layer, or with nothing but void (or
/// water, or stairs) directly above on every layer up to it.
pub(super) const VIEW_RADIUS_OUTDOORS: f32 = VIEW_RADIUS_OUTDOORS_FACTOR * VIEW_RADIUS;

/// Looking down, each layer below the player's is seen this much less far,
/// down to `VIEW_RADIUS_MIN`.
pub(super) const VIEW_RADIUS_STEP: f32 = 16.0;

pub(super) const VIEW_RADIUS_MIN: f32 = 2.0 * TILE;

/// From outdoors, layers below the ground level are seen only this
/// fraction as far (after the step per layer).
pub(super) const UNDERGROUND_FROM_ABOVE: f32 = 1.0 / VIEW_RADIUS_OUTDOORS_FACTOR;

/// View distance for a layer `depth` layers below the player's, from a
/// `base` radius (indoors or out), `underground` if the layer is below
/// the ground level while the player is above it.
pub(super) fn view_radius(base: f32, depth: usize, underground: bool) -> f32 {
    let radius = (base - VIEW_RADIUS_STEP * depth as f32).max(VIEW_RADIUS_MIN);
    if underground {
        (radius * UNDERGROUND_FROM_ABOVE).max(VIEW_RADIUS_MIN)
    } else {
        radius
    }
}

impl Game {
    /// Is the player under the open sky: on a layer at or above the ground
    /// level, and on the top layer or under void (or water) or stairs on
    /// every layer above theirs? Stairs don't count as a roof (standing on
    /// a ramp's foot, its top is right overhead), but floors and walls do.
    /// Underground, even a shaft of daylight from above doesn't count.
    pub fn outdoors(&self) -> bool {
        let c = center(self.player, PLAYER_SIZE);
        if self.top < self.local_ground() {
            return false;
        }
        // Played a window at a time, the layers above it count too.
        if let Some(stream) = &self.stream {
            let (tile, quarter) = quarter_at(c);
            let q = QUARTERS.iter().position(|&b| b == quarter).unwrap();
            let roof = stream.roofs.at(stream.absolute(tile))[q];
            return roof as usize <= stream.window.base + self.top + 1;
        }
        let [x, y] = tile_of(c);
        let open = |k: usize| {
            let layer = self.level.layer(k);
            layer.is_void_at(c) || layer.tile(x, y).stairs().is_some()
        };
        (self.top + 1..self.level.depth()).all(open)
    }

    /// Recompute what the player sees and remembers, on their layer and on
    /// every layer showing through below it. The view radius depends on
    /// being outdoors or in, and shrinks for each layer looked down to.
    pub(super) fn observe(&mut self) {
        let origin = center(self.player, PLAYER_SIZE);
        let base = if self.outdoors() {
            VIEW_RADIUS_OUTDOORS
        } else {
            VIEW_RADIUS
        };
        // Others moving doesn't change what's seen, only the player does.
        let key = Some((origin, self.top, base, self.local_ground()));
        let moved = self.observed != key;
        self.observed = key;
        let visible = self.level.visible_layers(self.top);
        for k in 0..self.level.depth() {
            if !visible.contains(&k) {
                self.memories[k].clear_visible();
                continue;
            }
            // Looking down, the layer's own walls don't block the view, so
            // its floors and wall tops show alike.
            let view = self.level.view(self.top, k);
            if moved {
                let shaft = (k < self.top).then(|| self.level.shaft(self.top, k));
                let above = (k < self.top).then_some(&*view);
                let underground = self.outdoors() && k < self.local_ground();
                self.visions[k].set_radius(view_radius(base, self.top - k, underground));
                let own = self.level.layer(k);
                // Props block sight on the viewer's own layer, and lamps
                // light it; below, seen from above, neither.
                let around = if k == self.top {
                    Around {
                        circles: self.level.circles(k),
                        lamps: self.level.lamps(k),
                    }
                } else {
                    Around::default()
                };
                let vision = &mut self.visions[k];
                vision.update_through(&view, own, shaft.as_deref(), above, origin, around);
            }
            let here = self.npcs.iter().filter(|n| n.layer == k);
            self.memories[k].update(&view, &self.visions[k], here);
        }
    }
}
