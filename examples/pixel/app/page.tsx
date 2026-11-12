// Pixel Pad: a pixel-art editor that is one HTML file and saves itself.
//
// forge:file does the file work: `useDocument` starts from the art saved inside this file,
// tracks unsaved changes and saves a copy of the whole app with Ctrl/Cmd+S; `useDrop` and
// `openFile` import images; `saveFile` exports PNG and JSON.
import { useEffect, useRef, useState } from "react";
import { embeddedState, fileName, openFile, saveFile, useDocument, useDrop } from "forge:file";
import "./pixel.css";

export const meta = { title: "Pixel Pad", description: "A pixel-art editor in one self-saving HTML file." };

const N = 32;
const CELL = 16;
const EMPTY = -1;
const PALETTE = [
  "#1a1c2c", "#5d275d", "#b13e53", "#ef7d57", "#ffcd75", "#a7f070", "#38b764", "#257179",
  "#29366f", "#3b5dc9", "#41a6f6", "#73eff7", "#f4f4f4", "#94b0c2", "#566c86", "#333c57",
];

type Art = { palette: string[]; pixels: number[] };
type Tool = "pen" | "fill" | "erase";

// The starter sprite: a heart from (x² + y² - 1)³ - x²y³ ≤ 0.
function heart(): number[] {
  return Array.from({ length: N * N }, (_, i) => {
    const x = ((i % N) - 15.5) / 11;
    const y = (15.5 - Math.floor(i / N)) / 11 + 0.15;
    const v = (x * x + y * y - 1) ** 3 - x * x * y ** 3;
    return v <= 0 ? (v > -0.02 ? 0 : x < -0.25 && y > 0.35 ? 4 : 2) : EMPTY;
  });
}

const hex = (c: string) => [1, 3, 5].map((i) => parseInt(c.slice(i, i + 2), 16));

function nearest(palette: string[], r: number, g: number, b: number): number {
  let best = 0;
  let bestD = Infinity;
  palette.forEach((c, i) => {
    const [pr, pg, pb] = hex(c);
    const d = (pr - r) ** 2 * 0.3 + (pg - g) ** 2 * 0.59 + (pb - b) ** 2 * 0.11;
    if (d < bestD) [best, bestD] = [i, d];
  });
  return best;
}

// Scale any image to the grid and map it onto the palette.
async function importImage(file: File, palette: string[]): Promise<number[]> {
  const bitmap = await createImageBitmap(file);
  const c = document.createElement("canvas");
  c.width = c.height = N;
  const g = c.getContext("2d", { willReadFrequently: true })!;
  const s = Math.min(N / bitmap.width, N / bitmap.height);
  g.drawImage(bitmap, (N - bitmap.width * s) / 2, (N - bitmap.height * s) / 2, bitmap.width * s, bitmap.height * s);
  const { data } = g.getImageData(0, 0, N, N);
  return Array.from({ length: N * N }, (_, i) => (data[i * 4 + 3] < 128 ? EMPTY : nearest(palette, data[i * 4], data[i * 4 + 1], data[i * 4 + 2])));
}

function fill(pixels: number[], at: number, color: number): number[] {
  const from = pixels[at];
  if (from === color) return pixels;
  const out = pixels.slice();
  const stack = [at];
  while (stack.length) {
    const i = stack.pop()!;
    if (out[i] !== from) continue;
    out[i] = color;
    const x = i % N;
    if (x > 0) stack.push(i - 1);
    if (x < N - 1) stack.push(i + 1);
    if (i >= N) stack.push(i - N);
    if (i < N * N - N) stack.push(i + N);
  }
  return out;
}

function draw(canvas: HTMLCanvasElement, art: Art, grid: boolean) {
  const g = canvas.getContext("2d")!;
  g.clearRect(0, 0, canvas.width, canvas.height);
  const cell = canvas.width / N;
  art.pixels.forEach((p, i) => {
    const x = (i % N) * cell;
    const y = Math.floor(i / N) * cell;
    if (p === EMPTY) {
      if (!grid) return;
      g.fillStyle = ((i % N) + Math.floor(i / N)) % 2 ? "#23263a" : "#1c1f30";
    } else g.fillStyle = art.palette[p];
    g.fillRect(x, y, cell, cell);
  });
  if (grid) {
    g.strokeStyle = "rgba(255,255,255,0.06)";
    g.beginPath();
    for (let k = 1; k < N; k++) {
      g.moveTo(k * cell + 0.5, 0);
      g.lineTo(k * cell + 0.5, canvas.height);
      g.moveTo(0, k * cell + 0.5);
      g.lineTo(canvas.width, k * cell + 0.5);
    }
    g.stroke();
  }
}

export default function PixelPad() {
  const [art, setArt, { save, dirty }] = useDocument<Art>({ palette: PALETTE, pixels: heart() });
  const [tool, setTool] = useState<Tool>("pen");
  const [color, setColor] = useState(2);
  const [restored] = useState(() => embeddedState() !== undefined);
  const [note, setNote] = useState("");
  const canvas = useRef<HTMLCanvasElement>(null);
  const history = useRef<Art[]>([]);
  const painting = useRef(false);
  const artRef = useRef(art);
  artRef.current = art;

  useEffect(() => {
    if (canvas.current) draw(canvas.current, art, true);
  }, [art]);

  const commit = (next: Art) => {
    history.current = [...history.current.slice(-99), artRef.current];
    setArt(next);
  };
  const undo = () => {
    const prev = history.current.pop();
    if (prev) setArt(prev);
  };

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if ((e.ctrlKey || e.metaKey) && e.key.toLowerCase() === "z") {
        e.preventDefault();
        undo();
      } else if (!e.ctrlKey && !e.metaKey && !(e.target instanceof HTMLInputElement)) {
        const t = ({ b: "pen", f: "fill", e: "erase" } as const)[e.key.toLowerCase() as "b" | "f" | "e"];
        if (t) setTool(t);
      }
    };
    addEventListener("keydown", onKey);
    return () => removeEventListener("keydown", onKey);
  }, []);

  const importFile = async (file: File | null | undefined) => {
    if (!file) return;
    try {
      if (file.name.endsWith(".json")) {
        const doc = JSON.parse(await file.text()) as Art;
        if (!Array.isArray(doc.pixels) || doc.pixels.length !== N * N) throw new Error("not a Pixel Pad file");
        commit({ palette: doc.palette ?? PALETTE, pixels: doc.pixels });
      } else {
        commit({ ...artRef.current, pixels: await importImage(file, artRef.current.palette) });
      }
      setNote(`Imported ${file.name}`);
    } catch (e) {
      setNote(`Could not import ${file.name}: ${(e as Error).message}`);
    }
  };
  const dragging = useDrop((files) => importFile(files[0]));

  const cellAt = (e: React.PointerEvent<HTMLCanvasElement>) => {
    const r = e.currentTarget.getBoundingClientRect();
    const x = Math.floor(((e.clientX - r.left) / r.width) * N);
    const y = Math.floor(((e.clientY - r.top) / r.height) * N);
    return x >= 0 && y >= 0 && x < N && y < N ? y * N + x : -1;
  };
  const paint = (i: number) => {
    if (i < 0) return;
    const value = tool === "erase" ? EMPTY : color;
    setArt((a) => {
      if (a.pixels[i] === value) return a;
      const pixels = a.pixels.slice();
      pixels[i] = value;
      return { ...a, pixels };
    });
  };

  const exportPng = () => {
    const c = document.createElement("canvas");
    c.width = c.height = N * CELL;
    draw(c, art, false);
    c.toBlob((blob) => blob && saveFile(blob, fileName().replace(/\.html?$/i, "") + ".png"));
  };
  const filled = art.pixels.filter((p) => p !== EMPTY).length;

  return (
    <main className={dragging ? "dragging" : ""}>
      <header>
        <h1>Pixel Pad</h1>
        <span className={dirty ? "badge unsaved" : "badge"} id="status">
          {dirty ? "unsaved changes" : restored ? "opened with saved art" : "new"}
        </span>
        <span className="grow" />
        <button id="save" className="primary" onClick={save} title="Ctrl/Cmd+S">
          Save this file
        </button>
      </header>

      <section className="tools">
        {(["pen", "fill", "erase"] as Tool[]).map((t) => (
          <button key={t} data-tool={t} className={tool === t ? "on" : ""} onClick={() => setTool(t)}>
            {t}
          </button>
        ))}
        <button onClick={undo} title="Ctrl/Cmd+Z">
          undo
        </button>
        <button onClick={() => commit({ ...art, pixels: Array(N * N).fill(EMPTY) })}>clear</button>
      </section>

      <div className="palette">
        {art.palette.map((c, i) => (
          <label key={i} data-color={i} className={color === i ? "swatch on" : "swatch"} style={{ background: c }} onClick={() => setColor(i)}>
            {color === i && (
              <input
                type="color"
                value={c}
                onChange={(e) => setArt({ ...art, palette: art.palette.map((p, k) => (k === i ? e.target.value : p)) })}
              />
            )}
          </label>
        ))}
      </div>

      <canvas
        id="canvas"
        ref={canvas}
        width={N * CELL}
        height={N * CELL}
        data-filled={filled}
        onPointerDown={(e) => {
          const i = cellAt(e);
          if (i < 0) return;
          history.current = [...history.current.slice(-99), art];
          if (tool === "fill") return setArt({ ...art, pixels: fill(art.pixels, i, color) });
          painting.current = true;
          e.currentTarget.setPointerCapture(e.pointerId);
          paint(i);
        }}
        onPointerMove={(e) => painting.current && paint(cellAt(e))}
        onPointerUp={() => (painting.current = false)}
      />

      <footer>
        <span id="count">{filled} px</span>
        <button onClick={async () => importFile(await openFile({ accept: "image/*,.json" }))}>Import image…</button>
        <button onClick={exportPng}>Export PNG</button>
        <button onClick={() => saveFile(art, fileName().replace(/\.html?$/i, "") + ".json")}>Export JSON</button>
        <span className="note">{note || "Drop an image anywhere to import it."}</span>
      </footer>
    </main>
  );
}
