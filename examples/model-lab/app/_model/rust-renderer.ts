// WebAssembly cannot hold WebGL objects. Keep their lifetimes inside one canvas
// host; the Rust module supplies the shader program, camera and draw commands.
import { readWasm } from "forge:mesh";
import type { MeshData } from "forge:mesh";
import rendererUrl from "forge:wasm/renderer";

type Color = [number, number, number];
type RendererAbi = WebAssembly.Exports & {
  memory: WebAssembly.Memory;
  renderer_abi_version(): number;
  renderer_init(): void;
  renderer_indices(count: number): number;
  renderer_edges(): number;
  renderer_mesh(count: number, x0: number, y0: number, z0: number, x1: number, y1: number, z1: number, ...color: Color): number;
  renderer_resize(width: number, height: number): void;
  renderer_orbit(x: number, y: number): void;
  renderer_zoom(factor: number): void;
  renderer_reset(): void;
  renderer_draw(rotation: number, wire: number, normals: number): void;
};

function checkAbi(exports: WebAssembly.Exports): asserts exports is RendererAbi {
  if (!(exports.memory instanceof WebAssembly.Memory)) throw new Error("Missing Rust renderer memory.");
  for (const name of ["renderer_abi_version", "renderer_init", "renderer_indices", "renderer_edges", "renderer_mesh",
    "renderer_resize", "renderer_orbit", "renderer_zoom", "renderer_reset", "renderer_draw"]) {
    if (typeof exports[name] !== "function") throw new Error(`Missing Rust renderer export: ${name}.`);
  }
  if (typeof exports.renderer_abi_version !== "function" || exports.renderer_abi_version() !== 1) {
    throw new Error("Unsupported Rust renderer ABI.");
  }
}

function buildProgram(gl: WebGL2RenderingContext, vertex: string, fragment: string) {
  const program = gl.createProgram();
  if (!program) throw new Error("Could not allocate a WebGL2 program.");
  const shaders: WebGLShader[] = [];
  try {
    const compile = (type: number, source: string) => {
      const shader = gl.createShader(type);
      if (!shader) throw new Error("Could not allocate a WebGL2 shader.");
      shaders.push(shader);
      gl.shaderSource(shader, source);
      gl.compileShader(shader);
      if (!gl.getShaderParameter(shader, gl.COMPILE_STATUS)) throw new Error(`Rust shader: ${gl.getShaderInfoLog(shader)}`);
      gl.attachShader(program, shader);
    };
    compile(gl.VERTEX_SHADER, vertex);
    compile(gl.FRAGMENT_SHADER, fragment);
    gl.linkProgram(program);
    if (!gl.getProgramParameter(program, gl.LINK_STATUS)) throw new Error(`Rust shader link: ${gl.getProgramInfoLog(program)}`);
    const uniform = (name: string) => {
      const location = gl.getUniformLocation(program, name);
      if (!location) throw new Error(`Missing shader uniform: ${name}.`);
      return location;
    };
    return {
      program, viewProjection: uniform("viewProjection"), centerInvRadius: uniform("centerInvRadius"),
      yaw: uniform("yaw"), baseColor: uniform("baseColor"), showNormals: uniform("showNormals"),
    };
  } catch (error) {
    gl.deleteProgram(program);
    throw error;
  } finally {
    for (const shader of shaders) gl.deleteShader(shader);
  }
}

function uploadGeometry(gl: WebGL2RenderingContext, mesh: MeshData) {
  const vao = gl.createVertexArray();
  if (!vao) throw new Error("Could not allocate a mesh vertex array.");
  const buffers = new Set<WebGLBuffer>();
  const dispose = () => {
    gl.deleteVertexArray(vao);
    for (const buffer of buffers) gl.deleteBuffer(buffer);
  };
  gl.bindVertexArray(vao);
  try {
    const upload = (target: number, data: Uint32Array | Float32Array) => {
      const buffer = gl.createBuffer();
      if (!buffer) throw new Error("Could not allocate a mesh buffer.");
      buffers.add(buffer);
      gl.bindBuffer(target, buffer);
      gl.bufferData(target, data, gl.STATIC_DRAW);
      return buffer;
    };
    const attribute = (location: number, data: Float32Array) => {
      upload(gl.ARRAY_BUFFER, data);
      gl.vertexAttribPointer(location, 3, gl.FLOAT, false, 0, 0);
      gl.enableVertexAttribArray(location);
    };
    attribute(0, mesh.positions);
    attribute(1, mesh.normals);
    const triangles = upload(gl.ELEMENT_ARRAY_BUFFER, mesh.indices);
    const checkUpload = () => {
      const error = gl.getError();
      if (error !== gl.NO_ERROR) throw new Error(`WebGL2 mesh upload failed (0x${error.toString(16)}).`);
    };
    checkUpload();
    // Shaded and normals-only views never allocate the wireframe index data.
    // The mesh owns a stable, validated snapshot of its triangle indices.
    let edges: WebGLBuffer | undefined;
    const wireBuffer = (abi: RendererAbi) => {
      if (edges) return edges;
      const count = mesh.indices.length;
      const ptr = abi.renderer_indices(count);
      if (!ptr) throw new Error("Could not allocate bounded wireframe storage for this mesh.");
      new Uint32Array(abi.memory.buffer, ptr, count).set(mesh.indices);
      const linePtr = abi.renderer_edges();
      if (!linePtr) throw new Error("Could not generate mesh edges.");
      const candidate = upload(gl.ELEMENT_ARRAY_BUFFER, new Uint32Array(abi.memory.buffer, linePtr, count * 2));
      try {
        checkUpload();
      } catch (error) {
        buffers.delete(candidate);
        gl.deleteBuffer(candidate);
        throw error;
      }
      edges = candidate;
      return edges;
    };
    return { vao, triangles, wireBuffer, dispose };
  } catch (error) {
    dispose();
    throw error;
  } finally {
    gl.bindVertexArray(null);
    gl.bindBuffer(gl.ARRAY_BUFFER, null);
  }
}

/**
 * Takes exclusive ownership of a fresh canvas's WebGL2 context. Supply a new
 * canvas for each backend switch. Dispose releases the drawing buffer when
 * WEBGL_lose_context is available. Mesh colors are linear RGB in [0,1].
 */
export async function createRustRenderer(canvas: HTMLCanvasElement, signal: AbortSignal) {
  const bytes = await readWasm(rendererUrl, signal);
  if (signal.aborted) throw signal.reason ?? new Error("Renderer loading was cancelled.");
  const gl = canvas.getContext("webgl2", { antialias: true, alpha: false, depth: true });
  if (!gl) throw new Error("The Rust renderer requires WebGL 2.");
  let wasm: RendererAbi | undefined;
  let pipeline: ReturnType<typeof buildProgram> | undefined;
  let geometry: ReturnType<typeof uploadGeometry> | undefined;
  let disposed = false;
  const decoder = new TextDecoder();
  // All imports are synchronous. Obtain fresh views whenever WASM calls us.
  const imports = {
    webgl2: {
      program(v: number, vBytes: number, f: number, fBytes: number) {
        const memory = active().memory.buffer;
        pipeline = buildProgram(gl, decoder.decode(new Uint8Array(memory, v, vBytes)), decoder.decode(new Uint8Array(memory, f, fBytes)));
      },
      present(mode: number, count: number, matrix: number, center: number, sin: number, cos: number, color: number, normals: number) {
        if (!pipeline || !geometry) throw new Error("The Rust renderer has no uploaded mesh.");
        if (gl.isContextLost()) throw new Error("The WebGL 2 context was lost.");
        gl.clear(gl.COLOR_BUFFER_BIT | gl.DEPTH_BUFFER_BIT);
        gl.useProgram(pipeline.program);
        gl.bindVertexArray(geometry.vao);
        const abi = active();
        // Building the optional line buffer can grow WASM memory. Get uniform
        // views only after that call, while Rust's frame pointers remain valid.
        const indices = mode === gl.LINES ? geometry.wireBuffer(abi) : geometry.triangles;
        const memory = abi.memory.buffer;
        gl.uniformMatrix4fv(pipeline.viewProjection, false, new Float32Array(memory, matrix, 16));
        gl.uniform4fv(pipeline.centerInvRadius, new Float32Array(memory, center, 4));
        gl.uniform2f(pipeline.yaw, sin, cos);
        gl.uniform3fv(pipeline.baseColor, new Float32Array(memory, color, 3));
        gl.uniform1i(pipeline.showNormals, normals);
        gl.bindBuffer(gl.ELEMENT_ARRAY_BUFFER, indices);
        gl.drawElements(mode, count, gl.UNSIGNED_INT, 0);
      },
    },
  };
  const dispose = () => {
    if (disposed) return;
    disposed = true;
    signal.removeEventListener("abort", dispose);
    gl.bindVertexArray(null);
    gl.useProgram(null);
    geometry?.dispose();
    if (pipeline) gl.deleteProgram(pipeline.program);
    geometry = undefined;
    pipeline = undefined;
    wasm = undefined;
    if (!gl.isContextLost()) gl.getExtension("WEBGL_lose_context")?.loseContext();
  };
  const active = () => {
    if (disposed || !wasm) throw new Error("The Rust renderer is unavailable.");
    return wasm;
  };
  try {
    const { instance } = await WebAssembly.instantiate(bytes, imports);
    if (signal.aborted) throw signal.reason ?? new Error("Renderer loading was cancelled.");
    const exports = instance.exports;
    checkAbi(exports);
    wasm = exports;
    wasm.renderer_init();
    gl.enable(gl.DEPTH_TEST);
    gl.clearColor(16/255, 26/255, 32/255, 1);
    signal.addEventListener("abort", dispose, { once: true });
  } catch (error) {
    dispose();
    throw error;
  }
  return {
    setMesh(mesh: MeshData, color: Color) {
      const abi = active();
      const count = mesh.indices.length;
      // Graphics allocation errors must leave the last displayed mesh intact.
      const next = uploadGeometry(gl, mesh);
      if (abi.renderer_mesh(count, ...mesh.bounds.min, ...mesh.bounds.max, ...color) !== 0) {
        next.dispose();
        throw new Error("Invalid bounds or color for the Rust renderer.");
      }
      geometry?.dispose();
      geometry = next;
    },
    resize(widthPx: number, heightPx: number) {
      const abi = active();
      canvas.width = Math.max(1, Math.round(widthPx));
      canvas.height = Math.max(1, Math.round(heightPx));
      gl.viewport(0, 0, gl.drawingBufferWidth, gl.drawingBufferHeight);
      abi.renderer_resize(gl.drawingBufferWidth, gl.drawingBufferHeight);
    },
    orbit(dxRadians: number, dyRadians: number) { active().renderer_orbit(dxRadians, dyRadians); },
    zoom(factor: number) { active().renderer_zoom(factor); },
    reset() { active().renderer_reset(); },
    draw(rotationRadians: number, wireframe: boolean, normals: boolean) {
      active().renderer_draw(rotationRadians, Number(wireframe), Number(normals));
    },
    dispose,
  };
}
