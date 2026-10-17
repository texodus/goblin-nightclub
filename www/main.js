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
    return Number.isInteger(given) && given >= 0 ? given >>> 0 : (Math.random() * 2 ** 32) >>> 0;
};

// Let the player pick a file; resolves to its text, or null if cancelled.
function pickFile() {
    return new Promise((resolve) => {
        fileInput.value = "";
        fileInput.onchange = async () => {
            const file = fileInput.files[0];
            resolve(file ? await file.text() : null);
        };
        fileInput.oncancel = () => resolve(null);
        fileInput.click();
    });
}

function download(name, text) {
    const url = URL.createObjectURL(new Blob([text], { type: "application/json" }));
    const a = Object.assign(document.createElement("a"), { href: url, download: name });
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

    // What's running: null (just the menu), "game" or "editor".
    let mode = null;
    const show = (next) => {
        mode = next;
        menuEl.hidden = true;
        editorEl.hidden = mode !== "editor";
        if (mode === "editor") refreshEditor();
        canvas.focus();
    };
    const showMenu = () => {
        app.menu();
        menuEl.hidden = false;
        editorEl.hidden = true;
        $("#menu-hint").hidden = mode === null;
    };

    // The start menu.
    menuEl.addEventListener("click", async (e) => {
        switch (e.target.dataset.action) {
            case "new": {
                // Generation parameters may be given in the URL as JSON,
                // e.g. ?params={"water_share":0.2,"ramp_spacing":0}.
                const params = new URLSearchParams(location.search).get("params");
                try {
                    if (params) app.new_game_with(seed(), params);
                    else app.new_game(seed());
                } catch (err) {
                    warn(`Bad generation parameters: ${err.message ?? err}`);
                    return;
                }
                show("game");
                break;
            }
            case "editor":
                app.editor();
                show("editor");
                break;
            case "load": {
                const text = await pickFile();
                if (text === null) return;
                try {
                    app.load_game(text, seed());
                    show("game");
                } catch (err) {
                    warn(`Couldn't load that level: ${err.message ?? err}`);
                }
                break;
            }
        }
    });
    showMenu();

    // The editor's interface: the palette from the tile sheet, tools,
    // shapes, layers. `refreshEditor` makes it show the editor's state.
    const sheet = new Image();
    sheet.src = "assets/tiles.png";
    await sheet.decode().catch(() => {});
    const SWATCH = 32;
    const CELL = 16;
    const palette = JSON.parse(app.editor_palette());
    const paint = (el, entry) => {
        el.classList.toggle("void", !entry.cell);
        el.title = entry.name;
        if (!entry.cell) {
            el.style.backgroundImage = "";
            return;
        }
        const k = SWATCH / CELL;
        el.style.backgroundImage = `url(${sheet.src})`;
        el.style.backgroundSize = `${sheet.width * k}px ${sheet.height * k}px`;
        el.style.backgroundPosition = `${-entry.cell[0] * SWATCH}px ${-entry.cell[1] * SWATCH}px`;
    };
    const swatches = palette.map((entry, i) => {
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

    let shownDepth = 0;
    function refreshEditor() {
        const state = JSON.parse(app.editor_state() ?? "null");
        if (!state) return;
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
                b.title = k === state.depth - 1 ? "Top layer" : k === 0 ? "Bottom layer" : `Layer ${k + 1}`;
                b.dataset.layer = k;
                layers.append(b);
            }
        }
        for (const b of document.querySelectorAll("[data-layer]")) {
            b.classList.toggle("on", Number(b.dataset.layer) === state.layer);
        }
        $("#status").textContent = `${state.cols}×${state.rows}, ${state.depth} layers`;
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
                const text = await pickFile();
                if (text === null) return;
                try {
                    app.editor_load(text);
                    shownDepth = 0;
                } catch (err) {
                    warn(`Couldn't open that level: ${err.message ?? err}`);
                }
                break;
            }
            case "save":
                download("level.json", app.editor_save());
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
    // pauses and C centers the camera on the player; in the editor, the
    // tool shortcuts and undo / redo.
    const GAME_KEYS = new Set(["ArrowUp", "ArrowDown", "ArrowLeft", "ArrowRight", "Space"]);
    const TOOL_KEYS = { KeyB: "pencil", KeyR: "rect", KeyF: "fill", KeyI: "pick" };
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
        requestAnimationFrame(loop);
    };
    requestAnimationFrame(loop);
}

main().catch((e) => fail(String(e?.stack ?? e)));
