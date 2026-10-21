import init, { App } from "./pkg/goblin_nightclub.js";

const $ = (selector) => document.querySelector(selector);
const canvas = $("#game");
const errorEl = $("#error");
const menuEl = $("#menu");
const editorEl = $("#editor");
const fileInput = $("#file");

function fail(message) {
  errorEl.textContent = message;
  errorEl.hidden = false;
}

// A message that clears itself.
let errorTimer;
function warn(message) {
  fail(message);
  clearTimeout(errorTimer);
  errorTimer = setTimeout(() => (errorEl.hidden = true), 5000);
}

// A random seed, or the one given in the URL as ?seed=<number>, for a
// level that comes out the same every time.
const seed = () => {
  const given = Number(new URLSearchParams(location.search).get("seed"));
  return Number.isInteger(given) && given >= 0
    ? given >>> 0
    : (Math.random() * 2 ** 32) >>> 0;
};

// Let the player pick files (a level file, or a packed level and any
// diffs over it); resolves to their bytes, or null if cancelled.
function pickFiles() {
  return new Promise((resolve) => {
    fileInput.value = "";
    fileInput.onchange = async () => {
      const files = [...fileInput.files];
      if (files.length === 0) return resolve(null);
      resolve(
        await Promise.all(
          files.map(async (f) => new Uint8Array(await f.arrayBuffer())),
        ),
      );
    };
    fileInput.oncancel = () => resolve(null);
    fileInput.click();
  });
}

function download(name, data, type = "application/json") {
  const url = URL.createObjectURL(new Blob([data], { type }));
  const a = Object.assign(document.createElement("a"), {
    href: url,
    download: name,
  });
  a.click();
  URL.revokeObjectURL(url);
}

async function main() {
  if (!navigator.gpu) {
    fail("WebGPU is not available in this browser.");
    return;
  }

  await init();

  // Render at the canvas's own resolution (device pixels), up to
  // MAX_PIXELS: past that, at a whole fraction of it, which CSS scales
  // back up exactly. The game lays out its interface and zoom to match.
  const MAX_PIXELS = 2_100_000;
  const fit = () => {
    const r = canvas.getBoundingClientRect();
    const dpr = window.devicePixelRatio || 1;
    let [w, h] = [Math.round(r.width * dpr), Math.round(r.height * dpr)];
    let k = 1;
    while ((w / k) * (h / k) > MAX_PIXELS) k++;
    [w, h] = [Math.max(1, Math.floor(w / k)), Math.max(1, Math.floor(h / k))];
    if (w === canvas.width && h === canvas.height) return false;
    canvas.width = w;
    canvas.height = h;
    return true;
  };
  fit();
  const app = await App.create(canvas);
  new ResizeObserver(() => {
    if (fit()) app.resize(canvas.width, canvas.height);
  }).observe(canvas);

  // The tile sheet: the level says which image it draws with (a URL or a
  // data: URL) and how big its cells are. The browser decodes it; the
  // pixels go to the renderer, and the image itself paints the editor's
  // swatches. Loaded again only when a level names a different image.
  const sheet = { image: null, cell: 16, img: null };
  async function syncSheet() {
    const info = JSON.parse(app.level_info() ?? "null");
    if (!info) return;
    sheet.cell = info.cell;
    if (info.image === sheet.image) return;
    sheet.image = info.image;
    const img = new Image();
    img.src = info.image;
    try {
      await img.decode();
    } catch {
      warn(`Couldn't load the tile sheet ${info.image}`);
      return;
    }
    if (sheet.image !== info.image) return; // superseded meanwhile
    const scratch = new OffscreenCanvas(img.width, img.height);
    const ctx = scratch.getContext("2d", { willReadFrequently: true });
    ctx.drawImage(img, 0, 0);
    const { data } = ctx.getImageData(0, 0, img.width, img.height);
    try {
      app.set_sheet(img.width, img.height, new Uint8Array(data.buffer));
    } catch (err) {
      warn(`Tile sheet: ${err.message ?? err}`);
    }
    sheet.img = img;
    if (mode === "editor") buildPalette();
  }

  // What's running: null (just the menu), "game" or "editor".
  let mode = null;
  const show = (next) => {
    mode = next;
    menuEl.hidden = true;
    editorEl.hidden = mode !== "editor";
    syncSheet();
    if (mode === "editor") {
      buildPalette();
      refreshEditor();
    }
    canvas.focus();
  };
  const showMenu = () => {
    app.menu();
    menuEl.hidden = false;
    editorEl.hidden = true;
    $("#menu-hint").hidden = mode === null;
  };

  // A game starting on a level that comes in pieces (from a server, or
  // made in workers): its tile sheet once there's a level to have it of,
  // and back to the menu if it couldn't be had (see the frame loop).
  let opening = false;

  // A new game: a big generated world, made a region at a time in workers
  // as it's played, the regions near the spawn first. ?size=<cols>x<rows>
  // makes another size; generation parameters may be given as JSON, e.g.
  // ?params={"water_share":0.2,"ramp_spacing":0}; ?seed=<n> makes the same
  // world every time. A game's requests carry its number, so regions still
  // coming for an earlier game are dropped.
  let workers = null;
  let world = 0;
  function newGame() {
    const query = new URLSearchParams(location.search);
    const asked = /^(\d+)x(\d+)$/.exec(query.get("size") ?? "");
    // The default size, if none's asked for, is the app's.
    let [cols, rows] = asked ? [Number(asked[1]), Number(asked[2])] : [];
    const params = query.get("params") ?? undefined;
    workers ??= Array.from(
      { length: Math.max(1, Math.min(4, (navigator.hardwareConcurrency ?? 2) - 1)) },
      () => {
        const worker = new Worker(new URL("./worker.js", import.meta.url), { type: "module" });
        worker.onmessage = ({ data }) => {
          if (data.game !== world) return;
          if (data.error) return warn(`Couldn't make a region: ${data.error}`);
          try {
            app.deliver_region(data.rx, data.ry, data.bytes);
          } catch (err) {
            warn(`Couldn't take a region: ${err.message ?? err}`);
          }
        };
        return worker;
      },
    );
    const [game, worldSeed] = [++world, seed()];
    let next = 0;
    const request = (rx, ry) => {
      workers[next++ % workers.length].postMessage({
        game, seed: worldSeed, params, cols, rows, rx, ry,
      });
    };
    try {
      [cols, rows] = app.new_generated_game(worldSeed, params, cols, rows, request);
    } catch (err) {
      warn(`Couldn't make that world: ${err.message ?? err}`);
      return false;
    }
    show("game");
    opening = true;
    return true;
  }

  // The start menu.
  menuEl.addEventListener("click", async (e) => {
    switch (e.target.dataset.action) {
      case "new":
        newGame();
        break;
      case "editor": {
        // A new world to edit, as for a new game (see above).
        const params = new URLSearchParams(location.search).get("params");
        try {
          app.editor(seed(), params ?? undefined);
        } catch (err) {
          warn(`Bad generation parameters: ${err.message ?? err}`);
          return;
        }
        show("editor");
        break;
      }
      case "load": {
        const files = await pickFiles();
        if (files === null) return;
        try {
          for (const bytes of files) app.stage(bytes);
          app.load_staged(seed());
          show("game");
        } catch (err) {
          warn(`Couldn't load that level: ${err.message ?? err}`);
        }
        break;
      }
    }
  });
  // A packed level on a server, read a piece at a time as it's played:
  // ?level=<url>.
  // Straight into a game, with ?level=<url> (a packed level on a server,
  // read a piece at a time as it's played) or ?size=<cols>x<rows> (a new
  // game's world that size); else the menu.
  const levelUrl = new URLSearchParams(location.search).get("level");
  if (levelUrl) {
    app.open_url(levelUrl, seed());
    show("game");
    opening = true;
  } else if (!new URLSearchParams(location.search).has("size") || !newGame()) {
    showMenu();
  }

  // The editor's interface: the palette from the level's tileset, tools,
  // shapes, layers. `buildPalette` makes the swatches for the tileset
  // being edited (again whenever it changes); `refreshEditor` makes the
  // interface show the editor's state.
  const SWATCH = 32;
  let palette = [];
  let paletteJson = null;
  let swatches = [];
  const paint = (el, entry) => {
    el.classList.toggle("void", !entry?.cell);
    el.title = entry?.name ?? "";
    if (!entry?.cell || !sheet.img) {
      el.style.backgroundImage = "";
      return;
    }
    const k = SWATCH / sheet.cell;
    el.style.backgroundImage = `url(${sheet.img.src})`;
    el.style.backgroundSize = `${sheet.img.width * k}px ${sheet.img.height * k}px`;
    el.style.backgroundPosition = `${-entry.cell[0] * SWATCH}px ${-entry.cell[1] * SWATCH}px`;
  };
  function buildPalette() {
    const json = app.editor_palette() ?? "[]";
    if (json === paletteJson) {
      swatches.forEach((el, i) => paint(el, palette[i]));
      return;
    }
    paletteJson = json;
    palette = JSON.parse(json);
    $("#swatches").replaceChildren();
    swatches = palette.map((entry, i) => {
      const el = document.createElement("div");
      el.className = "swatch";
      paint(el, entry);
      el.title += " (left: primary, right: secondary)";
      el.addEventListener("mousedown", (e) => {
        app.editor_brush(i, e.button === 2);
        refreshEditor();
      });
      el.addEventListener("contextmenu", (e) => e.preventDefault());
      $("#swatches").append(el);
      return el;
    });
    refreshEditor();
  }

  let shownDepth = 0;
  function refreshEditor() {
    const state = JSON.parse(app.editor_state() ?? "null");
    if (!state) return;
    $('[data-action="save-diff"]').disabled = !app.editor_has_base();
    for (const b of document.querySelectorAll("[data-tool]")) {
      b.classList.toggle("on", b.dataset.tool === state.tool);
    }
    for (const b of document.querySelectorAll("[data-shape]")) {
      b.classList.toggle("on", b.dataset.shape === state.shape);
    }
    swatches.forEach((el, i) => {
      el.classList.toggle("primary", i === state.primary);
      el.classList.toggle("secondary", i === state.secondary);
    });
    paint($("#primary"), palette[state.primary]);
    paint($("#secondary"), palette[state.secondary]);
    $("[data-action=undo]").disabled = !state.can_undo;
    $("[data-action=redo]").disabled = !state.can_redo;
    $("[data-action=add-layer-above]").disabled = !state.can_add_layer;
    $("[data-action=add-layer-below]").disabled = !state.can_add_layer;
    $("[data-action=remove-layer]").disabled = !state.can_remove_layer;
    if (shownDepth !== state.depth) {
      shownDepth = state.depth;
      const layers = $("#layers");
      layers.replaceChildren();
      // Top layer first, as they stack.
      for (let k = state.depth - 1; k >= 0; k--) {
        const b = document.createElement("button");
        b.textContent = k + 1;
        b.title =
          k === state.depth - 1
            ? "Top layer"
            : k === 0
              ? "Bottom layer"
              : `Layer ${k + 1}`;
        b.dataset.layer = k;
        layers.append(b);
      }
    }
    for (const b of document.querySelectorAll("[data-layer]")) {
      b.classList.toggle("on", Number(b.dataset.layer) === state.layer);
    }
    $("#status").textContent =
      `${state.cols}×${state.rows}, ${state.depth} layers`;
  }

  $("#toolbox").addEventListener("click", (e) => {
    const { tool, shape } = e.target.dataset;
    if (tool) app.editor_tool(tool);
    if (shape) app.editor_shape(shape);
    refreshEditor();
    canvas.focus();
  });

  const dialog = $("#new-level");
  dialog.addEventListener("close", () => {
    if (dialog.returnValue !== "ok") return;
    const form = dialog.querySelector("form");
    const num = (name) => Number(form.elements[name].value);
    app.editor_new(num("cols"), num("rows"), num("layers"));
    shownDepth = 0;
    syncSheet();
    buildPalette();
    refreshEditor();
  });

  $("#topbar").addEventListener("click", async (e) => {
    const { action, layer } = e.target.dataset;
    if (layer !== undefined) app.editor_layer(Number(layer));
    switch (action) {
      case "menu":
        showMenu();
        return;
      case "new-level":
        dialog.showModal();
        return;
      case "open": {
        const files = await pickFiles();
        if (files === null) return;
        try {
          for (const bytes of files) app.stage(bytes);
          app.editor_load_staged();
          shownDepth = 0;
          syncSheet();
          buildPalette();
        } catch (err) {
          warn(`Couldn't open that level: ${err.message ?? err}`);
        }
        break;
      }
      case "save":
        download("level.json", app.editor_save());
        break;
      case "save-pack":
        download("level.pack", app.editor_save_packed(), "application/octet-stream");
        break;
      case "save-diff":
        try {
          download("level-diff.pack", app.editor_save_diff(), "application/octet-stream");
        } catch (err) {
          warn(`Couldn't save a diff: ${err.message ?? err}`);
        }
        break;
      case "add-layer-above":
        app.editor_add_layer(true);
        break;
      case "add-layer-below":
        app.editor_add_layer(false);
        break;
      case "remove-layer":
        app.editor_remove_layer();
        break;
      case "undo":
        app.editor_undo();
        break;
      case "redo":
        app.editor_redo();
        break;
      case "play":
        app.play_edited(seed());
        show("game");
        return;
    }
    refreshEditor();
    canvas.focus();
  });

  // Keys: Escape toggles the menu; arrows / WASD pan; in a game, Space
  // pauses, = and - move the view a layer up and down, and C centers the
  // camera on the player, on their layer; in the editor, the tool
  // shortcuts and undo / redo.
  const GAME_KEYS = new Set([
    "ArrowUp",
    "ArrowDown",
    "ArrowLeft",
    "ArrowRight",
    "Space",
  ]);
  const TOOL_KEYS = {
    KeyB: "pencil",
    KeyR: "rect",
    KeyF: "fill",
    KeyI: "pick",
  };
  window.addEventListener("keydown", (e) => {
    if (dialog.open || e.target instanceof HTMLInputElement) return;
    if (e.code === "Escape") {
      if (!menuEl.hidden && mode) show(mode);
      else if (menuEl.hidden) showMenu();
      return;
    }
    if (!menuEl.hidden) return;
    if (GAME_KEYS.has(e.code)) e.preventDefault();
    if (mode === "editor") {
      const ctrl = e.ctrlKey || e.metaKey;
      if (ctrl && e.code === "KeyZ") {
        e.shiftKey ? app.editor_redo() : app.editor_undo();
        e.preventDefault();
        refreshEditor();
        return;
      }
      if (ctrl && e.code === "KeyY") {
        app.editor_redo();
        e.preventDefault();
        refreshEditor();
        return;
      }
      if (!ctrl && TOOL_KEYS[e.code] && !e.repeat) {
        app.editor_tool(TOOL_KEYS[e.code]);
        refreshEditor();
      }
    }
    app.key_down(e.code, e.repeat);
  });
  window.addEventListener("keyup", (e) => app.key_up(e.code));

  // Mouse positions in canvas pixels, which may be fewer than CSS pixels
  // (or more, on a high-density screen). The canvas captures the pointer
  // while a button is down, so drags that leave it still end.
  const logical = (e) => {
    const r = canvas.getBoundingClientRect();
    return [
      ((e.clientX - r.left) * canvas.width) / r.width,
      ((e.clientY - r.top) * canvas.height) / r.height,
    ];
  };
  canvas.addEventListener("pointerdown", (e) => {
    e.preventDefault();
    canvas.focus();
    canvas.setPointerCapture(e.pointerId);
    app.mouse_down(...logical(e), e.button);
  });
  canvas.addEventListener("pointerup", (e) => {
    app.mouse_up(...logical(e), e.button);
    // The picker changes the brushes, and any change can be undone.
    if (mode === "editor") refreshEditor();
  });
  canvas.addEventListener("pointermove", (e) => app.pointer(...logical(e)));
  canvas.addEventListener("contextmenu", (e) => e.preventDefault());

  // The wheel zooms about the pointer, a level per notch. Trackpads send
  // many small deltas, so they're added up into notches.
  const NOTCH = 1;
  let wheel = 0;
  canvas.addEventListener(
    "wheel",
    (e) => {
      e.preventDefault();
      // In pixels, whatever the event's unit (lines or pages).
      wheel += e.deltaY * [1, 40, 800][e.deltaMode];
      const steps = Math.trunc(wheel / NOTCH);
      if (steps !== 0) {
        wheel -= steps * NOTCH;
        // Scrolling down (positive) zooms out.
        app.zoom(-steps, ...logical(e));
      }
    },
    { passive: false },
  );

  const loop = (t) => {
    app.frame(t);
    // A level from a server: its tile sheet once its header's come, and
    // back to the menu if it couldn't be had.
    if (opening) {
      syncSheet();
      const error = app.take_error();
      if (error) {
        warn(`Couldn't open ${levelUrl ?? "that world"}: ${error}`);
        showMenu();
      }
      if (!app.opening()) opening = false;
    }
    requestAnimationFrame(loop);
  };
  requestAnimationFrame(loop);
}

main().catch((e) => fail(String(e?.stack ?? e)));
