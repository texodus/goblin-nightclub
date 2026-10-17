// Draws the placeholder tile sheet, crates/core/assets/tiles.png, in the EGA
// palette.
// Cells are CELL pixels square, laid out as src/tileset.rs expects:
//
//   row 0  white (for plain colored shapes), stairs up, stairs down
//   row 1  floors, two variants each: plain, planks, dirt, grate
//   row 2  walls, two variants each: brick, rock, metal
//   row 3  sprites: player, npc
//
//   node scripts/tiles.mjs

import { writeFile, mkdir } from "node:fs/promises";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { deflateSync } from "node:zlib";

const ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const OUT = path.join(ROOT, "crates", "core", "assets", "tiles.png");

const CELL = 16;
const COLS = 8;
const ROWS = 4;

const EGA = {
    black: [0x00, 0x00, 0x00],
    blue: [0x00, 0x00, 0xaa],
    green: [0x00, 0xaa, 0x00],
    cyan: [0x00, 0xaa, 0xaa],
    red: [0xaa, 0x00, 0x00],
    magenta: [0xaa, 0x00, 0xaa],
    brown: [0xaa, 0x55, 0x00],
    lightGray: [0xaa, 0xaa, 0xaa],
    darkGray: [0x55, 0x55, 0x55],
    lightBlue: [0x55, 0x55, 0xff],
    lightGreen: [0x55, 0xff, 0x55],
    lightCyan: [0x55, 0xff, 0xff],
    lightRed: [0xff, 0x55, 0x55],
    lightMagenta: [0xff, 0x55, 0xff],
    yellow: [0xff, 0xff, 0x55],
    white: [0xff, 0xff, 0xff],
};

const W = COLS * CELL;
const H = ROWS * CELL;
const pixels = new Uint8Array(W * H * 4);

// Draw into cell (cx, cy): `f(x, y)` gives the color at each cell pixel,
// or null for transparent.
function cell(cx, cy, f) {
    for (let y = 0; y < CELL; y++) {
        for (let x = 0; x < CELL; x++) {
            const c = f(x, y);
            const i = ((cy * CELL + y) * W + cx * CELL + x) * 4;
            if (c) {
                pixels.set([...c, 0xff], i);
            } else {
                pixels.set([0, 0, 0, 0], i);
            }
        }
    }
}

// Deterministic noise in [0, 1) per pixel and seed.
function noise(x, y, seed) {
    let h = (x * 374761393 + y * 668265263 + seed * 2147483647) >>> 0;
    h = Math.imul(h ^ (h >>> 13), 1274126177) >>> 0;
    return ((h ^ (h >>> 16)) >>> 0) / 2 ** 32;
}

// Row 0: white, stairs.
cell(0, 0, () => EGA.white);
for (const [cx, tread, gap] of [
    [1, EGA.lightGray, EGA.darkGray],
    [2, EGA.darkGray, EGA.black],
]) {
    cell(cx, 0, (x, y) => {
        if (x < 2 || x > 13) return EGA.black;
        return y % 4 === 1 || y % 4 === 2 ? tread : gap;
    });
}

// Row 1: floors.
for (const v of [0, 1]) {
    // Plain: dark, with a blue grid line along the top and left.
    cell(0 + v, 1, (x, y) => {
        if (x === 0 || y === 0) return EGA.blue;
        if (v === 1 && noise(x, y, 1) < 0.03) return EGA.darkGray;
        return EGA.black;
    });
    // Planks: horizontal boards with staggered joints.
    cell(2 + v, 1, (x, y) => {
        const board = y >> 2;
        if (y % 4 === 3) return EGA.black;
        const joint = (board * 6 + v * 3) % CELL;
        if (x === joint) return EGA.black;
        return noise(x, y, 2 + v) < 0.08 ? EGA.red : EGA.brown;
    });
    // Dirt: dark, speckled.
    cell(4 + v, 1, (x, y) => {
        const n = noise(x, y, 4 + v);
        if (n < 0.1) return EGA.brown;
        if (n < 0.16) return EGA.darkGray;
        return EGA.black;
    });
    // Grate: gray bars with dark holes.
    cell(6 + v, 1, (x, y) => {
        const hole = x % 4 !== 0 && y % 4 !== 0;
        if (v === 1 && hole && noise(x >> 2, y >> 2, 6) < 0.2) return EGA.blue;
        return hole ? EGA.black : EGA.darkGray;
    });
}

// Row 2: walls.
for (const v of [0, 1]) {
    // Brick: running bond with gray mortar.
    cell(0 + v, 2, (x, y) => {
        const course = y >> 2;
        if (y % 4 === 3) return EGA.darkGray;
        const offset = course % 2 === 0 ? 0 : 4;
        if ((x + offset + v * 2) % 8 === 7) return EGA.darkGray;
        return noise(x, y, 8 + v) < 0.06 ? EGA.lightRed : EGA.red;
    });
    // Rock: brown with gray and yellow flecks.
    cell(2 + v, 2, (x, y) => {
        const n = noise(x, y, 10 + v);
        if (n < 0.08) return EGA.darkGray;
        if (n < 0.11) return EGA.yellow;
        return EGA.brown;
    });
    // Metal: a gray plate with seams and rivets.
    cell(4 + v, 2, (x, y) => {
        if (x === 15 || y === 15) return EGA.darkGray;
        if (v === 0 && (x === 7 || y === 7)) return EGA.darkGray;
        const rivet = (x === 2 || x === 12) && (y === 2 || y === 12);
        if (rivet) return EGA.white;
        return EGA.lightGray;
    });
}

// Row 3: sprites, transparent around the figure.
const PLAYER = [
    "................",
    "................",
    "......####......",
    ".....#oooo#.....",
    ".....#o##o#.....",
    ".....#oooo#.....",
    "......#oo#......",
    "....########....",
    "...#oooooooo#...",
    "...#o#oooo#o#...",
    "...##.#oo#.##...",
    "......#oo#......",
    ".....#o##o#.....",
    ".....#o##o#.....",
    ".....##..##.....",
    "................",
];
const NPC = [
    "................",
    "................",
    "................",
    "......####......",
    "....##oooo##....",
    "...#oooooooo#...",
    "...#o##oo##o#...",
    "..#oo#xoox#oo#..",
    "..#oooooooooo#..",
    "..#oo######oo#..",
    "...#oooooooo#...",
    "...#o#o##o#o#...",
    "...##.#..#.##...",
    "................",
    "................",
    "................",
];
for (const [cx, art, body, eye] of [
    [0, PLAYER, EGA.lightCyan, EGA.white],
    [1, NPC, EGA.lightRed, EGA.yellow],
]) {
    cell(cx, 3, (x, y) => {
        switch (art[y][x]) {
            case "#": return EGA.black;
            case "o": return body;
            case "x": return eye;
            default: return null;
        }
    });
}

function png(width, height, rgba) {
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

await mkdir(path.dirname(OUT), { recursive: true });
await writeFile(OUT, png(W, H, pixels));
console.log(`Wrote ${path.relative(ROOT, OUT)} (${W}x${H}, ${CELL}px cells)`);
