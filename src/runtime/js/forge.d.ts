// Type declarations for Forge's virtual modules. Written to .forge/types/forge.d.ts on every build.

declare module "forge:navigation" {
  import type { ComponentProps, ReactElement } from "react";
  export function notFound(message?: string): never;
  export function redirect(location: string, status?: 301 | 302 | 303 | 307 | 308): never;
  export function navigate(to: string, options?: { replace?: boolean; scroll?: boolean }): Promise<void>;
  export function prefetch(to: string): void;
  export function href(to: string): string;
  export function Link(props: Omit<ComponentProps<"a">, "href"> & { to?: string; href?: string; prefetch?: boolean }): ReactElement;
  export function useRouter(): {
    pathname: string;
    search: string;
    params: Record<string, string>;
    navigate: typeof navigate;
    prefetch: typeof prefetch;
  };
  export function useParams<T extends Record<string, string> = Record<string, string>>(): T;
  export function usePathname(): string;
  export function useSearchParams(): URLSearchParams;
  export function currentRoute(): { route: string; params: Record<string, string>; data: unknown; pathname: string; search: string } | null;
}

declare module "forge:data" {
  export function useData<T = unknown>(): T;
  export function getData<T = unknown>(): T | null;
  export function preloadData(to: string): void;
}

declare module "forge:cache" {
  export function cache<T>(key: string, fn: () => T | Promise<T>, options?: { ttl?: number; tags?: string[] }): Promise<T>;
  export function invalidate(keyOrTag: string): void;
  /** The stored value, or undefined. Server: shared by all workers. */
  export function get<T = unknown>(key: string): T | undefined;
  /** Store a JSON-serialisable value (default ttl: one day). */
  export function set<T>(key: string, value: T, options?: { ttl?: number; tags?: string[] }): T;
  /** Live keys starting with `prefix`, sorted. */
  export function keys(prefix?: string): string[];
}

declare module "forge:store" {
  export const store: {
    get<T>(key: string, fallback: T): T;
    set(key: string, value: unknown): void;
    remove(key: string): void;
  };
  export function usePersistentState<T>(key: string, initial: T): [T, (next: T | ((prev: T) => T)) => void];
}

declare module "forge:ai" {
  export function sse(response: Response): AsyncGenerator<{ event: string; data: string; id?: string }>;
  export function streamText(url: string | URL, init?: RequestInit): AsyncGenerator<string>;
  export function sseResponse(source: AsyncIterable<string>, init?: ResponseInit): Response;
  export function useStream(): {
    text: string;
    streaming: boolean;
    error: unknown;
    start(source: AsyncIterable<string>): Promise<string>;
    stop(): void;
  };

  export type Provider = "openai" | "anthropic" | "gemini" | "ollama" | "mock";
  export type ToolCall = { id: string; name: string; arguments: Record<string, unknown> };
  export type Message =
    | { role: "system" | "user"; content: string }
    | { role: "assistant"; content: string; toolCalls?: ToolCall[] }
    | { role: "tool"; content: string; toolCallId: string; name?: string };
  export type ToolSpec = { name: string; description?: string; parameters?: Record<string, unknown> };
  export type ChatEvent =
    | { type: "text"; text: string }
    | ({ type: "tool_call" } & ToolCall)
    | { type: "usage"; input: number; output: number }
    | { type: "done"; stop: string };
  export type ChatOptions = {
    provider?: Provider;
    model?: string;
    messages: Message[];
    system?: string;
    tools?: ToolSpec[];
    apiKey?: string;
    /** e.g. http://localhost:11434/v1 (Ollama, OpenAI-compatible), https://openrouter.ai/api/v1 */
    baseUrl?: string;
    temperature?: number;
    maxTokens?: number;
    signal?: AbortSignal;
    headers?: Record<string, string>;
    /** mock provider: milliseconds between words */
    delay?: number;
  };
  export function chat(options: ChatOptions): AsyncIterable<ChatEvent> & {
    text(): Promise<string>;
    result(): Promise<{ text: string; toolCalls: ToolCall[]; usage: { input: number; output: number } | null; stop: string | null }>;
  };
  export type Tool<A = any> = { description?: string; parameters?: Record<string, unknown>; run(args: A, ctx: { signal?: AbortSignal }): unknown };
  export function runAgent(
    options: Omit<ChatOptions, "tools"> & { tools?: Record<string, Tool>; maxSteps?: number },
  ): AsyncIterable<
    | ChatEvent
    | { type: "tool_result"; id: string; name: string; result: unknown }
    | { type: "final"; text: string; messages: Message[]; steps: number; stop?: "max_steps" }
  >;
}

declare module "forge:realtime" {
  export type SocketStatus = "connecting" | "open" | "reconnecting" | "closed" | "offline";
  export type SocketHandle = { send(message: unknown): boolean; close(code?: number, reason?: string): void; readonly status: SocketStatus };
  /** Browser: a reconnecting JSON WebSocket to a socket route. */
  export function connect(
    path: string,
    options?: {
      onMessage?: (message: any, socket: SocketHandle) => void;
      onOpen?: (socket: SocketHandle) => void;
      onClose?: (event: CloseEvent) => void;
      onStatus?: (status: SocketStatus) => void;
      reconnect?: boolean;
    },
  ): SocketHandle;
  /** React hook around connect(). */
  export function useSocket(path: string | null, onMessage: (message: any, socket: SocketHandle) => void): { status: SocketStatus; send(message: unknown): boolean };
  /** Server: send to every socket subscribed to a topic (any worker). Returns the number reached. */
  export function publish(topic: string, message: unknown): number;
  /** Server: number of sockets subscribed to a topic. */
  export function subscribers(topic: string): number;
}

/** The socket passed to a route's `socket` handlers. */
interface ForgeSocket<P extends Record<string, string> = Record<string, string>> {
  id: number;
  params: P;
  url: URL;
  /** Per-connection state. */
  data: Record<string, any>;
  send(message: unknown): boolean;
  close(code?: number, reason?: string): void;
  subscribe(topic: string): void;
  unsubscribe(topic: string): void;
  /** Send to every subscriber of `topic` except this socket. */
  publish(topic: string, message: unknown): number;
}

/** `export const socket: ForgeSocketHandlers = { open, message, close }` in a route file. */
interface ForgeSocketHandlers<P extends Record<string, string> = Record<string, string>> {
  open?(ws: ForgeSocket<P>, ctx: ForgeContext<P>): void | Promise<void>;
  message?(ws: ForgeSocket<P>, message: any, ctx: ForgeContext<P>): void | Promise<void>;
  close?(ws: ForgeSocket<P>, code: number, reason: string): void | Promise<void>;
}

declare module "forge:game" {
  export function loop(options: { update?: (dt: number) => void; render?: (alpha: number) => void; step?: number; maxSteps?: number }): () => void;
  export function input(target?: Window | HTMLElement): {
    pointer: { x: number; y: number; down: boolean };
    down(code: string): boolean;
    pressed(code: string): boolean;
    axis(negative: string, positive: string): -1 | 0 | 1;
    endFrame(): void;
    dispose(): void;
  };
  export function fit(canvas: HTMLCanvasElement, onResize?: (width: number, height: number, dpr: number) => void, options?: { maxDpr?: number }): () => void;
}

declare module "forge:file" {
  export type SaveOptions = {
    /** MIME type; inferred from the data and the file extension otherwise. */
    type?: string;
    /** Let the user pick where to write (File System Access API) instead of downloading. */
    picker?: boolean;
    /** With `picker`: later saves with the same key write to the same file without asking. */
    key?: string;
  };
  /** Ask the user for files; `[]` if they cancel. `accept` as for `<input type="file">`. */
  export function openFiles(options?: { accept?: string; multiple?: boolean }): Promise<File[]>;
  /** Ask the user for one file; `null` if they cancel. */
  export function openFile(options?: { accept?: string }): Promise<File | null>;
  /** Save a Blob, string, bytes or JSON value as `name`. Resolves `false` if the user cancelled. */
  export function saveFile(data: Blob | string | ArrayBuffer | ArrayBufferView | unknown, name: string, options?: SaveOptions): Promise<boolean>;
  /** Files dropped on `target` (default: the window). Returns a cleanup function. */
  export function onDrop(onFiles: (files: File[]) => void, options?: { target?: EventTarget; onOver?: (over: boolean) => void }): () => void;
  /** React: files dropped anywhere (or on `ref.current`); returns whether a drag is hovering. */
  export function useDrop(onFiles: (files: File[]) => void, ref?: { current: EventTarget | null }): boolean;
  /** The state saved into this single-html file by `saveSelf`, or `undefined`. */
  export function embeddedState<T = unknown>(): T | undefined;
  /** The name of the file this page was opened from (`index.html` if unknown). */
  export function fileName(): string;
  /** This single-html app as an HTML document with `state` embedded. */
  export function selfHtml(state?: unknown): string;
  /** Save a copy of this single-html app with `state` embedded; opening it restores the state. */
  export function saveSelf(state: unknown, options?: { name?: string; picker?: boolean }): Promise<boolean>;
  /** React: state saved in this file (or `initial`), unsaved-change tracking, Ctrl/Cmd+S to save. */
  export function useDocument<T>(
    initial: T,
    options?: { name?: string; picker?: boolean; shortcut?: boolean },
  ): [T, (next: T | ((prev: T) => T)) => void, { save(): Promise<boolean>; dirty: boolean }];
}

interface ImportMetaEnv {
  readonly MODE: string;
  readonly DEV: boolean;
  readonly PROD: boolean;
  readonly SSR: boolean;
  /** The build target: "server", "static" or "single-html". */
  readonly TARGET: "server" | "static" | "single-html";
}
interface ImportMeta {
  readonly env: ImportMetaEnv;
}

/** Context passed to `loader()` and API route handlers. */
interface ForgeContext<P extends Record<string, string> = Record<string, string>> {
  params: P;
  url: URL;
  searchParams: URLSearchParams;
  request: Request;
  /** Only variables listed in `server.env` of forge.config.json. */
  env: Record<string, string | undefined>;
  /** Fires when the client disconnects or the request times out. */
  signal: AbortSignal;
}

/** Context passed to vanilla pages' `mount(el, ctx)` / `render(ctx)`. */
interface ForgePageContext<D = unknown, P extends Record<string, string> = Record<string, string>> {
  params: P;
  data: D;
  pathname: string;
  search?: string;
  hydrate?: boolean;
  navigate?: (to: string) => Promise<void>;
}

declare module "*.css";
declare module "*.png" { const url: string; export default url; }
declare module "*.jpg" { const url: string; export default url; }
declare module "*.jpeg" { const url: string; export default url; }
declare module "*.gif" { const url: string; export default url; }
declare module "*.webp" { const url: string; export default url; }
declare module "*.avif" { const url: string; export default url; }
declare module "*.svg" { const url: string; export default url; }
declare module "*.mp3" { const url: string; export default url; }
declare module "*.ogg" { const url: string; export default url; }
declare module "*.wav" { const url: string; export default url; }
declare module "*.glb" { const url: string; export default url; }
declare module "*.gltf" { const url: string; export default url; }
declare module "*.hdr" { const url: string; export default url; }
declare module "*.ktx2" { const url: string; export default url; }
declare module "*.wasm" { const url: string; export default url; }
declare module "*.onnx" { const url: string; export default url; }
declare module "*.woff2" { const url: string; export default url; }
declare module "*.glsl" { const source: string; export default source; }
declare module "*.vert" { const source: string; export default source; }
declare module "*.frag" { const source: string; export default source; }
declare module "*.wgsl" { const source: string; export default source; }
declare module "*.txt" { const text: string; export default text; }
declare module "*.md" { const text: string; export default text; }
declare module "*.prompt" { const text: string; export default text; }
