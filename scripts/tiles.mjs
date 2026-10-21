// Draws the placeholder tile sheet, www/assets/tiles.png: 16-pixel cells in
// a 16-bit console style (hand-picked color ramps, hue-shifted from cool
// shadows to warm highlights, light from the top left), laid out as the
// generator's tileset (crates/generate/assets/tileset.json) names them:
//
//   row 0      white (for plain colored shapes), stairs up, stairs down,
//              then water, eight variants (columns 3 to 10)
//   rows 1-4   the first layout, kept so level files saved with the old
//              tileset still draw: plain, planks, dirt, grate (row 1);
//              brick, rock, metal (row 2); sprites: player, npc (row 3);
//              grass, sand, stone (row 4); two variants each
//   row 3      after the sprites, from column 2, trims: small sprites the
//              tileset scatters along material edges (grass tufts and a
//              flower clump, pebbles, foam)
//   rows 5-14  every variant of one material a row: plain, planks, dirt,
//              grate, grass, sand, stone, rock, brick, metal
//   rows 15-16 props (things standing on tiles): trees, each two cells
//              square, seen from above (columns 0-5), then rocks, a cell
//              each, on row 15 (from column 6), then torches (braziers),
//              a cell each (from column 10)
//
// Natural textures are tileable across variants: near a cell's edges each
// variant shows a field all of its material's variants share, and only
// further in its own, so any variant can sit beside any other without a
// seam. Variants differ in that inner field, in their overall shade, and
// in sparse details (tufts, pebbles, flowers, cracks); distinctive details
// go in variants the tileset picks rarely, so nothing repeats visibly when
// zoomed out. Texture is mostly in clusters of a few pixels, not single-
// pixel noise.
//
//   node scripts/tiles.mjs

import { writeFile, mkdir } from "node:fs/promises";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { deflateSync } from "node:zlib";

const ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const OUT = path.join(ROOT, "www", "assets", "tiles.png");

export const CELL = 16;
const COLS = 16;
const ROWS = 17;

// --- Colors ---------------------------------------------------------------

const rgb = (s) => [1, 3, 5].map((i) => parseInt(s.slice(i, i + 2), 16));
const ramp = (...hexes) => hexes.map(rgb);

// Each from shadow to highlight.
const R = {
    grass: ramp("#163a2d", "#1f5439", "#2c7040", "#449046", "#69b04f", "#9bcb5c"),
    dirt: ramp("#2c1c1b", "#472e25", "#664230", "#865a3b", "#a5774e", "#c49a69"),
    sand: ramp("#7a5c45", "#a07e51", "#c19e63", "#d9bb7b", "#eacf93", "#f7e6b6"),
    stone: ramp("#252534", "#38394b", "#4f5265", "#6a6d81", "#8a8ea1", "#b0b4c4"),
    rock: ramp("#211a1d", "#382b2a", "#523f36", "#6e5642", "#8c7354", "#ae946c"),
    brick: ramp("#32141a", "#531f25", "#792e2d", "#9c4239", "#bc6048", "#d98760"),
    mortar: ramp("#2c2829", "#463f3d", "#615854", "#7f756c"),
    metal: ramp("#191f2e", "#2a3547", "#425065", "#5f7187", "#899aaf", "#bccad9"),
    planks: ramp("#2d1a13", "#4a2c1d", "#6a4428", "#8a5d33", "#a97a44", "#c89b5e"),
    plain: ramp("#0e1018", "#191c28", "#232838", "#30384c", "#414b63", "#57637f"),
    water: ramp("#163262", "#1e4682", "#275da0", "#3477bd", "#55a0d8", "#92cdef", "#e4f6ff"),
    moss: ramp("#2f4a2c", "#466a36", "#638f43", "#87b152"),
    rust: ramp("#4d2416", "#7a3a1c", "#a35a2a"),
    hole: ramp("#07080c", "#0e1118"),
};
const PETALS = ["#f3efe2", "#f2cf4a", "#e9809f", "#86b6ec"].map(rgb);
const PISTIL = ["#e9a83a", "#f6e27a", "#fbe3ef", "#f3efe2"].map(rgb);
const WHITE = rgb("#ffffff");

// The color `i` steps up `r`, rounded and clamped.
const pick = (r, i) => r[Math.max(0, Math.min(r.length - 1, Math.round(i)))];

// --- Noise ----------------------------------------------------------------

// Deterministic hash of integers to [0, 1).
function hash(...ns) {
    let h = 0x9e3779b9;
    for (const n of ns) {
        h = Math.imul(h ^ (n | 0), 0x85ebca6b);
        h ^= h >>> 13;
        h = Math.imul(h, 0xc2b2ae35);
        h ^= h >>> 16;
    }
    return (h >>> 0) / 2 ** 32;
}

const seedOf = (name) => [...name].reduce((h, c) => Math.imul(h ^ c.charCodeAt(0), 16777619), 2166136261) >>> 0;

// A small seeded generator, for placing details.
function rng(seed) {
    let a = seed >>> 0;
    return () => {
        a = (a + 0x6d2b79f5) >>> 0;
        let t = Math.imul(a ^ (a >>> 15), 1 | a);
        t = (t + Math.imul(t ^ (t >>> 7), 61 | t)) ^ t;
        return ((t ^ (t >>> 14)) >>> 0) / 2 ** 32;
    };
}

const smooth = (t) => t * t * (3 - 2 * t);
const smoothstep = (a, b, x) => smooth(Math.min(1, Math.max(0, (x - a) / (b - a))));

// Value noise on a lattice `s` pixels apart (dividing CELL), repeating
// every cell, so a cell tiles with itself.
function vnoise(x, y, s, seed) {
    const n = CELL / s;
    const fx = x / s;
    const fy = y / s;
    const ix = Math.floor(fx);
    const iy = Math.floor(fy);
    const tx = smooth(fx - ix);
    const ty = smooth(fy - iy);
    const at = (i, j) => hash(((i % n) + n) % n, ((j % n) + n) % n, seed);
    const a = at(ix, iy);
    const b = at(ix + 1, iy);
    const c = at(ix, iy + 1);
    const d = at(ix + 1, iy + 1);
    return a + (b - a) * tx + (c - a) * ty + (a - b - c + d) * tx * ty;
}

// Octaves of `[spacing, weight]`, normalized to [0, 1).
function fbm(x, y, seed, octaves) {
    let sum = 0;
    let norm = 0;
    for (const [s, w] of octaves) {
        sum += vnoise(x, y, s, seed + s * 131) * w;
        norm += w;
    }
    return sum / norm;
}

// How much of a variant's own field shows at cell point (x, y): none at
// the edges, all of it `band` pixels in.
function inner(x, y, band = 4) {
    const u = ((x % CELL) + CELL) % CELL;
    const v = ((y % CELL) + CELL) % CELL;
    return smoothstep(0, band, Math.min(u, CELL - u, v, CELL - v));
}

// A field for variant `v` of `material`: the variant's own, except right
// at the edges, where it eases into a field all the material's variants
// share, flattened toward its middle value so its features (identical at
// every tile's edge) don't show as a grid. See the top of the file.
const SEAM_BAND = Number(process.env.SEAM_BAND ?? 2);
const SEAM_FLAT = Number(process.env.SEAM_FLAT ?? 0.5);
function seamless(material, v, octaves, band = SEAM_BAND, flat = SEAM_FLAT) {
    const shared = seedOf(material);
    const own = shared + 1000 + v * 7919;
    return (x, y) => {
        const w = inner(x, y, band);
        const edge = 0.5 + (fbm(x, y, shared, octaves) - 0.5) * flat;
        return edge * (1 - w) + fbm(x, y, own, octaves) * w;
    };
}

// Stones: a field's contour at its middle value is a network of cracks,
// and each region it encloses a stone, bulging toward its middle (lit on
// the side toward the light), every other one a shade lighter. `crevice`
// is how wide the cracks are, in field units; `bulge` how far in a stone
// rises to full height.
function stones(material, v, octaves, { crevice, bulge }) {
    // Not flattened at the edges: that would draw cracks along them.
    const f = seamless(material, v, octaves, SEAM_BAND, 1);
    const h = (x, y) => smoothstep(crevice, bulge, Math.abs(f(x, y) - 0.5));
    return { f, h, crack: (x, y) => Math.abs(f(x, y) - 0.5) < crevice, side: (x, y) => (f(x, y) > 0.5 ? 1 : -1) };
}

// How much height field `h` faces the light (from the top left) at (x, y).
const light = (h, x, y) => h(x + 1, y) - h(x - 1, y) + h(x, y + 1) - h(x, y - 1);

// --- Cells ----------------------------------------------------------------

// A cell's pixels, row by row: a color each, or null for transparent.
const blank = () => new Array(CELL * CELL).fill(null);
const get = (t, x, y) => t[y * CELL + x];
function set(t, x, y, c) {
    if (x >= 0 && y >= 0 && x < CELL && y < CELL) t[y * CELL + x] = c;
}
function fill(f) {
    const t = blank();
    for (let y = 0; y < CELL; y++) for (let x = 0; x < CELL; x++) t[y * CELL + x] = f(x, y, x + 0.5, y + 0.5);
    return t;
}

// A natural ground: a seamless height field, shaded by the light and by a
// per-variant tone (inside only, so edges still match), on ramp `r` around
// step `mid`, `spread` steps either way.
function ground(material, v, r, { mid, spread, octaves, lit, tone = 0.6 }) {
    const h = seamless(material, v, octaves);
    const shade = (hash(seedOf(material), v) - 0.5) * tone;
    const t = fill((x, y, px, py) => pick(r, mid + (h(px, py) - 0.5) * 2 * spread + light(h, px, py) * lit + shade * inner(px, py)));
    return { t, h };
}

// A rounded pebble or stone, `w` x `h`, lit from the top left, with a
// shadow down and right; colors are steps of `r`.
function pebble(t, x, y, w, h, r, base) {
    for (let j = 0; j < h; j++) {
        for (let i = 0; i < w; i++) {
            const corner = (i === 0 || i === w - 1) && (j === 0 || j === h - 1);
            if (corner && w > 2 && h > 2) continue;
            const lit = i + j < (w + h) / 2 - 1;
            const dark = i + j > (w + h) / 2;
            set(t, x + i, y + j, pick(r, base + (lit ? 1 : dark ? -1 : 0)));
        }
    }
    // The shadow it casts.
    for (let i = 1; i <= w; i++) if (get(t, Math.min(x + i, 15), Math.min(y + h, 15))) set(t, x + i, y + h, pick(r, base - 2));
    for (let j = 1; j < h; j++) set(t, x + w, y + j, pick(r, base - 2));
}

// Spots of details for variant `v`, away from the edges: `n` positions.
function spots(material, v, n, margin = 2) {
    const next = rng(seedOf(material) + v * 104729 + 7);
    const out = [];
    for (let i = 0; i < n; i++) {
        out.push([margin + Math.floor(next() * (CELL - 2 * margin)), margin + Math.floor(next() * (CELL - 2 * margin)), next()]);
    }
    return out;
}

// A hairline crack wandering from (x, y) for `n` pixels: dark, with the lit
// lip of the far side beside it, and a short branch.
function crack(t, x, y, n, dark, lip, seed) {
    const next = rng(seed);
    let [dx, dy] = next() < 0.5 ? [1, 1] : [-1, 1];
    for (let i = 0; i < n; i++) {
        set(t, x, y, dark);
        if (get(t, x + 1, y) !== dark) set(t, x + 1, y, lip);
        if (i === Math.floor(n / 2)) crack(t, x, y, Math.floor(n / 3), dark, lip, seed + 1);
        const turn = next();
        if (turn < 0.35) x += dx;
        else if (turn < 0.5) dx = -dx;
        y += dy;
        if (next() < 0.2) [dx, dy] = [dx, dy * (next() < 0.5 ? 1 : 0) || 1];
    }
}

// --- Materials ----------------------------------------------------------
// Each draws variant `v`. Variants from `RARE[material]` up have the
// distinctive details, and the tileset lists them once to the others'
// twice.

export const VARIANTS = {
    plain: 10, planks: 10, dirt: 12, grate: 6, grass: 14,
    sand: 10, stone: 12, rock: 12, brick: 10, metal: 8, water: 8,
};
export const RARE = {
    plain: 6, planks: 7, dirt: 8, grate: 4, grass: 9,
    sand: 7, stone: 8, rock: 8, brick: 7, metal: 5, water: 6,
};

function grass(v) {
    const r = R.grass;
    const { t } = ground("grass", v, r, { mid: 2.4, spread: 1.5, octaves: [[8, 0.3], [4, 0.45], [2, 0.25]], lit: 1.2, tone: 0.3 });
    // Tufts: two blades, lit tips, a shadow at their roots.
    for (const [x, y] of spots("grass", v, 4 + (v % 4), 2)) {
        set(t, x, y, r[4]);
        set(t, x, y + 1, r[3]);
        set(t, x + 2, y - 1, r[5]);
        set(t, x + 2, y, r[4]);
        set(t, x + 2, y + 1, r[3]);
        set(t, x + 1, y + 2, r[1]);
        set(t, x + 2, y + 2, r[1]);
    }
    if (v >= RARE.grass) {
        const kind = v - RARE.grass;
        if (kind < 3) {
            // Flowers: four petals about a pistil, one or two of them.
            for (const [x, y] of spots("flowers", v, kind === 1 ? 2 : 1, 3)) {
                const c = PETALS[(v + x) % PETALS.length];
                set(t, x, y - 1, c);
                set(t, x - 1, y, c);
                set(t, x + 1, y, c);
                set(t, x, y + 1, c);
                set(t, x, y, PISTIL[(v + x) % PISTIL.length]);
                set(t, x + 1, y + 1, r[1]);
                set(t, x + 1, y + 2, r[0]);
            }
        } else if (kind === 3) {
            pebble(t, 6, 7, 4, 3, R.stone, 3);
        } else {
            // A clump of clover: darker leaves, lit edges.
            for (const [x, y] of spots("clover", v, 3, 4)) {
                set(t, x, y, r[1]);
                set(t, x + 1, y, r[2]);
                set(t, x, y + 1, r[2]);
                set(t, x + 1, y + 1, r[4]);
            }
        }
    }
    return t;
}

function dirt(v) {
    const r = R.dirt;
    const { t } = ground("dirt", v, r, { mid: 2.3, spread: 1.3, octaves: [[4, 0.6], [2, 0.4]], lit: 1.6, tone: 0.3 });
    for (const [x, y, k] of spots("dirt", v, 2 + (v % 3), 2)) {
        if (k < 0.5) pebble(t, x, y, 2, 2, r, 4);
        else set(t, x, y, r[0]); // a pit
    }
    if (v >= RARE.dirt) {
        const kind = v - RARE.dirt;
        if (kind === 0 || kind === 2) pebble(t, 5, 6, 4, 3, R.stone, 3);
        if (kind === 1 || kind === 2) {
            // A twig.
            for (let i = 0; i < 5; i++) set(t, 4 + i, 10 - Math.floor(i / 2), i % 2 ? R.planks[2] : R.planks[3]);
        }
        if (kind === 3) for (const [x, y] of spots("dirt-stones", v, 4, 2)) pebble(t, x, y, 2, 2, R.stone, 3);
    }
    return t;
}

function sand(v) {
    const r = R.sand;
    // Flat sand in broad bands (no thin contours: the field must move well
    // past a threshold to change color), sprinkled with grains.
    // (Not flattened at the edges: thresholds on a flattened edge would
    // outline every tile.)
    const h = seamless("sand", v, [[8, 0.4], [4, 0.6]], SEAM_BAND, 1);
    const shade = (hash(seedOf("sand"), v) - 0.5) * 0.12;
    const t = fill((x, y, px, py) => {
        const n = h(px, py) + shade * inner(px, py);
        return n > 0.64 ? r[4] : n < 0.36 ? r[2] : r[3];
    });
    for (const [x, y, k] of spots("sand", v, 7, 1)) set(t, x, y, k < 0.45 ? r[2] : k < 0.85 ? r[4] : r[5]);
    // Wind ripples: a shallow arc, a lit crest over a shaded trough.
    if (v % 3 === 0) {
        const [[x, y]] = spots("sand-ripples", v, 1, 4);
        for (let i = 0; i < 6; i++) {
            const dy = i === 0 || i === 5 ? 1 : 0;
            set(t, x + i - 1, y + dy, r[4]);
            set(t, x + i - 1, y + dy + 1, r[2]);
        }
    }
    if (v >= RARE.sand) {
        const kind = v - RARE.sand;
        if (kind === 0) {
            // A shell.
            const c = rgb("#f1d6cf");
            set(t, 7, 7, c);
            set(t, 8, 7, rgb("#fbeee6"));
            set(t, 7, 8, rgb("#d9a99c"));
            set(t, 8, 8, c);
            set(t, 9, 8, rgb("#b98579"));
        } else {
            pebble(t, 5 + kind * 2, 6, 3, 2, R.stone, 4);
        }
    }
    return t;
}

function stone(v) {
    const r = R.stone;
    // Bare rock underfoot: broad flat slabs between fine cracks, the odd
    // loose pebble.
    const { h, crack, side } = stones("stone", v, [[8, 0.35], [4, 0.65]], { crevice: 0.016, bulge: 0.2 });
    const grain = seamless("stone-grain", v, [[4, 0.5], [2, 0.5]]);
    const shade = (hash(seedOf("stone"), v) - 0.5) * 0.3;
    const t = fill((x, y, px, py) => {
        if (crack(px, py)) return r[1];
        const lit = light(h, px, py);
        return pick(r, 3 + side(px, py) * 0.4 + (h(px, py) - 1) * 0.9 + lit * 0.8 + (grain(px, py) - 0.5) * 0.4 + shade * inner(px, py));
    });
    for (const [x, y, k] of spots("stone-pebbles", v, 2, 2)) if (k < 0.5) pebble(t, x, y, 2, 2, r, 4);
    if (v >= RARE.stone) {
        const kind = v - RARE.stone;
        if (kind < 2) {
            // Lichen.
            for (const [x, y] of spots("lichen", v, 4, 2)) {
                set(t, x, y, R.moss[2]);
                set(t, x + 1, y, R.moss[1]);
                if (kind) set(t, x, y + 1, R.moss[1]);
            }
        } else {
            pebble(t, 5, 6, 4, 3, r, 3);
        }
    }
    return t;
}

function rock(v) {
    const r = R.rock;
    // Rough rock seen from above: small boulders between dark cracks.
    const { h, crack, side } = stones("rock", v, [[8, 0.3], [4, 0.7]], { crevice: 0.03, bulge: 0.18 });
    const shade = (hash(seedOf("rock"), v) - 0.5) * 0.3;
    const t = fill((x, y, px, py) => {
        if (crack(px, py)) return r[0];
        const lit = light(h, px, py);
        return pick(r, 2.7 + side(px, py) * 0.5 + (h(px, py) - 0.7) * 1.2 + lit * 1.6 + shade * inner(px, py));
    });
    if (v >= RARE.rock) {
        const kind = v - RARE.rock;
        if (kind < 2) {
            // Moss along the cracks.
            for (let y = 0; y < CELL; y++) {
                for (let x = 0; x < CELL; x++) {
                    const [px, py] = [x + 0.5, y + 0.5];
                    const m = vnoise(px, py, 4, seedOf("moss") + v);
                    if (!crack(px, py) && h(px, py) < 0.5 && m > 0.62) set(t, x, y, pick(R.moss, 1 + light(h, px, py) * 3));
                }
            }
        } else {
            // A glint of ore.
            for (const [x, y] of spots("ore", v, 2, 3)) {
                set(t, x, y, rgb("#d8bf63"));
                set(t, x + 1, y, rgb("#f4e7a6"));
                set(t, x, y + 1, rgb("#8f7a3a"));
            }
        }
    }
    return t;
}

function brick(v) {
    const r = R.brick;
    const m = R.mortar;
    const next = rng(seedOf("brick") + v);
    const tones = new Map();
    // Courses four pixels tall, bricks eight wide, every other course offset
    // by half a brick. A brick across the cell's edge is the same in every
    // variant, so neighbors agree on it.
    const tone = (course, id, seam) => {
        const key = seam ? `s${course}` : `${course}.${id}`;
        if (!tones.has(key)) {
            const u = seam ? hash(seedOf("brick-seam"), course) : next();
            tones.set(key, u < 0.15 ? 1.6 : u < 0.8 ? 2.6 : 3.4);
        }
        return tones.get(key);
    };
    const speck = seamless("brick", v, [[4, 0.5], [2, 0.5]]);
    const t = fill((x, y, px, py) => {
        const course = Math.floor(y / 4);
        const off = course % 2 ? 4 : 0;
        const bx = (x + off) % CELL;
        const row = y % 4;
        const col = bx % 8;
        if (row === 3) return col === 7 ? m[0] : m[1];
        if (col === 7) return row === 0 ? m[2] : m[1];
        const id = Math.floor(bx / 8);
        const base = tone(course, id, off === 4 && id === 0);
        const edge = row === 0 ? 1 : row === 2 || col === 6 ? -0.8 : 0;
        return pick(r, base + edge + (speck(px, py) - 0.5) * 1.4);
    });
    if (v >= RARE.brick) {
        const kind = v - RARE.brick;
        if (kind === 0) {
            // A chipped corner.
            set(t, 9, 4, m[0]);
            set(t, 10, 4, m[1]);
            set(t, 9, 5, m[1]);
        } else if (kind === 1) {
            // Moss creeping along the mortar.
            for (const [x] of spots("brick-moss", v, 4, 1)) {
                set(t, x, 7, R.moss[1]);
                set(t, x + 1, 7, R.moss[2]);
                set(t, x, 6, R.moss[2]);
            }
        } else {
            // A crack.
            for (let i = 0; i < 4; i++) set(t, 5 + (i % 2), 8 + i, r[0]);
        }
    }
    return t;
}

function metal(v) {
    const r = R.metal;
    // A plate: brushed horizontally, a seam along its bottom and right, its
    // top and left edges catching the light, rivets in the corners.
    const brush = (x, y) => vnoise(x, y * 8, 4, seedOf("metal") + v) * 0.6 + vnoise(x, y, 8, seedOf("metal")) * 0.4;
    const t = fill((x, y, px, py) => {
        if (x === 15 || y === 15) return r[0];
        if (x === 0 || y === 0) return r[4];
        if (x === 14 || y === 14) return r[2];
        return pick(r, 3 + (brush(px, py) - 0.5) * 1.6);
    });
    for (const [x, y] of [[2, 2], [12, 2], [2, 12], [12, 12]]) {
        set(t, x, y, r[5]);
        set(t, x + 1, y, r[4]);
        set(t, x, y + 1, r[4]);
        set(t, x + 1, y + 1, r[1]);
    }
    if (v % 3 === 1) for (let x = 1; x < 15; x++) { set(t, x, 7, r[1]); set(t, x, 8, r[4]); }
    if (v >= RARE.metal) {
        const kind = v - RARE.metal;
        if (kind === 0) {
            // A vent.
            for (let y = 5; y < 11; y += 2) for (let x = 5; x < 11; x++) { set(t, x, y, R.hole[0]); set(t, x, y + 1, r[4]); }
        } else {
            // Rust.
            for (const [x, y] of spots("metal-rust", v, 3 + kind, 3)) {
                set(t, x, y, R.rust[1]);
                set(t, x + 1, y, R.rust[2]);
                set(t, x, y + 1, R.rust[0]);
            }
        }
    }
    return t;
}

function planks(v) {
    const r = R.planks;
    const next = rng(seedOf("planks") + v);
    // Boards four pixels tall across the cell. Where a board leaves the
    // cell it has the same shade in every variant; a variant may break a
    // board with two butt joints and shade the piece between differently.
    const boards = [0, 1, 2, 3].map((b) => {
        const edge = 2.6 + hash(seedOf("planks-edge"), b) * 0.6;
        if (next() < 0.7) return { edge, joints: [] };
        const a = 2 + Math.floor(next() * 5);
        const z = a + 4 + Math.floor(next() * 5);
        return { edge, joints: [a, Math.min(z, 13)], mid: 1.9 + next() * 1.8 };
    });
    const grain = seamless("planks-grain", v, [[8, 0.5], [4, 0.5]]);
    const t = fill((x, y, px, py) => {
        const b = boards[Math.floor(y / 4)];
        const row = y % 4;
        if (row === 3) return r[0];
        if (b.joints.includes(x)) return r[1];
        const inside = b.joints.length && x > b.joints[0] && x < b.joints[1];
        const base = inside ? b.mid : b.edge;
        const g = Math.abs(vnoise(px * 0.5, py * 4, 8, seedOf("planks") + v * 31 + Math.floor(y / 4)) - 0.5) < 0.035 ? -0.8 : 0;
        return pick(r, base + (row === 0 ? 0.9 : row === 2 ? -0.5 : 0) + g + (grain(px, py) - 0.5) * 0.8);
    });
    if (v >= RARE.planks) {
        const [x, y] = [4 + (v % 3) * 3, 1 + (v % 2) * 8];
        // A knot.
        set(t, x, y, r[1]);
        set(t, x + 1, y, r[0]);
        set(t, x, y + 1, r[2]);
        set(t, x + 1, y + 1, r[1]);
        if (v - RARE.planks === 2) for (let i = 0; i < 3; i++) set(t, 9 + i, 5, r[5]); // a worn nail line
    }
    return t;
}

function plain(v) {
    const r = R.plain;
    // A big square floor tile: grout along the top and left, a lit inner
    // rim, a shaded bottom and right, a faintly mottled face.
    const face = seamless("plain", v, [[8, 0.6], [4, 0.4]]);
    const shade = (hash(seedOf("plain"), v) - 0.5) * 0.8;
    const t = fill((x, y, px, py) => {
        if (x === 0 || y === 0) return r[0];
        if (x === 1 || y === 1) return r[4];
        if (x === 15 || y === 15) return r[1];
        return pick(r, 2.8 + shade + (face(px, py) - 0.5) * 1.6);
    });
    for (const [x, y, k] of spots("plain", v, 2, 3)) set(t, x, y, k < 0.5 ? r[2] : r[4]);
    if (v >= RARE.plain) {
        const kind = v - RARE.plain;
        if (kind === 0 || kind === 3) {
            crack(t, 5 + kind, 3, 8, r[0], r[3], seedOf("plain-crack") + v);
        } else if (kind === 1) {
            // Moss in the grout.
            for (let i = 3; i < 13; i++) if (hash(i, v) < 0.6) set(t, i, 0, R.moss[1 + (i % 2)]);
            for (let i = 2; i < 9; i++) if (hash(i, v, 1) < 0.6) set(t, 0, i, R.moss[1]);
        } else {
            // A stain.
            for (let y = 5; y < 12; y++) {
                for (let x = 4; x < 12; x++) {
                    const d = (x - 8) ** 2 + (y - 8) ** 2 + vnoise(x, y, 2, v) * 6;
                    if (d < 9) set(t, x, y, pick(r, 2.2 - (d < 4 ? 0.6 : 0)));
                }
            }
        }
    }
    return t;
}

function grate(v) {
    const r = R.metal;
    // A drain grate: bars two pixels wide every eight, lit along their top
    // and left, dark below and right; holes between, deep, their top rows in
    // the bars' shadow.
    const t = fill((x, y) => {
        const [gx, gy] = [x % 8, y % 8];
        const barX = gx < 2;
        const barY = gy < 2;
        if (barX || barY) {
            if ((barY && gy === 0) || (barX && gx === 0 && !barY)) return r[barX && barY ? 5 : 4];
            return r[barX && barY ? 3 : 2];
        }
        if (gy === 2 || gx === 2) return R.hole[0];
        return gy === 7 ? r[0] : R.hole[1];
    });
    for (const [x, y] of spots("grate", v, 2, 0)) if (x % 8 < 2 || y % 8 < 2) set(t, x, y, r[1]); // grime
    if (v >= RARE.grate) {
        for (const [x, y] of spots("grate-rust", v, 8, 0)) {
            if (x % 8 < 2 || y % 8 < 2) set(t, x, y, R.rust[1 + ((x + y) % 2)]);
        }
    }
    return t;
}

function water(v) {
    const r = R.water;
    const { t } = ground("water", v, r, { mid: 2.4, spread: 1.0, octaves: [[8, 0.3], [4, 0.7]], lit: 0.8, tone: 0.2 });
    // Ripples: a lit crest with a shadow below.
    for (const [x, y] of spots("water", v, 3, 2)) {
        const len = 3 + ((x + y) % 3);
        for (let i = 0; i < len; i++) {
            set(t, x + i, y, i === 1 ? r[5] : r[4]);
            if (i > 0 && i < len - 1) set(t, x + i, y + 1, r[1]);
        }
    }
    if (v >= RARE.water) for (const [x, y] of spots("sparkle", v, 2, 3)) set(t, x, y, r[6]);
    return t;
}

// Stairs: treads four pixels deep between dark stringers. Up is lit,
// rising toward the top; down sinks into the dark.
function stairs(up) {
    const r = R.stone;
    return fill((x, y) => {
        if (x < 2 || x > 13) return x === 0 || x === 15 ? r[0] : r[1];
        const tread = Math.floor(y / 4);
        const row = y % 4;
        const depth = up ? 3 - tread : tread;
        const base = up ? 4.4 - depth * 0.6 : 3.4 - depth * 1.0;
        if (row === 3) return pick(r, base - 2.2); // the riser's shadow
        return pick(r, base + (row === 0 ? 0.8 : 0));
    });
}

// Characters: chunky, outlined in a dark plum rather than black, lit from
// the top left, standing on a soft shadow (translucent, so it never covers
// a hole: see `fs_unmask`). Each pixel is a key into the legend below it.

// An adventurer: spiky red hair, a yellow scarf, a blue tunic, boots.
const PLAYER = [
    "................",
    ".....o.oo.o.....",
    "....ojojjojo....",
    "...ojjhjjhjjo...",
    "...ohjhhjhhhHo..",
    "...oHhhtthhHHo..",
    "...oHtettetsHo..",
    "...oHtettetsHo..",
    "....oSttttsSo...",
    "...oyzyyyyyYYo..",
    "..ocbbbbbbbbBo..",
    "..ocbbLGLLbbBo..",
    "..otobbbbbbBoto.",
    "....oppoopPo....",
    "...~okko~okKo~..",
    "....~~~~~~~~....",
];
const PLAYER_LEGEND = {
    o: "#1d1226", // outline
    H: "#6b1e18", h: "#b2381f", j: "#e8662e", // hair
    S: "#a65a40", s: "#d08a62", t: "#f1c39a", // skin
    e: "#1d1226", // eyes
    Y: "#b8862a", y: "#e8b83a", z: "#fbe27c", // scarf
    B: "#22386a", b: "#3460a8", c: "#5c94d8", // tunic
    L: "#5a3820", G: "#f0cf5a", // belt, buckle
    P: "#2c2436", p: "#4c3e56", // trousers
    K: "#3a2216", k: "#7a4a28", // boots
};

// A goblin: green, big-eared, yellow-eyed, in a purple vest with a gold
// earring (it's a nightclub).
const NPC = [
    "................",
    "................",
    "....o......o....",
    "...oGo.oo.oGo...",
    "...ogGoggooGgo..",
    "....oggqgggGo...",
    "....oyegyegGo...",
    "....oggGgggGo...",
    ".....owwwwo$....",
    "....oVvvvvVVo...",
    "...ogVvmvvVVgo..",
    "...ooVvvvvVVoo..",
    ".....ogGoGgo....",
    ".....ooo.ooo....",
    "....~~~~~~~~....",
    "................",
];
const NPC_LEGEND = {
    o: "#1d1226", // outline
    G: "#2f5a26", g: "#5a9a38", q: "#8ccc54", // skin
    y: "#f8e050", e: "#1d1226", // eyes
    w: "#f3efe2", // teeth
    V: "#3a1f52", v: "#6a3a8e", m: "#a070c8", // vest
    $: "#f0c850", // earring
};

// The shadow under a character's feet: black at this opacity.
const SHADOW = 0.32;

function sprite(art, legend) {
    for (const row of art) if (row.length !== CELL) throw new Error(`sprite row ${JSON.stringify(row)} isn't ${CELL} wide`);
    return fill((x, y) => {
        const k = art[y][x];
        if (k === "~") return [0, 0, 0, Math.round(SHADOW * 255)];
        return legend[k] ? rgb(legend[k]) : null;
    });
}

// --- Trims ----------------------------------------------------------------
// Small sprites scattered along material edges, centered in their cells
// (where the edge, or a point a little inside the material, is), with a
// shadow ('~') down and right. Drawn small so they read at any angle of
// edge.

const TRIM_LEGEND = {
    t: "#9bcb5c", h: "#69b04f", g: "#449046", d: "#2c7040", // grass
    w: "#f3efe2", y: "#e9a83a", p: "#e9809f", // petals, pistil
    l: "#b0b4c4", m: "#8a8ea1", n: "#6a6d81", k: "#4f5265", // stone
    f: "#e4f6ff", F: "#92cdef", // foam
};
const TRIMS = [
    // Grass tufts.
    ["...t...",
     ".t.h.t.",
     ".h.g.h.",
     "hg.g.gh",
     ".gdgdg.",
     "..~~~.."],
    ["..t...t..",
     "..h.t.h..",
     ".hg.h.gh.",
     ".gg.g.gd.",
     "..dgdgd..",
     "...~~~..."],
    [".t.t.",
     ".h.h.",
     "hg.gh",
     ".dgd.",
     ".~~~."],
    // A flower clump.
    ["..w....",
     ".wyw.p.",
     "..w.pyp",
     "..g..p.",
     ".hg.g..",
     "..dgd..",
     "...~~.."],
    // Pebbles.
    [".lm..",
     "lmmn.",
     ".nnk~",
     "..~~."],
    [".lm.....",
     "lmmn.lm.",
     ".nk~lmmn",
     "..~~.nk~",
     "......~~"],
    ["lm....",
     "nk~.lm",
     ".~~.nk",
     "lm...~",
     "nk~...",
     ".~~..."],
    // Foam.
    [".ff.",
     "fFFf",
     ".ff."],
    ["..f...",
     ".fFf.f",
     "..f.fF",
     "....f."],
    [".f..f.",
     "fFf.Ff",
     ".f...."],
];

// Art smaller than a cell, centered in one.
function trim(art) {
    const [w, h] = [art[0].length, art.length];
    for (const row of art) if (row.length !== w) throw new Error(`trim row ${JSON.stringify(row)} isn't ${w} wide`);
    const [x0, y0] = [Math.floor((CELL - w) / 2), Math.floor((CELL - h) / 2)];
    return fill((x, y) => {
        const k = art[y - y0]?.[x - x0];
        if (k === "~") return [0, 0, 0, Math.round(SHADOW * 255)];
        return k && TRIM_LEGEND[k] ? rgb(TRIM_LEGEND[k]) : null;
    });
}

// --- Props ----------------------------------------------------------------
// Things standing on tiles, seen from straight above, lit from the top
// left, each over its shadow (down and right): drawn as one picture, then
// cut into cells.

// A shape of round clumps (`[x, y, r]`, in pixels), `size` pixels square:
// each pixel's height (0 outside, up to 1 at a clump's middle), as a dome
// over each clump.
function clumps(size, list) {
    return (x, y) => {
        let best = 0;
        for (const [cx, cy, r] of list) {
            const d2 = (x - cx) ** 2 + (y - cy) ** 2;
            if (d2 < r * r) best = Math.max(best, Math.sqrt(1 - d2 / (r * r)));
        }
        return x < 0 || y < 0 || x >= size || y >= size ? 0 : best;
    };
}

// A picture `size` pixels square of the shape with heights `h`: shaded on
// ramp `r` around step `mid` by how it faces the light, roughened by
// `grain` (0 to 1) of noise, outlined in the ramp's darkest, over its
// shadow `drop` pixels down and right.
function shaded(size, h, r, { mid, grain, seed, drop }) {
    const px = new Array(size * size).fill(null);
    for (let y = 0; y < size; y++) {
        for (let x = 0; x < size; x++) {
            const [cx, cy] = [x + 0.5, y + 0.5];
            const here = h(cx, cy);
            if (here <= 0) {
                if (h(cx - drop, cy - drop) > 0) px[y * size + x] = [0, 0, 0, Math.round(SHADOW * 255)];
                continue;
            }
            const edge = [[1, 0], [-1, 0], [0, 1], [0, -1]].some(([dx, dy]) => h(cx + dx, cy + dy) <= 0);
            if (edge) {
                px[y * size + x] = r[0];
                continue;
            }
            const lit = (h(cx - 1, cy - 1) - h(cx + 1, cy + 1)) * -4 + here * 1.5;
            const rough = (hash(x >> 1, y >> 1, seed) - 0.5) * 2 * grain;
            px[y * size + x] = pick(r, mid + lit + rough);
        }
    }
    return px;
}

// The cells of a picture `n` cells square, row by row.
function cells(px, n) {
    const size = n * CELL;
    const out = [];
    for (let j = 0; j < n; j++) {
        for (let i = 0; i < n; i++) {
            out.push(fill((x, y) => px[(j * CELL + y) * size + i * CELL + x]));
        }
    }
    return out;
}

const LEAVES = ramp("#0f2a22", "#163a2d", "#1f5439", "#2c7040", "#449046", "#69b04f", "#9bcb5c");

// A tree, variant `v`: a round canopy of leafy clumps, two cells square,
// centered on its tile (a cell in from each side).
function tree(v) {
    const size = 2 * CELL;
    const next = rng(seedOf("tree") + v * 7919);
    const c = size / 2;
    const list = [[c, c, 9 + next() * 2]];
    const around = 6 + Math.floor(next() * 3);
    for (let i = 0; i < around; i++) {
        const a = (i / around + next() * 0.1) * Math.PI * 2;
        const d = 6 + next() * 2.5;
        list.push([c + Math.cos(a) * d, c + Math.sin(a) * d, 4.5 + next() * 2]);
    }
    const h = clumps(size, list);
    return cells(shaded(size, h, LEAVES, { mid: 2.6, grain: 0.9, seed: seedOf("leaves") + v, drop: 2 }), 2);
}

// A rock, variant `v`: a boulder or two, a cell square.
function boulder(v) {
    const next = rng(seedOf("boulder") + v * 104729);
    const c = CELL / 2;
    const list = [[c - 0.5 + next() * 1.5, c + next() * 1.5, 4.5 + next() * 1.5]];
    if (next() < 0.6) list.push([c - 3 + next() * 6, c - 3 + next() * 6, 2.5 + next() * 1.5]);
    const h = clumps(CELL, list);
    return shaded(CELL, h, R.rock, { mid: 2.6, grain: 0.5, seed: seedOf("boulder") + v, drop: 1 });
}

const FIRE = ramp("#5a1a14", "#9c2f17", "#d8571c", "#f28c28", "#fbc04a", "#ffe98a", "#fffbe0");

// A torch, variant `v`: an iron brazier, a cell square, seen from above,
// its coals glowing and a flame licking up out of them, brightest at the
// heart.
function brazier(v) {
    const next = rng(seedOf("brazier") + v * 15485863);
    const c = CELL / 2;
    const bowl = shaded(CELL, clumps(CELL, [[c - 0.5, c - 0.5, 6]]), R.metal, { mid: 1.6, grain: 0.3, seed: seedOf("brazier") + v, drop: 1 });
    // The flame: a few tongues about the middle, swept a little one way.
    const tongues = [[c - 0.5, c - 0.5, 2.2]];
    for (let i = 0; i < 4; i++) {
        const a = (i / 4 + next() * 0.15) * Math.PI * 2;
        tongues.push([c - 0.5 + Math.cos(a) * 1.9, c - 0.5 + Math.sin(a) * 1.9, 1.1 + next() * 0.7]);
    }
    const flame = clumps(CELL, tongues);
    return bowl.map((px, i) => {
        const [x, y] = [i % CELL, Math.floor(i / CELL)];
        const d = Math.hypot(x + 0.5 - (c - 0.5), y + 0.5 - (c - 0.5));
        if (d > 4.2) return px;
        // Coals in the bowl, flecked; the flame over them.
        const coal = 0.4 + hash(x >> 1, y, seedOf("coals") + v) * 2 - d * 0.3;
        const f = flame(x + 0.5, y + 0.5);
        return pick(FIRE, f > 0 ? Math.max(coal, 2 + f * 4.4) : coal);
    });
}

export const PROPS = { trees: 3, rocks: 4, torches: 2 };

export const SPRITES = { player: [PLAYER, PLAYER_LEGEND], npc: [NPC, NPC_LEGEND] };
export { sprite };

export const MATERIALS = { plain, planks, dirt, grate, grass, sand, stone, rock, brick, metal, water };

// The row each material's variants are in.
export const ROW = { plain: 5, planks: 6, dirt: 7, grate: 8, grass: 9, sand: 10, stone: 11, rock: 12, brick: 13, metal: 14 };

// The whole sheet, as RGBA.
export function sheet() {
    const W = COLS * CELL;
    const H = ROWS * CELL;
    const pixels = new Uint8Array(W * H * 4);
    const put = (cx, cy, t) => {
        for (let y = 0; y < CELL; y++) {
            for (let x = 0; x < CELL; x++) {
                // A color, with its alpha if it has one (a shadow).
                const c = t[y * CELL + x];
                pixels.set(c ? [c[0], c[1], c[2], c[3] ?? 0xff] : [0, 0, 0, 0], ((cy * CELL + y) * W + cx * CELL + x) * 4);
            }
        }
    };
    put(0, 0, fill(() => WHITE));
    put(1, 0, stairs(true));
    put(2, 0, stairs(false));
    for (let v = 0; v < VARIANTS.water; v++) put(3 + v, 0, water(v));
    for (const [name, row] of Object.entries(ROW)) {
        for (let v = 0; v < VARIANTS[name]; v++) put(v, row, MATERIALS[name](v));
    }
    // The first layout, two variants each.
    const legacy = [
        ["plain", 0, 1], ["planks", 2, 1], ["dirt", 4, 1], ["grate", 6, 1],
        ["brick", 0, 2], ["rock", 2, 2], ["metal", 4, 2],
        ["grass", 0, 4], ["sand", 2, 4], ["stone", 4, 4],
    ];
    for (const [name, cx, cy] of legacy) {
        put(cx, cy, MATERIALS[name](0));
        put(cx + 1, cy, MATERIALS[name](1));
    }
    put(0, 3, sprite(PLAYER, PLAYER_LEGEND));
    put(1, 3, sprite(NPC, NPC_LEGEND));
    TRIMS.forEach((art, i) => put(2 + i, 3, trim(art)));
    for (let v = 0; v < PROPS.trees; v++) {
        tree(v).forEach((t, i) => put(2 * v + (i % 2), 15 + Math.floor(i / 2), t));
    }
    for (let v = 0; v < PROPS.rocks; v++) put(2 * PROPS.trees + v, 15, boulder(v));
    for (let v = 0; v < PROPS.torches; v++) put(2 * PROPS.trees + PROPS.rocks + v, 15, brazier(v));
    return { width: W, height: H, pixels };
}

export function png(width, height, rgba) {
    const crcTable = Array.from({ length: 256 }, (_, n) => {
        let c = n;
        for (let k = 0; k < 8; k++) c = c & 1 ? 0xedb88320 ^ (c >>> 1) : c >>> 1;
        return c >>> 0;
    });
    const crc = (buf) => {
        let c = 0xffffffff;
        for (const b of buf) c = crcTable[(c ^ b) & 0xff] ^ (c >>> 8);
        return (c ^ 0xffffffff) >>> 0;
    };
    const chunk = (type, data) => {
        const out = Buffer.alloc(12 + data.length);
        out.writeUInt32BE(data.length, 0);
        out.write(type, 4, "ascii");
        Buffer.from(data).copy(out, 8);
        out.writeUInt32BE(crc(out.subarray(4, 8 + data.length)), 8 + data.length);
        return out;
    };
    const header = Buffer.alloc(13);
    header.writeUInt32BE(width, 0);
    header.writeUInt32BE(height, 4);
    header[8] = 8; // bit depth
    header[9] = 6; // RGBA
    // Each scanline starts with filter type 0 (none).
    const raw = Buffer.alloc(height * (width * 4 + 1));
    for (let y = 0; y < height; y++) {
        Buffer.from(rgba.subarray(y * width * 4, (y + 1) * width * 4)).copy(raw, y * (width * 4 + 1) + 1);
    }
    return Buffer.concat([
        Buffer.from([0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a]),
        chunk("IHDR", header),
        chunk("IDAT", deflateSync(raw)),
        chunk("IEND", Buffer.alloc(0)),
    ]);
}

if (import.meta.url === `file://${process.argv[1]}`) {
    const { width, height, pixels } = sheet();
    await mkdir(path.dirname(OUT), { recursive: true });
    await writeFile(OUT, png(width, height, pixels));
    console.log(`Wrote ${path.relative(ROOT, OUT)} (${width}x${height}, ${CELL}px cells)`);
}
