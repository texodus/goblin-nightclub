//! A level editor in the spirit of MS Paint. Pick a primary and a
//! secondary brush from the palette (floor and wall types, void, stairs)
//! and a shape (a whole tile, or split along a diagonal: the primary in
//! that corner's half, the secondary in the rest), then paint with the left
//! button, or the right for the two brushes swapped:
//!
//! - pencil: the tile clicked, or every tile dragged across;
//! - rectangle: the region dragged out;
//! - fill: the tile clicked and every like tile joined to it;
//! - picker: take the brushes and shape from the tile clicked;
//! - spawn: put the player's start at the point clicked (to the art's
//!   pixel), if the player fits there.
//!
//! One layer is edited at a time, seen over the layers below it as in the
//! game but with everything in view. Levels save to and load from
//! [`LevelFile`]s, and the palette is whatever the level's tileset has.
//! Nothing here touches the browser or GPU.

use serde::Serialize;

use goblin_nightclub_core::camera::{self, Camera};
use goblin_nightclub_core::draw::Canvas;
use goblin_nightclub_core::entity::PLAYER_SIZE;
use goblin_nightclub_core::frame::{Frame, LayerFrame};
use goblin_nightclub_core::input::Input;
use goblin_nightclub_core::layer::{Corner, Dir, Layer, Part, TILE, Tile};
use goblin_nightclub_core::level::{Level, MAX_LAYERS, can_stand};
use goblin_nightclub_core::level_file::{Character, LevelFile, Spawn};
use goblin_nightclub_core::palette::{self, Color};
use goblin_nightclub_core::path;
use goblin_nightclub_core::prop::{Prop, PropId};
use goblin_nightclub_core::tileset::Tileset;

/// Steps of undo kept.
const UNDO_LIMIT: usize = 100;

/// What a click or drag does.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Tool {
    Pencil,
    Rect,
    Fill,
    Pick,
    Spawn,
}

/// The shape painted: a whole tile of the primary brush, or the primary in
/// one corner's half and the secondary in the rest.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BrushShape {
    Whole,
    Split(Corner),
}

/// Something to paint with, as listed in the palette: a tile's part or
/// stairs, or a prop (or none) standing on it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Brush {
    Part(Part),
    Stairs(Dir),
    Prop(PropId),
    NoProp,
}

/// What a brush paints: a tile, or what stands on one.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Paint {
    Tile(Tile),
    Prop(Option<PropId>),
}

/// A palette entry as the interface shows it.
#[derive(Serialize)]
pub struct Swatch {
    pub name: String,
    /// Its cell in the tile sheet, if it has one (void doesn't).
    pub cell: Option<[u16; 2]>,
}

/// The editor's settings, for the interface to show.
#[derive(Serialize)]
pub struct State {
    pub tool: Tool,
    pub shape: &'static str,
    pub primary: usize,
    pub secondary: usize,
    pub layer: usize,
    pub depth: usize,
    pub cols: usize,
    pub rows: usize,
    pub can_undo: bool,
    pub can_redo: bool,
    /// Whether a layer can be added (under [`MAX_LAYERS`]) or the current
    /// one removed (not the last).
    pub can_add_layer: bool,
    pub can_remove_layer: bool,
}

/// Every brush `tileset` offers, in palette order: floors, walls, void,
/// liquids, stairs, props and none.
pub fn palette(tileset: &Tileset) -> Vec<Brush> {
    let floors = (0..tileset.floors.len()).map(|f| Brush::Part(Part::Floor(f as u8)));
    let walls = (0..tileset.walls.len()).map(|w| Brush::Part(Part::Wall(w as u8)));
    let liquids = (0..tileset.liquids.len()).map(|l| Brush::Part(Part::Liquid(l as u8)));
    let props = (0..tileset.props.len()).map(|p| Brush::Prop(p as PropId));
    floors
        .chain(walls)
        .chain([Brush::Part(Part::Void)])
        .chain(liquids)
        .chain([Brush::Stairs(Dir::Up), Brush::Stairs(Dir::Down)])
        .chain(props)
        .chain([Brush::NoProp])
        .collect()
}

pub fn swatches(tileset: &Tileset) -> Vec<Swatch> {
    palette(tileset)
        .into_iter()
        .map(|b| match b {
            Brush::Part(Part::Floor(f)) => {
                let floor = &tileset.floors[f as usize];
                Swatch {
                    name: format!("{} floor", floor.name),
                    cell: Some(floor.cells[0]),
                }
            }
            Brush::Part(Part::Wall(w)) => {
                let wall = &tileset.walls[w as usize];
                Swatch {
                    name: format!("{} wall", wall.name),
                    cell: Some(wall.cells[0]),
                }
            }
            Brush::Part(Part::Void) => Swatch {
                name: "void".into(),
                cell: None,
            },
            Brush::Part(Part::Liquid(l)) => {
                let liquid = &tileset.liquids[l as usize];
                Swatch {
                    name: liquid.name.clone(),
                    cell: Some(liquid.cells[0]),
                }
            }
            Brush::Stairs(Dir::Up) => Swatch {
                name: "stairs up".into(),
                cell: Some(tileset.stairs[0]),
            },
            Brush::Stairs(Dir::Down) => Swatch {
                name: "stairs down".into(),
                cell: Some(tileset.stairs[1]),
            },
            Brush::Prop(p) => {
                let prop = &tileset.props[p as usize];
                // Its tile's cell of its art.
                let [x, y] = prop.cells[0];
                let at = prop.anchor.map(|a| a.floor() as u16);
                Swatch {
                    name: prop.name.clone(),
                    cell: Some([x + at[0], y + at[1]]),
                }
            }
            Brush::NoProp => Swatch {
                name: "no prop".into(),
                cell: None,
            },
        })
        .collect()
}

/// A drag in progress: the button's (swapped brushes for the right
/// button), where it started and where it's got to, in tiles.
struct Stroke {
    swap: bool,
    start: [i32; 2],
    last: [i32; 2],
}

#[derive(Clone, PartialEq)]
struct Snapshot {
    layers: Vec<Vec<Tile>>,
    props: Vec<Vec<Option<PropId>>>,
    spawn: Spawn,
}

pub struct Editor {
    pub input: Input,
    cols: usize,
    rows: usize,
    /// Bottom first, each row by row.
    layers: Vec<Vec<Tile>>,
    /// What stands on each tile of each layer, likewise.
    props: Vec<Vec<Option<PropId>>>,
    spawn: Spawn,
    /// The ground level (kept from the file; see
    /// [`LevelFile::ground`]).
    ground: usize,
    /// The art, and the characters' sprites, kept for the file.
    tileset: Tileset,
    player: Character,
    npc: Character,
    /// The brushes `tileset` offers (see [`palette`]).
    palette: Vec<Brush>,
    /// The layer being edited.
    layer: usize,
    /// Built from `layers` (when `dirty` is clear), for drawing.
    level: Level,
    dirty: bool,
    tool: Tool,
    shape: BrushShape,
    /// Palette indices.
    primary: usize,
    secondary: usize,
    camera: Camera,
    stroke: Option<Stroke>,
    /// While dragging with the middle button: the last pointer position.
    panning: Option<[f32; 2]>,
    /// The tile under the pointer.
    hover: Option<[i32; 2]>,
    undo: Vec<Snapshot>,
    redo: Vec<Snapshot>,
}

fn build(
    cols: usize,
    rows: usize,
    layers: &[Vec<Tile>],
    props: &[Vec<Option<PropId>>],
    tileset: &Tileset,
) -> Level {
    Level::new(
        layers
            .iter()
            .zip(props)
            .map(|(tiles, props)| {
                Layer::from_tiles(cols, rows, tiles.clone()).with_props(&listed(cols, props))
            })
            .collect(),
        tileset.clone(),
    )
}

/// Props tile by tile, row by row, `cols` across, as a list.
fn listed(cols: usize, props: &[Option<PropId>]) -> Vec<Prop> {
    (props.iter().enumerate())
        .filter_map(|(i, p)| {
            p.map(|kind| Prop {
                at: [i % cols, i / cols],
                kind,
            })
        })
        .collect()
}

impl Editor {
    /// Edit a level file (a saved one, or a blank one to start from; see
    /// [`LevelFile::blank`]). The brushes are the first wall and the first
    /// floor of its tileset.
    pub fn from_file(file: &LevelFile, view: [f32; 2]) -> Self {
        let (cols, rows) = (file.cols, file.rows);
        let layers = file.tiles();
        let props: Vec<Vec<Option<PropId>>> = file
            .props
            .iter()
            .map(|list| {
                let mut props = vec![None; cols * rows];
                for p in list {
                    props[p.at[1] * cols + p.at[0]] = Some(p.kind);
                }
                props
            })
            .collect();
        let level = build(cols, rows, &layers, &props, &file.tileset);
        let palette = palette(&file.tileset);
        let index = |b: Brush| palette.iter().position(|p| *p == b).unwrap();
        let mut editor = Self {
            input: Input::default(),
            cols,
            rows,
            layer: file.spawn.layer,
            layers,
            props,
            spawn: file.spawn,
            ground: file.ground,
            tileset: file.tileset.clone(),
            player: file.player.clone(),
            npc: file.npc.clone(),
            primary: index(Brush::Part(Part::Wall(0))),
            secondary: index(Brush::Part(Part::Floor(0))),
            palette,
            level,
            dirty: false,
            tool: Tool::Pencil,
            shape: BrushShape::Whole,
            camera: Camera::new(view, file.tileset.zoom()),
            stroke: None,
            panning: None,
            hover: None,
            undo: Vec::new(),
            redo: Vec::new(),
        };
        editor.camera.center_on(file.spawn.pos);
        editor
    }

    pub fn to_file(&self) -> LevelFile {
        LevelFile::new(
            self.cols,
            self.rows,
            &self.layers,
            self.spawn,
            self.tileset.clone(),
            &self.player.sprite,
            &self.npc.sprite,
        )
        .with_ground(self.ground.min(self.layers.len() - 1))
        .with_props(self.props.iter().map(|p| listed(self.cols, p)).collect())
    }

    /// The art the level is drawn with.
    pub fn tileset(&self) -> &Tileset {
        &self.tileset
    }

    /// The palette as the interface shows it.
    pub fn swatches(&self) -> Vec<Swatch> {
        swatches(&self.tileset)
    }

    pub fn state(&self) -> State {
        State {
            tool: self.tool,
            shape: match self.shape {
                BrushShape::Whole => "whole",
                BrushShape::Split(Corner::NW) => "NW",
                BrushShape::Split(Corner::NE) => "NE",
                BrushShape::Split(Corner::SW) => "SW",
                BrushShape::Split(Corner::SE) => "SE",
            },
            primary: self.primary,
            secondary: self.secondary,
            layer: self.layer,
            depth: self.layers.len(),
            cols: self.cols,
            rows: self.rows,
            can_undo: !self.undo.is_empty(),
            can_redo: !self.redo.is_empty(),
            can_add_layer: self.layers.len() < MAX_LAYERS,
            can_remove_layer: self.layers.len() > 1,
        }
    }

    pub fn set_tool(&mut self, tool: Tool) {
        self.tool = tool;
    }

    pub fn set_shape(&mut self, shape: BrushShape) {
        self.shape = shape;
    }

    /// Choose palette entry `index` as the primary or the secondary brush.
    pub fn set_brush(&mut self, index: usize, secondary: bool) {
        if index < self.palette.len() {
            if secondary {
                self.secondary = index;
            } else {
                self.primary = index;
            }
        }
    }

    pub fn set_layer(&mut self, layer: usize) {
        self.layer = layer.min(self.layers.len() - 1);
    }

    /// Add a layer of plain floor just above (or below) the current one
    /// and switch to it. The spawn stays on its layer.
    pub fn add_layer(&mut self, above: bool) {
        if self.layers.len() >= MAX_LAYERS {
            return;
        }
        self.checkpoint();
        let at = if above { self.layer + 1 } else { self.layer };
        let floor = self.layers[self.layer]
            .iter()
            .find_map(|t| t.floor_id())
            .unwrap_or(0);
        let tiles = vec![Tile::Whole(Part::Floor(floor)); self.cols * self.rows];
        self.layers.insert(at, tiles);
        self.props.insert(at, vec![None; self.cols * self.rows]);
        if self.spawn.layer >= at {
            self.spawn.layer += 1;
        }
        self.layer = at;
        self.dirty = true;
    }

    /// Remove the current layer (never the last one). A spawn on it moves
    /// to the layer that takes its place.
    pub fn remove_layer(&mut self) {
        if self.layers.len() <= 1 {
            return;
        }
        self.checkpoint();
        self.layers.remove(self.layer);
        self.props.remove(self.layer);
        if self.spawn.layer > self.layer {
            self.spawn.layer -= 1;
        }
        self.spawn.layer = self.spawn.layer.min(self.layers.len() - 1);
        self.layer = self.layer.min(self.layers.len() - 1);
        self.dirty = true;
    }

    pub fn zoom_by(&mut self, steps: i32, p: [f32; 2]) {
        self.camera.zoom_by(steps, p);
    }

    pub fn resize(&mut self, view: [f32; 2]) {
        self.camera.resize(view);
    }

    pub fn update(&mut self, dt: f32) {
        self.camera.pan(&self.input, dt);
        if self.dirty {
            self.level = build(
                self.cols,
                self.rows,
                &self.layers,
                &self.props,
                &self.tileset,
            );
            self.dirty = false;
        }
    }

    fn snapshot(&self) -> Snapshot {
        Snapshot {
            layers: self.layers.clone(),
            props: self.props.clone(),
            spawn: self.spawn,
        }
    }

    fn restore(&mut self, s: Snapshot) {
        self.layers = s.layers;
        self.props = s.props;
        self.spawn = s.spawn;
        self.layer = self.layer.min(self.layers.len() - 1);
        self.dirty = true;
    }

    pub fn undo(&mut self) {
        if let Some(s) = self.undo.pop() {
            self.redo.push(self.snapshot());
            self.restore(s);
        }
    }

    pub fn redo(&mut self) {
        if let Some(s) = self.redo.pop() {
            self.undo.push(self.snapshot());
            self.restore(s);
        }
    }

    /// Remember the level as it is, before a change.
    fn checkpoint(&mut self) {
        self.undo.push(self.snapshot());
        if self.undo.len() > UNDO_LIMIT {
            self.undo.remove(0);
        }
        self.redo.clear();
    }

    /// The tile at view pixel `p`, if it's in the level.
    fn tile_at(&self, p: [f32; 2]) -> Option<[i32; 2]> {
        let w = self.camera.to_world(p);
        let t = [(w[0] / TILE).floor() as i32, (w[1] / TILE).floor() as i32];
        self.in_bounds(t).then_some(t)
    }

    fn in_bounds(&self, [x, y]: [i32; 2]) -> bool {
        x >= 0 && y >= 0 && (x as usize) < self.cols && (y as usize) < self.rows
    }

    fn index(&self, [x, y]: [i32; 2]) -> usize {
        y as usize * self.cols + x as usize
    }

    /// What the brushes paint: with `swap` (the right button), the
    /// secondary takes the primary's place.
    fn brush_paint(&self, swap: bool) -> Paint {
        let palette = &self.palette;
        let (a, b) = if swap {
            (palette[self.secondary], palette[self.primary])
        } else {
            (palette[self.primary], palette[self.secondary])
        };
        let floor = match b {
            Brush::Part(Part::Floor(f)) => f,
            _ => 0,
        };
        match a {
            Brush::Prop(p) => Paint::Prop(Some(p)),
            Brush::NoProp => Paint::Prop(None),
            Brush::Stairs(dir) => Paint::Tile(Tile::Stairs(dir, floor)),
            Brush::Part(p) => Paint::Tile(match self.shape {
                BrushShape::Whole => Tile::Whole(p),
                BrushShape::Split(c) => {
                    let rest = match b {
                        Brush::Part(q) => q,
                        _ => Part::Floor(floor),
                    };
                    Tile::split(c, p, rest)
                }
            }),
        }
    }

    /// What's at tile `t` of the layer of the kind `like` paints.
    fn painted(&self, t: [i32; 2], like: Paint) -> Paint {
        let i = self.index(t);
        match like {
            Paint::Tile(_) => Paint::Tile(self.layers[self.layer][i]),
            Paint::Prop(_) => Paint::Prop(self.props[self.layer][i]),
        }
    }

    fn paint(&mut self, t: [i32; 2], paint: Paint) {
        if self.painted(t, paint) == paint {
            return;
        }
        let i = self.index(t);
        match paint {
            Paint::Tile(tile) => self.layers[self.layer][i] = tile,
            Paint::Prop(prop) => self.props[self.layer][i] = prop,
        }
        self.dirty = true;
    }

    /// Paint every tile on the line from `a` to `b`, so fast drags leave
    /// no gaps.
    fn paint_line(&mut self, a: [i32; 2], b: [i32; 2], tile: Paint) {
        let (dx, dy) = ((b[0] - a[0]).abs(), -(b[1] - a[1]).abs());
        let (sx, sy) = ((b[0] - a[0]).signum(), (b[1] - a[1]).signum());
        let (mut p, mut err) = (a, dx + dy);
        loop {
            self.paint(p, tile);
            if p == b {
                break;
            }
            let e2 = 2 * err;
            if e2 >= dy {
                err += dy;
                p[0] += sx;
            }
            if e2 <= dx {
                err += dx;
                p[1] += sy;
            }
        }
    }

    fn fill_rect(&mut self, a: [i32; 2], b: [i32; 2], tile: Paint) {
        for y in a[1].min(b[1])..=a[1].max(b[1]) {
            for x in a[0].min(b[0])..=a[0].max(b[0]) {
                self.paint([x, y], tile);
            }
        }
    }

    /// Flood fill from `start`: every tile like it (in what `tile`
    /// paints: the tile, or what stands on it), joined to it across
    /// sides.
    fn flood(&mut self, start: [i32; 2], tile: Paint) {
        let like = self.painted(start, tile);
        if like == tile {
            return;
        }
        let mut stack = vec![start];
        let mut seen = vec![false; self.cols * self.rows];
        let mut region = Vec::new();
        while let Some(t) = stack.pop() {
            if !self.in_bounds(t) || seen[self.index(t)] || self.painted(t, tile) != like {
                continue;
            }
            seen[self.index(t)] = true;
            region.push(t);
            for d in [[1, 0], [-1, 0], [0, 1], [0, -1]] {
                stack.push([t[0] + d[0], t[1] + d[1]]);
            }
        }
        for t in region {
            self.paint(t, tile);
        }
    }

    /// Take the brushes and shape from tile `t`: with `swap`, a whole
    /// tile's part goes to the secondary brush.
    fn pick(&mut self, t: [i32; 2], swap: bool) {
        let index = |b: Brush| self.palette.iter().position(|p| *p == b).unwrap();
        // What stands on it first, if anything does.
        if let Some(p) = self.props[self.layer][self.index(t)] {
            self.set_brush(index(Brush::Prop(p)), swap);
            return;
        }
        match self.layers[self.layer][self.index(t)] {
            Tile::Whole(p) => {
                let i = index(Brush::Part(p));
                self.set_brush(i, swap);
                self.shape = BrushShape::Whole;
            }
            Tile::Split { corner, at, rest } => {
                let (a, b) = (index(Brush::Part(at)), index(Brush::Part(rest)));
                self.primary = a;
                self.secondary = b;
                self.shape = BrushShape::Split(corner);
            }
            Tile::Stairs(dir, floor) => {
                let (a, b) = (
                    index(Brush::Stairs(dir)),
                    index(Brush::Part(Part::Floor(floor))),
                );
                self.primary = a;
                self.secondary = b;
            }
        }
    }

    /// A button went down at view pixel `p`: 0 is the left, 1 the middle
    /// (pans), 2 the right.
    pub fn mouse_down(&mut self, p: [f32; 2], button: u32) {
        if button == 1 {
            self.panning = Some(p);
            return;
        }
        let Some(t) = self.tile_at(p) else {
            return;
        };
        let swap = button == 2;
        let tile = self.brush_paint(swap);
        match self.tool {
            Tool::Pick => self.pick(t, swap),
            Tool::Spawn => {
                // The point itself, on a whole pixel of the art, where the
                // player's box fits on this layer.
                let zoom = self.tileset.zoom();
                let pos = self.camera.to_world(p).map(|v| (v * zoom).round() / zoom);
                let tiles = &self.layers[self.layer];
                let props = &self.props[self.layer];
                let blocked = path::tiles_under(pos, PLAYER_SIZE)
                    .into_iter()
                    .any(|[x, y]| {
                        let i = y as usize * self.cols + x as usize;
                        let p = props.get(i).copied().flatten();
                        p.is_some_and(|p| self.tileset.props[p as usize].blocks)
                    });
                if !blocked && can_stand(tiles, self.cols, self.rows, pos, PLAYER_SIZE) {
                    self.checkpoint();
                    self.spawn = Spawn {
                        layer: self.layer,
                        pos,
                    };
                }
            }
            Tool::Fill => {
                self.checkpoint();
                self.flood(t, tile);
            }
            Tool::Pencil => {
                self.checkpoint();
                self.paint(t, tile);
                self.stroke = Some(Stroke {
                    swap,
                    start: t,
                    last: t,
                });
            }
            Tool::Rect => {
                self.stroke = Some(Stroke {
                    swap,
                    start: t,
                    last: t,
                });
            }
        }
        self.drop_unchanged_checkpoint();
    }

    /// The pointer moved to view pixel `p`.
    pub fn pointer(&mut self, p: [f32; 2]) {
        if let Some(from) = self.panning {
            self.camera.pan_by([from[0] - p[0], from[1] - p[1]]);
            self.panning = Some(p);
        }
        self.hover = self.tile_at(p);
        let Some(t) = self.tile_at(p).or_else(|| self.clamped_tile(p)) else {
            return;
        };
        let Some(stroke) = &self.stroke else {
            return;
        };
        let (last, swap) = (stroke.last, stroke.swap);
        if self.tool == Tool::Pencil {
            let tile = self.brush_paint(swap);
            self.paint_line(last, t, tile);
        }
        if let Some(stroke) = &mut self.stroke {
            stroke.last = t;
        }
    }

    /// The tile at view pixel `p`, moved into the level if it's outside.
    fn clamped_tile(&self, p: [f32; 2]) -> Option<[i32; 2]> {
        let w = self.camera.to_world(p);
        let t = [
            ((w[0] / TILE).floor() as i32).clamp(0, self.cols as i32 - 1),
            ((w[1] / TILE).floor() as i32).clamp(0, self.rows as i32 - 1),
        ];
        Some(t)
    }

    /// A button went up at view pixel `p`.
    pub fn mouse_up(&mut self, p: [f32; 2], button: u32) {
        if button == 1 {
            self.panning = None;
            return;
        }
        self.pointer(p);
        let Some(stroke) = self.stroke.take() else {
            return;
        };
        if self.tool == Tool::Rect {
            self.checkpoint();
            let tile = self.brush_paint(stroke.swap);
            self.fill_rect(stroke.start, stroke.last, tile);
        }
        self.drop_unchanged_checkpoint();
    }

    /// Don't keep an undo step for a change that changed nothing.
    fn drop_unchanged_checkpoint(&mut self) {
        if self.stroke.is_none() && self.undo.last() == Some(&self.snapshot()) {
            self.undo.pop();
        }
    }

    /// Size of the level, in world units.
    fn world(&self) -> [f32; 2] {
        [self.cols as f32 * TILE, self.rows as f32 * TILE]
    }

    /// Marks over the level: a grid over the part in view, the tile under
    /// the pointer, and the region a rectangle drag covers.
    fn overlay(&self) -> Canvas {
        const GRID: Color = [0.33, 0.33, 0.33, 0.6];
        let mut canvas = Canvas::new(&self.tileset);
        let px = 1.0 / self.tileset.zoom();
        let [w, h] = self.world();
        let lo = self.camera.pos;
        let size = self.camera.view_world();
        let cols = ((lo[0] / TILE).floor().max(0.0) as usize)
            ..=(((lo[0] + size[0]) / TILE).ceil().max(0.0) as usize).min(self.cols);
        let rows = ((lo[1] / TILE).floor().max(0.0) as usize)
            ..=(((lo[1] + size[1]) / TILE).ceil().max(0.0) as usize).min(self.rows);
        for x in cols {
            canvas.rect([x as f32 * TILE, 0.0], [px, h], GRID);
        }
        for y in rows {
            canvas.rect([0.0, y as f32 * TILE], [w, px], GRID);
        }
        let outline = |canvas: &mut Canvas, a: [i32; 2], b: [i32; 2], color| {
            let lo = [a[0].min(b[0]) as f32 * TILE, a[1].min(b[1]) as f32 * TILE];
            let hi = [
                (a[0].max(b[0]) + 1) as f32 * TILE,
                (a[1].max(b[1]) + 1) as f32 * TILE,
            ];
            canvas.outline(lo, [hi[0] - lo[0], hi[1] - lo[1]], px, color);
        };
        if let (Tool::Rect, Some(s)) = (self.tool, &self.stroke) {
            outline(&mut canvas, s.start, s.last, palette::WHITE);
        } else if let Some(t) = self.hover {
            outline(&mut canvas, t, t, palette::YELLOW);
        }
        canvas
    }

    pub fn frame(&self) -> Frame {
        let top = self.layer;
        let [w, h] = self.world();
        let everywhere = vec![[0.0, 0.0], [w, 0.0], [w, h], [0.0, h]];
        let zoom = self.camera.zoom();
        let parallax = camera::parallax(zoom, self.tileset.zoom() * self.camera.ui_scale());
        let layers = self
            .level
            .visible_layers(top)
            .iter()
            .map(|&k| {
                let layer = self.level.layer(k);
                let scale = parallax.powi((top - k) as i32);
                let within = camera::layer_view(self.camera.pos, self.camera.view, zoom, scale);
                let mut visible = Canvas::new(&self.tileset);
                let bevel = 1.0 / self.tileset.zoom();
                layer.draw_bevels(&self.tileset, &mut visible, |_| true, bevel, within);
                if self.spawn.layer == k {
                    let player = self
                        .tileset
                        .sprite(&self.player.sprite)
                        .map_or(self.tileset.white, |s| s.cell);
                    visible.sprite(self.spawn.pos, player, palette::WHITE);
                }
                let meshes = self.level.meshes(k);
                LayerFrame {
                    index: k,
                    meshes: meshes.clone(),
                    props_seen: Canvas::new(&self.tileset),
                    remembered: visible.clone(),
                    visible,
                    unknown: Canvas::new(&self.tileset),
                    polygon: everywhere.clone(),
                    lit_shapes: Vec::new(),
                    standing: Canvas::new(&self.tileset),
                    shaft: (k < top).then(|| self.level.shaft_mesh(top, k)),
                }
            })
            .collect();
        Frame {
            depth: self.layers.len(),
            top,
            layers,
            origin: [w / 2.0, h / 2.0],
            radius: f32::INFINITY,
            radius_step: 0.0,
            radius_min: 0.0,
            radius_underground: 1.0,
            time: 0.0,
            camera: self.camera.pos,
            view: self.camera.view,
            zoom: self.camera.zoom(),
            cell: self.tileset.cell,
            ui_scale: self.camera.ui_scale(),
            outdoors: false,
            ground: 0,
            world: self.world(),
            offset: [0, 0],
            base: 0,
            remember: false,
            fade: 1.0,
            sink: 0.0,
            overlay: self.overlay(),
            ui: Canvas::new(&self.tileset),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const VIEW: [f32; 2] = [320.0, 200.0];

    fn tileset() -> Tileset {
        goblin_nightclub_core::tileset::testing::tileset()
    }

    fn editor() -> Editor {
        let blank = LevelFile::blank(8, 6, 2, tileset(), "player", "npc");
        let mut e = Editor::from_file(&blank, VIEW);
        // The whole level in view, a tile 16 pixels across at the origin.
        e.camera.pos = [0.0, 0.0];
        e
    }

    /// View pixel at the center of tile `t`.
    fn at(e: &Editor, t: [i32; 2]) -> [f32; 2] {
        let z = e.camera.zoom();
        [
            ((t[0] as f32 + 0.5) * TILE - e.camera.pos[0]) * z,
            ((t[1] as f32 + 0.5) * TILE - e.camera.pos[1]) * z,
        ]
    }

    fn tile(e: &Editor, t: [i32; 2]) -> Tile {
        e.layers[e.layer][e.index(t)]
    }

    fn brush(b: Brush) -> usize {
        palette(&tileset()).iter().position(|p| *p == b).unwrap()
    }

    fn click(e: &mut Editor, t: [i32; 2], button: u32) {
        let p = at(e, t);
        e.mouse_down(p, button);
        e.mouse_up(p, button);
    }

    #[test]
    fn pencil_paints_clicks_and_drags_without_gaps() {
        let mut e = editor();
        click(&mut e, [1, 1], 0);
        assert_eq!(tile(&e, [1, 1]), Tile::WALL, "primary: wall");
        click(&mut e, [1, 1], 2);
        assert_eq!(tile(&e, [1, 1]), Tile::FLOOR, "secondary: floor");

        // A fast diagonal drag: every tile along it.
        e.mouse_down(at(&e, [0, 0]), 0);
        e.pointer(at(&e, [5, 5]));
        e.mouse_up(at(&e, [5, 5]), 0);
        for i in 0..=5 {
            assert_eq!(tile(&e, [i, i]), Tile::WALL, "{i}");
        }
        assert_eq!(tile(&e, [5, 0]), Tile::FLOOR);
    }

    #[test]
    fn rectangles_fill_the_region_dragged() {
        let mut e = editor();
        e.set_tool(Tool::Rect);
        e.mouse_down(at(&e, [5, 4]), 0);
        e.pointer(at(&e, [2, 1]));
        assert_eq!(tile(&e, [3, 2]), Tile::FLOOR, "nothing until released");
        e.mouse_up(at(&e, [2, 1]), 0);
        for y in 0..6 {
            for x in 0..8 {
                let inside = (2..=5).contains(&x) && (1..=4).contains(&y);
                assert_eq!(tile(&e, [x, y]) == Tile::WALL, inside, "{x} {y}");
            }
        }
    }

    #[test]
    fn fill_floods_like_tiles() {
        let mut e = editor();
        // A wall across column 3 splits the floor in two.
        e.set_tool(Tool::Rect);
        e.mouse_down(at(&e, [3, 0]), 0);
        e.mouse_up(at(&e, [3, 5]), 0);
        e.set_tool(Tool::Fill);
        e.set_brush(brush(Brush::Part(Part::Void)), false);
        click(&mut e, [0, 0], 0);
        assert_eq!(tile(&e, [2, 5]), Tile::VOID);
        assert_eq!(tile(&e, [3, 2]), Tile::WALL);
        assert_eq!(tile(&e, [4, 0]), Tile::FLOOR, "the other side");
    }

    #[test]
    fn shapes_split_tiles_and_stairs_sit_on_the_secondary_floor() {
        let mut e = editor();
        e.set_shape(BrushShape::Split(Corner::NW));
        e.set_brush(brush(Brush::Part(Part::Void)), true);
        click(&mut e, [2, 2], 0);
        assert_eq!(
            tile(&e, [2, 2]),
            Tile::split(Corner::NW, Part::Wall(0), Part::Void)
        );
        // The right button swaps them.
        click(&mut e, [3, 2], 2);
        assert_eq!(
            tile(&e, [3, 2]),
            Tile::split(Corner::NW, Part::Void, Part::Wall(0))
        );
        let grate = tileset().floor("grate").unwrap();
        e.set_brush(brush(Brush::Stairs(Dir::Down)), false);
        e.set_brush(brush(Brush::Part(Part::Floor(grate))), true);
        click(&mut e, [4, 2], 0);
        assert_eq!(tile(&e, [4, 2]), Tile::Stairs(Dir::Down, grate));

        // The picker takes it all back.
        e.set_tool(Tool::Pick);
        click(&mut e, [2, 2], 0);
        let s = e.state();
        assert_eq!(
            (s.shape, s.primary),
            ("NW", brush(Brush::Part(Part::Wall(0))))
        );
        assert_eq!(s.secondary, brush(Brush::Part(Part::Void)));
    }

    #[test]
    fn undo_and_redo_whole_strokes() {
        let mut e = editor();
        assert!(!e.state().can_undo);
        e.mouse_down(at(&e, [0, 0]), 0);
        e.pointer(at(&e, [3, 0]));
        e.mouse_up(at(&e, [3, 0]), 0);
        click(&mut e, [0, 0], 0);
        assert_eq!(e.undo.len(), 1, "repainting the same tile changes nothing");
        e.undo();
        assert!(
            (0..=3).all(|x| tile(&e, [x, 0]) == Tile::FLOOR),
            "the whole drag"
        );
        e.redo();
        assert!((0..=3).all(|x| tile(&e, [x, 0]) == Tile::WALL));
        // A new change drops what could be redone.
        e.undo();
        click(&mut e, [5, 5], 0);
        assert!(!e.state().can_redo);
    }

    #[test]
    fn layers_spawn_and_files() {
        let mut e = editor();
        assert_eq!(e.state().layer, 1, "starts on the top layer");
        e.set_layer(0);
        click(&mut e, [1, 1], 0);
        e.set_tool(Tool::Spawn);
        click(&mut e, [6, 4], 0);
        assert_eq!(e.layers[1][e.index([1, 1])], Tile::FLOOR);
        assert_eq!(e.layers[0][e.index([1, 1])], Tile::WALL);
        // Into the wall just painted: ignored. Off a tile's center: kept to
        // the art's pixel (16 to a tile).
        click(&mut e, [1, 1], 0);
        assert_eq!(e.spawn, Spawn::at_tile(0, [6, 4]));
        let z = e.camera.zoom();
        let p = [(23.3 - e.camera.pos[0]) * z, (41.1 - e.camera.pos[1]) * z];
        e.mouse_down(p, 0);
        e.mouse_up(p, 0);
        assert_eq!(e.spawn.pos, [23.125, 41.25]);
        e.undo();
        assert_eq!(e.spawn, Spawn::at_tile(0, [6, 4]));

        let file = LevelFile::from_json(&e.to_file().to_json()).unwrap();
        assert_eq!(file.spawn, Spawn::at_tile(0, [6, 4]));
        let loaded = Editor::from_file(&file, VIEW);
        assert_eq!(loaded.layers, e.layers);
        assert_eq!(loaded.state().layer, 0, "opens on the spawn's layer");
        assert_eq!(file.tileset, tileset(), "the tileset travels with it");
        assert_eq!(file.player.sprite, "player");
        // The palette offers everything in the tileset.
        let names: Vec<String> = e.swatches().into_iter().map(|s| s.name).collect();
        assert!(names.contains(&"grate floor".to_string()));
        assert!(names.contains(&"water".to_string()));
        // Floors, walls, void, water, stairs, props and none.
        assert_eq!(e.swatches().len(), 4 + 3 + 1 + 1 + 2 + 2 + 1);
    }

    #[test]
    fn props_are_painted_picked_undone_and_saved() {
        let mut e = editor();
        let tree = tileset().prop("tree").unwrap();
        e.set_brush(brush(Brush::Prop(tree)), false);
        e.set_tool(Tool::Rect);
        let (a, b) = (at(&e, [1, 1]), at(&e, [3, 2]));
        e.mouse_down(a, 0);
        e.pointer(b);
        e.mouse_up(b, 0);
        let on = |e: &Editor, t: [i32; 2]| e.props[e.state().layer][e.index(t)];
        assert_eq!(on(&e, [2, 2]), Some(tree));
        assert_eq!(on(&e, [4, 2]), None);
        // The tiles under them stay as they were.
        assert_eq!(e.layers[1][e.index([2, 2])], Tile::FLOOR);
        // Picked from where one stands; erased with none.
        e.set_brush(brush(Brush::NoProp), false);
        e.set_tool(Tool::Pick);
        click(&mut e, [1, 1], 0);
        assert_eq!(e.state().primary, brush(Brush::Prop(tree)));
        e.set_brush(brush(Brush::NoProp), false);
        e.set_tool(Tool::Pencil);
        click(&mut e, [1, 1], 0);
        assert_eq!(on(&e, [1, 1]), None);
        e.undo();
        assert_eq!(on(&e, [1, 1]), Some(tree));
        // Saved and loaded: there, and drawn.
        let file = LevelFile::from_json(&e.to_file().to_json()).unwrap();
        assert_eq!(file.props[1].len(), 6);
        let loaded = Editor::from_file(&file, VIEW);
        assert_eq!(loaded.props, e.props);
        e.update(0.0);
        let pieces = &e.level.meshes(1).pieces;
        assert!(pieces.iter().any(|p| !p.props.vertices.is_empty()));
        // The spawn can't be put where a prop's in the way.
        e.set_tool(Tool::Spawn);
        click(&mut e, [2, 2], 0);
        assert_ne!(e.spawn, Spawn::at_tile(1, [2, 2]));
    }

    #[test]
    fn layers_can_be_added_and_removed() {
        let mut e = editor();
        assert_eq!(e.state().depth, 2);
        e.set_layer(0);
        click(&mut e, [1, 1], 0);
        // A new layer above the bottom one, between the two: the spawn (on
        // the old top) moves up with it, and the wall stays below.
        e.add_layer(true);
        let s = e.state();
        assert_eq!((s.depth, s.layer), (3, 1));
        assert_eq!(e.spawn.layer, 2);
        assert_eq!(e.layers[0][e.index([1, 1])], Tile::WALL);
        assert!(e.layers[1].iter().all(|t| t.is_floor()), "plain floor");
        // Below the current one: the spawn moves again.
        e.add_layer(false);
        assert_eq!((e.state().depth, e.state().layer), (4, 1));
        assert_eq!(e.spawn.layer, 3);
        // Undo both.
        e.undo();
        e.undo();
        assert_eq!((e.state().depth, e.spawn.layer), (2, 1));
        // Remove the spawn's layer: the spawn drops to the one below.
        e.set_layer(1);
        e.remove_layer();
        assert_eq!((e.state().depth, e.state().layer, e.spawn.layer), (1, 0, 0));
        assert!(!e.state().can_remove_layer);
        e.remove_layer();
        assert_eq!(e.state().depth, 1, "never the last");
        e.redo();
        e.undo();
        assert_eq!(e.state().depth, 2);
        // Up to the most a level may have.
        for _ in 0..MAX_LAYERS {
            e.add_layer(true);
        }
        assert_eq!(e.state().depth, MAX_LAYERS);
        assert!(!e.state().can_add_layer);
        let file = LevelFile::from_json(&e.to_file().to_json()).unwrap();
        assert_eq!(file.layers.len(), MAX_LAYERS);
    }

    #[test]
    fn frames_show_everything_on_the_layer_and_below() {
        let mut e = editor();
        e.set_brush(brush(Brush::Part(Part::Void)), false);
        click(&mut e, [2, 2], 0);
        e.update(0.0);
        let frame = e.frame();
        assert_eq!(
            frame.layers.iter().map(|l| l.index).collect::<Vec<_>>(),
            [1, 0]
        );
        let voids = &frame.layers[0].meshes.pieces[0].voids;
        assert_eq!(voids.vertices.len(), 6, "one void tile");
        assert_eq!(frame.layers[0].polygon.len(), 4, "the whole level in view");
        assert!(!frame.overlay.vertices.is_empty(), "the grid");
    }
}
