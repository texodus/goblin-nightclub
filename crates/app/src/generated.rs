//! A world generated a region at a time in web workers (see `www/worker.js`
//! and [`goblin_nightclub_generate::Plan`]): each region the game wants is
//! asked for (JS hands it to a worker), made off the main thread, and
//! handed back packed ([`generate_region`], then [`Regions::deliver`]);
//! until then the game waits on it, as on any slow source.

use std::cell::RefCell;
use std::collections::{HashMap, HashSet, VecDeque};
use std::rc::Rc;
use std::sync::Arc;

use goblin_nightclub_core::chunk::{CHUNK, Chunk, ChunkKey};
use goblin_nightclub_core::packed::Packed;
use goblin_nightclub_core::tileset::Tileset;
use goblin_nightclub_core::world::{ChunkSource, Roof};
use goblin_nightclub_generate::{Params, Plan, region_part, tileset};
use wasm_bindgen::prelude::*;

/// Regions kept, the most recently handed back: more are asked for again.
const KEEP: usize = 32;

/// Make region `(rx, ry)` of the world from `seed`, `cols` x `rows` tiles,
/// with generation parameters `params` (JSON, or the defaults), packed:
/// what a worker does.
#[wasm_bindgen]
pub fn generate_region(
    seed: u32,
    params: Option<String>,
    cols: usize,
    rows: usize,
    rx: usize,
    ry: usize,
) -> Result<Vec<u8>, JsError> {
    // The same plan for every region of a world: made once a worker.
    thread_local! {
        static PLAN: RefCell<Option<(String, Arc<Plan>)>> = const { RefCell::new(None) };
    }
    if cols == 0 || rows == 0 {
        return Err(JsError::new("a world has at least a tile"));
    }
    let key = format!("{seed} {cols} {rows} {}", params.as_deref().unwrap_or(""));
    let params: Params = match &params {
        Some(json) => serde_json::from_str(json).map_err(|e| JsError::new(&e.to_string()))?,
        None => Params::default(),
    };
    let plan = PLAN.with_borrow_mut(|kept| match kept {
        Some((k, plan)) if *k == key => plan.clone(),
        _ => {
            let plan = Arc::new(Plan::new(seed, cols, rows, &params));
            *kept = Some((key, plan.clone()));
            plan
        }
    });
    Ok(region_part(&plan, rx, ry))
}

/// The regions handed back, and those asked for.
#[derive(Default)]
pub struct Regions {
    made: HashMap<[usize; 2], Packed<Arc<[u8]>>>,
    order: VecDeque<[usize; 2]>,
    asked: HashSet<[usize; 2]>,
}

impl Regions {
    /// Region `at`, made and packed.
    pub fn deliver(&mut self, at: [usize; 2], bytes: &[u8]) -> Result<(), String> {
        let packed = Packed::open(Arc::<[u8]>::from(bytes))?.ok_or("a region came cut short")?;
        self.asked.remove(&at);
        self.made.insert(at, packed);
        self.order.push_back(at);
        if self.order.len() > KEEP {
            let gone = self.order.pop_front().unwrap();
            self.made.remove(&gone);
        }
        Ok(())
    }
}

/// A generated world's chunks, a region at a time from workers.
pub struct WorkerSource {
    plan: Plan,
    tileset: Tileset,
    regions: Rc<RefCell<Regions>>,
    /// Asks JS for a region: `request(rx, ry)`.
    request: js_sys::Function,
}

impl WorkerSource {
    /// The world `plan` says, its regions asked for with `request(rx,
    /// ry)` and handed back to the regions this returns too.
    pub fn new(plan: Plan, request: js_sys::Function) -> (Self, Rc<RefCell<Regions>>) {
        let regions = Rc::new(RefCell::new(Regions::default()));
        let source = Self {
            plan,
            tileset: tileset().clone(),
            regions: regions.clone(),
            request,
        };
        (source, regions)
    }

    /// The region chunk column `(cx, cy)` is in, if it's in the world.
    fn region_of(&self, cx: i32, cy: i32) -> Option<[usize; 2]> {
        let [across, down] = self.plan.size.map(|n| n.div_ceil(CHUNK) as i32);
        let inside = (0..across).contains(&cx) && (0..down).contains(&cy);
        let per = (self.plan.region / CHUNK) as i32;
        inside.then(|| [(cx / per) as usize, (cy / per) as usize])
    }

    /// Run `f` on region `at` if it's here; else ask for it.
    fn with<T>(
        &self,
        at: [usize; 2],
        f: impl FnOnce(&Packed<Arc<[u8]>>) -> Option<T>,
    ) -> Option<T> {
        let mut regions = self.regions.borrow_mut();
        if let Some(packed) = regions.made.get(&at) {
            return f(packed);
        }
        if regions.asked.insert(at) {
            let (rx, ry) = (JsValue::from(at[0] as u32), JsValue::from(at[1] as u32));
            if let Err(e) = self.request.call2(&JsValue::NULL, &rx, &ry) {
                web_sys::console::error_2(&"asking for a region failed:".into(), &e);
            }
        }
        None
    }
}

impl ChunkSource for WorkerSource {
    fn size(&self) -> [usize; 3] {
        [self.plan.size[0], self.plan.size[1], self.plan.depth()]
    }

    fn tileset(&self) -> &Tileset {
        &self.tileset
    }

    fn chunk(&self, key: ChunkKey) -> Option<Arc<Chunk>> {
        if key.0 >= self.plan.depth() {
            return None;
        }
        let at = self.region_of(key.1, key.2)?;
        self.with(at, |packed| packed.chunk(key))
    }

    fn roof(&self, cx: i32, cy: i32) -> Option<Roof> {
        let at = self.region_of(cx, cy)?;
        self.with(at, |packed| packed.roof(cx, cy))
    }
}
