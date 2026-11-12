// forge:realtime (browser): a reconnecting WebSocket that speaks JSON.
import { rt } from "./shared.js";

const toWs = (path) => {
  const u = new URL(path, location.href);
  u.protocol = u.protocol === "https:" ? "wss:" : "ws:";
  return u.href;
};

/**
 * Connect to a socket route. Messages are JSON-encoded/decoded (strings and binary pass
 * through). Reconnects with jittered backoff; sends made while disconnected are queued.
 * Status: "connecting" | "open" | "reconnecting" | "closed" | "offline" (file:// builds).
 */
export function connect(path, { onMessage, onOpen, onClose, onStatus, reconnect = true } = {}) {
  let ws;
  let status;
  let attempts = 0;
  let timer;
  let stopped = false;
  const queue = [];
  const set = (s) => {
    status = s;
    onStatus?.(s);
  };
  const handle = {
    send(msg) {
      const data = typeof msg === "string" || msg instanceof ArrayBuffer || ArrayBuffer.isView(msg) ? msg : JSON.stringify(msg);
      if (ws?.readyState === 1) ws.send(data);
      else if (queue.length < 256 && !stopped) queue.push(data);
      else return false;
      return true;
    },
    close(code = 1000, reason = "") {
      stopped = true;
      clearTimeout(timer);
      ws?.close(code, reason);
      set("closed");
    },
    get status() {
      return status;
    },
  };
  if (typeof WebSocket === "undefined" || location.protocol === "file:") {
    set("offline");
    return handle;
  }
  const open = () => {
    set(attempts ? "reconnecting" : "connecting");
    ws = new WebSocket(toWs(path));
    ws.binaryType = "arraybuffer";
    ws.onopen = () => {
      attempts = 0;
      set("open");
      for (const m of queue.splice(0)) ws.send(m);
      onOpen?.(handle);
    };
    ws.onmessage = (e) => {
      let d = e.data;
      if (typeof d === "string") {
        try {
          d = JSON.parse(d);
        } catch {}
      } else d = new Uint8Array(d);
      onMessage?.(d, handle);
    };
    ws.onclose = (e) => {
      onClose?.(e);
      // 1008 = policy (e.g. too slow / rejected): do not hammer the server.
      if (stopped || !reconnect || e.code === 1008) return set("closed");
      set("reconnecting");
      timer = setTimeout(open, Math.min(5000, 250 * 2 ** attempts++) * (0.5 + Math.random()));
    };
  };
  open();
  return handle;
}

/** React hook: `const { status, send } = useSocket("/api/room/1", (msg) => …)`. */
export function useSocket(path, onMessage, options) {
  const { useEffect, useRef, useState, useCallback } = rt.React;
  const [status, setStatus] = useState("connecting");
  const handler = useRef(onMessage);
  handler.current = onMessage;
  const socket = useRef(null);
  useEffect(() => {
    if (!path) return;
    const s = connect(path, { ...options, onMessage: (m, h) => handler.current?.(m, h), onStatus: setStatus });
    socket.current = s;
    return () => s.close();
  }, [path]);
  const send = useCallback((m) => socket.current?.send(m) ?? false, []);
  return { status, send };
}

export function publish() {
  throw new Error("forge:realtime publish() is server-only; send through a socket instead");
}
export const subscribers = () => 0;
