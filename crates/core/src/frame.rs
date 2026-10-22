//                                    ▗◡≝▔▔` ▔≝=◺▂             ∕        ▐▋  ▝▉
//                                  ╣▒◿▒≘≉◬_    _▒▆▒▀         ◞         ▐▋  ▐▌
//                                  ╔`     ▝◺◿▒▟▘╔╹ ▚◛                  ▐▌  ▐▏
//       G O B L I N               ▕▎_"▃▅≣P ▐▓▛▗▉▂_▂__◚◛_         __▂▂▂▂▟▙◓▀▀▀▀≝
//        N I G H T C L U B         ║`║║_▂▐╝ ▟◙██▛╬▔`_▂◙▒▖       ▕∏▃▃▃▃▂▐▌║▂▂▂▔▔
//                                  ▙⋡▜╥∺╝ ▗▟███▛Ƒ  ▐▛▔▔▜█▌      ▐╣ ▝▜███▋ ∭▔▔▔▔
//                                   ▁⋡▚▃▄▀▉Ƣ█▀▉╺▗▐∏▊   ╠█▎      ▐╣   ▁∭█▉ ▊ ∭▐█
//                          ▁▃◛◭◒≠◚▔▔╸ ╔▒∏▕▓▌ ▟▎ ▐▒██▖    ▜▎     ╠▎≕∏__  ◙∏▍ ╣█▌
//                     _◿▎`▂_ ▟▒║╣▂▂▂▂▞╜▔▜ ╬◙▏∏▎ ▐███◘⊠◚"` ▜     ╣ ▂▅▅▅_ ▐█▎▕▒█Ǩ
//              _▂_ _▂▂▒∭▒▒◛≝▔           ▕▶╣◚▔▔▔╠∏∏║▌ ┉━━≝▒▃▘   ▕▌▟█████▙ ∭ ▐▐▌▓
//    ▁▁▁▁_╓◛▋▝◠▬Ǟ◡▃╠◚≝≊■■■▆▆▤▀◚⊜⊛╥▄▃▃▂_║╳▞▗╝▟▛◚███████████▌    ▕▒████◚◚█▌▋ ∭▟▎╱
//   ▐▔▔"⋨∭◅▃╏◠≉╬▒▇▆▅┫ ▕▄▃∏◠━━◡▃▂║▃▃▃▄▙▄∭▟▟▙▓≝▬▬╩◘██████████    ▐∏██◘▜▁╔▕▒▎ ▒█▎╣
//   ▕╣◚◘▉▐▄▃▂▁▟▒╝◙▉◙Ů▒▒◚▜▛◙◘▒▒▌▍ ▔▔"▀▬ǡ∗▃∑◚▀▀◘◘▓████▀╣▟╢▒◙◙▙▖  ▐ ╬     ∭▐▏▐█▉▏▜
//    ĴĄ  ╲╲ ▔╠▛◘◘▓▍▅╣┇▐▒∭Ǭ▛╨◀∗▃▒▀Ň◘█▒⋯┳▂▃_▐█████████████████▌∑◚╣__%◺_⊋∛ ∭ ▐▐▉▌▝
//  ▔  ┗▌   ▔◚▜▍▝▀▀▜█▌▐▐▐██▙▄▃▂_ ▗╢▒▛◚▬∗▃▂◚▀╩╩╩◘◘◙╩◘Ů▔▔▔▔▔▔▔▔▔▝▜▒▒▒▒▒◘◘▣▤▌ ∭█▌▕▎
//  Copyright 2026, [Apache 2.0](https://www.apache.org/licenses/LICENSE-2.0).

//! Everything the renderer needs for one frame. Built by the game, so it
//! contains no GPU types.

use std::sync::Arc;

use crate::art::LayerMeshes;
use crate::camera;
use crate::draw::Canvas;
use crate::geom::Rect;
use crate::layer::TILE;
use crate::map::Map;
use crate::mesh::Mesh;
use crate::vision::Lit;

/// One layer of the level as seen this frame.
pub struct LayerFrame {
    /// Which layer (0 is the bottom).
    pub index: usize,
    /// What's drawn of the layer whatever's seen: its tiles, then the
    /// fringes where materials meet and the trims along their edges, drawn
    /// under both `visible` and `remembered` (wherever fringes and trims
    /// show, the layer isn't void); and where it's void, and liquid.
    pub meshes: Arc<LayerMeshes>,
    /// Drawn over `tiles` where the viewer can currently see.
    pub visible: Canvas,
    /// Drawn over `tiles` where the viewer has seen before but can't now.
    pub remembered: Canvas,
    /// Boundary of what's in line of sight around `origin`, by angle;
    /// drawn as a triangle fan. It goes as far as `lights` light: visible
    /// within the view radius, and beyond it where they light it.
    pub polygon: Vec<[f32; 2]>,
    /// Convex pieces of solid lit along with the polygon (which stops at
    /// wall faces): the band one tile behind the visible stretches of wall.
    pub lit_shapes: Vec<Vec<[f32; 2]>>,
    /// What the lights showing what's beyond the view radius light (see
    /// [`crate::vision::Vision::lights`]): each in its color, brightest
    /// at its center.
    pub lights: Vec<Arc<Lit>>,
    /// The art of the props seen in the way of sight (see
    /// [`crate::vision::Vision::props_seen`]): wherever it's opaque, it's
    /// visible, though the polygon stops at the near side of each one's
    /// circle.
    pub props_seen: Canvas,
    /// Sprites of things standing on this layer: wherever they're opaque,
    /// the layer is drawn over any void beneath them rather than letting
    /// the layer below show through.
    pub standing: Canvas,
    /// For a layer below the viewer's, where it shows through to it; only
    /// there is what's remembered of it shown. `None` for the viewer's own
    /// layer (or one shown whole, or not shown at all: see
    /// [`Frame::sighted`]).
    pub shaft: Option<Arc<Mesh>>,
    /// For a layer below the one sight is from ([`Frame::sight`]), where
    /// it shows through to that one; only there may what's visible of it
    /// be remembered as seen. `None` for that layer itself, and those above
    /// it (where nothing is in sight).
    pub seen_through: Option<Arc<Mesh>>,
}

pub struct Frame {
    /// The viewer's layer: the one the view is from.
    pub top: usize,
    /// The viewer's layer and each one below it that shows through, top
    /// first.
    pub layers: Vec<LayerFrame>,
    /// The layer sight is from: what's visible is on it and those below it
    /// showing through. The viewer's, unless the view's been moved off
    /// it to another (to look around the level); then `origin`, the view
    /// radius and the lights are this one's.
    pub sight: usize,
    /// The layers seen from `sight` that aren't in `layers`, top first:
    /// not drawn, but what's visible of them is remembered as seen all the
    /// same. Only what's in sight of each is filled in.
    pub sighted: Vec<LayerFrame>,
    pub origin: [f32; 2],
    /// View radius on the viewer's layer, in world units; `f32::INFINITY` for
    /// unlimited.
    pub radius: f32,
    /// How much less far each layer below the viewer's is seen, and the
    /// least any is.
    pub radius_step: f32,
    pub radius_min: f32,
    /// Layers below `ground` are seen this fraction as far (1 for no
    /// change), applied after the step per layer.
    pub radius_underground: f32,
    /// Game time in seconds, for animated styles.
    pub time: f32,
    /// Top-left of the view, in world units.
    pub camera: [f32; 2],
    /// Size of the view, in screen pixels.
    pub view: [f32; 2],
    /// Screen pixels per world unit (the art is at its own size at
    /// `cell / TILE`, see [`crate::tileset::Tileset::zoom`]).
    pub zoom: f32,
    /// Edge of a cell of the tile sheet, in pixels.
    pub cell: u32,
    /// Screen pixels per interface pixel, for `ui`.
    pub ui_scale: f32,
    /// Is the viewer under the open sky? Then the mist over layers below is
    /// light instead of dark.
    pub outdoors: bool,
    /// The ground level: the lowest layer under the open sky. Looking down
    /// from outdoors, the mist over layers at or above it is light, over
    /// those below dark.
    pub ground: usize,
    /// Size of the level, in world units: of the window onto it, for a
    /// level played a window at a time (see [`crate::world`]), whose
    /// coordinates are all the window's.
    pub world: [f32; 2],
    /// Where the window is in the whole level: its top-left tile, and the
    /// layer its layer 0 is. What's kept across windows (what's been seen)
    /// and patterns pinned to the world go by these.
    pub offset: [i32; 2],
    pub base: usize,
    /// For a level played a window at a time, what's been explored of it,
    /// shown beyond the window.
    pub map: Option<Arc<Map>>,
    /// Whether what's visible is remembered as seen; the editor shows
    /// everything and remembers none of it.
    pub remember: bool,
    /// How opaque the viewer's layer is: less than 1 while it fades in or
    /// out, going from one layer to another, showing the layer beneath.
    pub fade: f32,
    /// How many layers deeper than it is each layer is drawn (so how much
    /// smaller): between 0 and 1 (or -1) while the layers zoom from one
    /// layer's view to the next's; 0 otherwise.
    pub sink: f32,
    /// Drawn in world space over everything, on the viewer's layer.
    pub overlay: Canvas,
    /// Drawn over everything in interface pixels (`ui_scale` screen
    /// pixels each): the interface.
    pub ui: Canvas,
}

impl Frame {
    /// How much smaller each layer below the viewer's is drawn than the
    /// one above it (see [`camera::parallax`]).
    pub fn parallax(&self) -> f32 {
        camera::parallax(self.zoom, self.cell as f32 / TILE * self.ui_scale)
    }

    /// How much smaller than the viewer's layer `index` is drawn.
    pub fn scale(&self, index: usize) -> f32 {
        self.parallax().powf((self.top - index) as f32 + self.sink)
    }

    /// The part of layer `index` in view, padded a little.
    pub fn layer_view(&self, index: usize) -> Rect {
        let scale = self.scale(index);
        camera::layer_view(self.camera, self.view, self.zoom, scale)
    }
}
