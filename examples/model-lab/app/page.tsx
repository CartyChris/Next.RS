import { useEffect, useRef, useState } from "react";
import { Alert, Badge, Button, Card, CardContent, Checkbox, Container, Heading, PageHeader, Row, Select, Slider, Stack, Stat, Text, TextInput } from "forge:ui";
import type { MeshKind } from "forge:mesh";
import { createModelView, initialParameters } from "./_model/view";
import type { ModelParameters, ModelStats, ModelView } from "./_model/view";

export const ssr = false;

function parseKind(value: string): MeshKind {
  switch (value) {
    case "sphere": case "torus": case "rock": case "terrain": return value;
    default: throw new Error("Choose one of the available model shapes.");
  }
}

function errorMessage(error: unknown): string {
  return error instanceof Error ? error.message : "An unexpected model error occurred.";
}

export default function ModelLab() {
  const surface = useRef<HTMLDivElement>(null);
  const view = useRef<ModelView | null>(null);
  const [parameters, setParameters] = useState<ModelParameters>(initialParameters);
  const [stats, setStats] = useState<ModelStats | null>(null);
  const [status, setStatus] = useState<"loading" | "ready" | "error">("loading");
  const [message, setMessage] = useState("Loading the Rust generator and graphics renderer…");
  const [wireframe, setWireframe] = useState(false);
  const [normals, setNormals] = useState(false);
  const [paused, setPaused] = useState(false);
  const [exporting, setExporting] = useState(false);
  const [notice, setNotice] = useState<string | null>(null);

  useEffect(() => {
    const container = surface.current;
    if (!container) return;
    const controller = new AbortController();
    const initiallyPaused = matchMedia("(prefers-reduced-motion: reduce)").matches;
    setPaused(initiallyPaused);
    const fail = (error: Error) => {
      if (controller.signal.aborted) return;
      setStatus("error");
      setMessage(`${error.message} Try reloading, or open the page in a browser with WebAssembly and WebGL 2 enabled.`);
    };
    createModelView(container, { signal: controller.signal, paused: initiallyPaused, onStats: setStats, onError: fail })
      .then((created) => {
        if (controller.signal.aborted) { created.dispose(); return; }
        view.current = created;
        setStatus("ready");
        setMessage("Model ready");
      })
      .catch((error: unknown) => { if (!controller.signal.aborted) fail(new Error(errorMessage(error))); });
    return () => { controller.abort(); view.current?.dispose(); view.current = null; };
  }, []);

  const ready = status === "ready";
  const chooseKind = (kind: MeshKind) => {
    setParameters((current) => {
      const seedValid = Number.isInteger(current.seed) && current.seed >= 0 && current.seed <= 0xffffffff;
      const seeded = kind === "rock" || kind === "terrain";
      return { ...current, kind, seed: seeded || seedValid ? current.seed : initialParameters.seed };
    });
  };
  const generate = () => {
    if (!view.current || !ready) return;
    try {
      view.current.generate(parameters);
      setNotice(null);
      setMessage("Model ready");
    } catch (error) { setNotice(errorMessage(error)); }
  };
  const exportModel = async () => {
    if (!view.current || !ready) return;
    const currentView = view.current;
    setExporting(true);
    try {
      await currentView.exportGlb();
      if (view.current === currentView) setNotice("GLB exported with positions, normals, UVs, and a PBR material.");
    } catch (error) {
      if (view.current === currentView) setNotice(errorMessage(error));
    } finally { if (view.current === currentView) setExporting(false); }
  };
  const extent = stats ? stats.bounds.max.map((value, axis) => (value - stats.bounds.min[axis]).toFixed(2)).join(" × ") : "—";

  return (
    <Container size="wide" className="model-lab">
      <PageHeader title="Model lab" description="Shape, inspect, and export procedural 3D models."
        eyebrow={<Text as="span" size="sm" tone="muted">NEXT.RS / PROCEDURAL STUDIO</Text>}
        actions={<Badge tone="neutral">Rust · WebAssembly</Badge>} />
      <Text id="model-status" className="model-status" size="sm" tone={status === "error" ? "default" : "muted"}
        role="status" aria-live="polite" data-state={status}>{message}</Text>

      <main className="model-workspace">
        <section className="model-preview" aria-label="Model preview">
          <div className="model-surface" ref={surface} />
          <div className="model-preview-toolbar">
            <Row gap="sm">
              <Button id="model-pause" variant="secondary" size="sm" disabled={!ready} aria-pressed={paused}
                onClick={() => { setPaused(!paused); view.current?.setPaused(!paused); }}>{paused ? "Resume rotation" : "Pause rotation"}</Button>
              <Button id="model-reset" variant="ghost" size="sm" disabled={!ready} onClick={() => view.current?.reset()}>Reset view</Button>
            </Row>
            <Row gap="xs">
              <Button id="model-zoom-out" variant="ghost" size="sm" aria-label="Zoom out" disabled={!ready} onClick={() => view.current?.zoom(1.2)}>−</Button>
              <Button id="model-zoom-in" variant="ghost" size="sm" aria-label="Zoom in" disabled={!ready} onClick={() => view.current?.zoom(1 / 1.2)}>+</Button>
            </Row>
          </div>
          <div className="model-preview-caption">
            <Text size="xs" tone="muted">Drag to orbit · Pinch or scroll to zoom</Text>
            <Text size="xs" tone="muted">Y-up · metres</Text>
          </div>
        </section>

        <Card className="model-controls">
          <CardContent>
            <Stack gap="lg">
              <Heading level={2} size="sm">Geometry</Heading>
              <Select id="model-kind" label="Shape" value={parameters.kind} disabled={!ready}
                onChange={(event) => chooseKind(parseKind(event.target.value))}>
                <option value="torus">Torus</option><option value="sphere">Sphere</option><option value="rock">Seeded rock</option><option value="terrain">Terrain patch</option>
              </Select>
              <Select id="model-detail" label="Detail" value={parameters.detail} disabled={!ready}
                description="Higher detail adds vertices. Choose 16 or 32 for smaller devices."
                onChange={(event) => setParameters({ ...parameters, detail: Number(event.target.value) })}>
                {[8, 16, 32, 48, 64, 96, 128].map((detail) => <option key={detail} value={detail}>{detail} segments</option>)}
              </Select>
              <TextInput id="model-seed" label="Seed" type="number" min={0} max={4294967295} step={1} value={Number.isFinite(parameters.seed) ? parameters.seed : ""}
                disabled={!ready || (parameters.kind !== "rock" && parameters.kind !== "terrain")}
                description="Reproduce a rock or terrain with the same seed."
                onChange={(event) => setParameters({ ...parameters, seed: event.target.valueAsNumber })} />
              <Slider id="model-variation" label={parameters.kind === "torus" ? "Tube thickness" : "Surface variation"}
                min={0} max={1} step={0.01} value={parameters.variation} showValue={(value) => `${Math.round(value * 100)}%`}
                disabled={!ready || parameters.kind === "sphere"}
                onChange={(event) => setParameters({ ...parameters, variation: event.target.valueAsNumber })} />
              <Slider id="model-scale" label="Scale" min={0.25} max={3} step={0.05} value={parameters.scale} showValue={(value) => `${value.toFixed(2)} m`}
                disabled={!ready} onChange={(event) => setParameters({ ...parameters, scale: event.target.valueAsNumber })} />
              <Button id="model-regenerate" fullWidth disabled={!ready || exporting} onClick={generate}>Generate model</Button>
              <div className="model-inspection">
                <Heading level={2} size="sm">Inspect</Heading>
                <Checkbox id="model-wireframe" label="Wireframe" checked={wireframe} disabled={!ready}
                  onChange={(event) => { setWireframe(event.target.checked); view.current?.setWireframe(event.target.checked); }} />
                <Checkbox id="model-normals" label="Surface normals" checked={normals} disabled={!ready}
                  onChange={(event) => { setNormals(event.target.checked); view.current?.setNormals(event.target.checked); }} />
              </div>
              <Button id="model-export" variant="secondary" fullWidth disabled={!ready || exporting} loading={exporting} onClick={exportModel}>Export GLB</Button>
              <Text size="xs" tone="muted">Exports the current model at its original orientation. Preview modes are excluded.</Text>
            </Stack>
          </CardContent>
        </Card>
      </main>

      <div className="model-statistics" aria-label="Model statistics">
        <Stat label="Vertices" value={<span id="model-vertices">{stats?.vertices.toLocaleString() ?? "—"}</span>} />
        <Stat label="Triangles" value={<span id="model-triangles">{stats?.triangles.toLocaleString() ?? "—"}</span>} />
        <Stat label="Generated in" value={<span id="model-generation-ms">{stats ? `${stats.generationMs.toFixed(1)} ms` : "—"}</span>} />
        <Stat label="Renderer" value={<span id="model-renderer">{stats ? (stats.backend === "webgpu" ? "WebGPU" : "WebGL 2") : "—"}</span>} />
      </div>
      <Row justify="between" className="model-status-row">
        <Text size="xs" tone="muted"><span id="model-memory">{stats ? `${(stats.memoryBytes / 1048576).toFixed(2)} MiB WASM` : ""}</span>{stats ? ` · ${extent} m` : ""}</Text>
      </Row>
      {notice && <Alert live="polite">{notice}</Alert>}
      {stats?.fallbackReason && <Text size="xs" tone="muted" className="model-backend-note">{stats.fallbackReason}</Text>}
    </Container>
  );
}
