// forge:realtime (server): publish to WebSocket topics from any server code
// (API routes, loaders, socket handlers). Fan-out happens in Rust, across workers.
import { host } from "./host-ref.js";

const encode = (m) => (typeof m === "string" || m instanceof Uint8Array ? m : JSON.stringify(m));

/** Send `message` to every socket subscribed to `topic`. Returns how many received it. */
export const publish = (topic, message) => host.publish(String(topic), encode(message), null);

/** How many sockets are subscribed to `topic` right now (presence counts). */
export const subscribers = (topic) => host.subscribers(String(topic));

export function connect() {
  throw new Error("forge:realtime connect() runs in the browser; on the server use publish()");
}

// Server render of a component using the hook: no connection, nothing to send.
export const useSocket = () => ({ status: "connecting", send: () => false });
