// forge:file: files in and out of the browser, and self-saving single-file apps.
//
// openFile / openFiles   pick files (no <input> plumbing)
// saveFile               download, or write through the File System Access picker
// onDrop / useDrop       drag-and-drop anywhere on the page (or one element)
// saveSelf / useDocument the app writes a copy of its own HTML file with your state
//                        embedded; opening that copy starts from the saved state
import { rt, STATE_ID, APP_SCRIPT_ID, ROOT_ID } from "./shared.js";

const browserOnly = (name) => {
  if (rt.server) throw new Error(`forge:file ${name}() runs in the browser, not during server rendering`);
};

const TYPES = {
  json: "application/json",
  html: "text/html",
  htm: "text/html",
  txt: "text/plain",
  md: "text/markdown",
  csv: "text/csv",
  svg: "image/svg+xml",
  png: "image/png",
  jpg: "image/jpeg",
  jpeg: "image/jpeg",
  webp: "image/webp",
  gif: "image/gif",
  wav: "audio/wav",
  mp3: "audio/mpeg",
  glb: "model/gltf-binary",
};
const typeOf = (name) => TYPES[name.split(".").pop()?.toLowerCase() ?? ""];

function toBlob(data, name, type) {
  if (data instanceof Blob) return type && data.type !== type ? new Blob([data], { type }) : data;
  type ??= typeOf(name);
  if (typeof data === "string") return new Blob([data], { type: type ?? "text/plain" });
  if (data instanceof ArrayBuffer || ArrayBuffer.isView(data)) return new Blob([data], { type: type ?? "application/octet-stream" });
  return new Blob([JSON.stringify(data, null, 2)], { type: type ?? "application/json" });
}

/** Ask the user for files. Resolves `[]` if they cancel. */
export function openFiles({ accept = "", multiple = true } = {}) {
  browserOnly("openFiles");
  return new Promise((resolve) => {
    const input = Object.assign(document.createElement("input"), { type: "file", accept, multiple });
    input.style.display = "none";
    const done = (files) => {
      input.remove();
      resolve(files);
    };
    input.addEventListener("change", () => done([...(input.files ?? [])]), { once: true });
    input.addEventListener("cancel", () => done([]), { once: true });
    document.body.append(input);
    input.click();
  });
}

/** Ask the user for one file. Resolves `null` if they cancel. */
export async function openFile(options = {}) {
  return (await openFiles({ ...options, multiple: false }))[0] ?? null;
}

const handles = new Map();

/**
 * Save `data` (Blob, string, bytes, or any JSON value) as a file called `name`.
 * By default this is a download. With `picker: true` and a browser that has the File
 * System Access API, the user picks where to write, and later saves with the same `key`
 * write there again without asking. Resolves `false` if the user cancelled.
 */
export async function saveFile(data, name, { type, picker = false, key } = {}) {
  browserOnly("saveFile");
  const blob = toBlob(data, name, type);
  if (picker && typeof showSaveFilePicker === "function") {
    try {
      let handle = key != null ? handles.get(key) : undefined;
      if (!handle) {
        const ext = name.includes(".") ? "." + name.split(".").pop() : "";
        const types = ext && blob.type ? [{ accept: { [blob.type]: [ext] } }] : undefined;
        handle = await showSaveFilePicker({ suggestedName: name, types });
        if (key != null) handles.set(key, handle);
      }
      const out = await handle.createWritable();
      await out.write(blob);
      await out.close();
      return true;
    } catch (e) {
      if (e?.name === "AbortError") return false;
      throw e;
    }
  }
  const a = Object.assign(document.createElement("a"), { href: URL.createObjectURL(blob), download: name });
  a.style.display = "none";
  document.body.append(a);
  a.click();
  a.remove();
  setTimeout(() => URL.revokeObjectURL(a.href), 30_000);
  return true;
}

/**
 * Call `onFiles(files)` when files are dropped on `target` (default: the whole window).
 * `onOver(true | false)` reports a drag hovering over it. Returns a cleanup function.
 */
export function onDrop(onFiles, { target, onOver } = {}) {
  browserOnly("onDrop");
  const el = target ?? window;
  let depth = 0;
  const files = (e) => Array.from(e.dataTransfer?.types ?? []).includes("Files");
  const handlers = {
    dragenter: (e) => {
      if (!files(e)) return;
      e.preventDefault();
      if (depth++ === 0) onOver?.(true);
    },
    dragover: (e) => {
      if (!files(e)) return;
      e.preventDefault();
      e.dataTransfer.dropEffect = "copy";
    },
    dragleave: (e) => {
      if (files(e) && depth > 0 && --depth === 0) onOver?.(false);
    },
    drop: (e) => {
      if (!files(e)) return;
      e.preventDefault();
      depth = 0;
      onOver?.(false);
      onFiles(Array.from(e.dataTransfer.files));
    },
  };
  for (const [type, fn] of Object.entries(handlers)) el.addEventListener(type, fn);
  return () => {
    for (const [type, fn] of Object.entries(handlers)) el.removeEventListener(type, fn);
  };
}

/** React: drop files anywhere (or on `ref.current`). Returns whether a drag is hovering. */
export function useDrop(onFiles, ref) {
  const { useState, useEffect, useRef } = rt.React;
  const [over, setOver] = useState(false);
  const latest = useRef(onFiles);
  latest.current = onFiles;
  useEffect(() => onDrop((f) => latest.current(f), { target: ref?.current ?? undefined, onOver: setOver }), [ref]);
  return over;
}

// ---- Self-saving documents (single-html builds) ----

/** The state saved into this file by `saveSelf`, or `undefined` (always, on the server). */
export function embeddedState() {
  if (rt.server) return undefined;
  const doc = rt.selfDocument;
  if (doc) return doc.state;
  const el = document.getElementById(STATE_ID);
  try {
    return el ? JSON.parse(el.textContent) : undefined;
  } catch {
    return undefined;
  }
}

/** The file name this page was opened from, or `index.html`. */
export function fileName() {
  if (rt.server) return "index.html";
  try {
    return decodeURIComponent(location.pathname.split("/").pop() || "") || "index.html";
  } catch {
    return "index.html";
  }
}

const json = (value) => JSON.stringify(value).replace(/</g, "\\u003c");

/** This single-file app as HTML, with `state` embedded (no state: the original file). */
export function selfHtml(state) {
  browserOnly("selfHtml");
  const doc = rt.selfDocument;
  const app = document.getElementById(APP_SCRIPT_ID);
  if (!doc || !app) throw new Error("forge:file saveSelf() needs a single-html build (forge build --target single-html)");
  const saved = state === undefined ? "" : `<script type="application/json" id="${STATE_ID}">${json(state)}</script>`;
  const lang = document.documentElement.getAttribute("lang");
  return (
    `<!doctype html><html${lang == null ? "" : ` lang="${lang.replace(/[&"]/g, (c) => (c === "&" ? "&amp;" : "&quot;"))}"`}>` +
    `<head>${doc.head}${saved}</head><body><div id="${ROOT_ID}">${doc.root}</div>${app.outerHTML}</body></html>`
  );
}

/**
 * Write a copy of this single-file app with `state` embedded. Opening the copy starts from
 * that state (`embeddedState()`, `useDocument`). Options as for `saveFile`; the name
 * defaults to the current file's. With `picker: true`, the first save asks where to write
 * (pick this same file to save in place) and later saves write there directly.
 */
export function saveSelf(state, { name, picker = false } = {}) {
  return saveFile(new Blob([selfHtml(state)], { type: "text/html" }), name ?? fileName(), { picker, key: "forge:self" });
}

/**
 * React: document state for a self-saving app. Starts from the state saved in this file
 * (or `initial`), tracks unsaved changes, saves with Ctrl/Cmd+S, and warns before closing
 * with unsaved changes. Returns `[state, setState, { save, dirty }]`.
 */
export function useDocument(initial, { name, picker = false, shortcut = true } = {}) {
  const { useState, useEffect, useCallback, useRef } = rt.React;
  const [state, setState] = useState(() => {
    const saved = embeddedState();
    return saved === undefined ? initial : saved;
  });
  const [dirty, setDirty] = useState(false);
  const latest = useRef(state);
  latest.current = state;
  const options = useRef({ name, picker });
  options.current = { name, picker };
  const set = useCallback((next) => {
    setState(next);
    setDirty(true);
  }, []);
  const save = useCallback(async () => {
    const ok = await saveSelf(latest.current, options.current);
    if (ok) setDirty(false);
    return ok;
  }, []);
  useEffect(() => {
    if (!shortcut) return;
    const onKey = (e) => {
      if ((e.ctrlKey || e.metaKey) && !e.shiftKey && !e.altKey && e.key.toLowerCase() === "s") {
        e.preventDefault();
        save();
      }
    };
    addEventListener("keydown", onKey);
    return () => removeEventListener("keydown", onKey);
  }, [shortcut, save]);
  useEffect(() => {
    if (!dirty) return;
    const warn = (e) => {
      e.preventDefault();
      e.returnValue = "";
    };
    addEventListener("beforeunload", warn);
    return () => removeEventListener("beforeunload", warn);
  }, [dirty]);
  return [state, set, { save, dirty }];
}
