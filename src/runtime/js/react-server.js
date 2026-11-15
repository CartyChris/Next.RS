import { createElement } from "react";
import { renderToReadableStream } from "react-dom/server.browser";
import { App } from "./react.js";

// Resolves once the shell is ready (or rejects if it cannot render). With
// `stream: false` it waits for every Suspense boundary so the HTML is complete.
export async function render(state, { nonce, stream, signal }) {
  const store = { get: () => state, subscribe: () => () => {} };
  const body = await renderToReadableStream(createElement(App, { store }), {
    nonce,
    signal,
    onError: (e) => signal?.aborted || console.error(e),
  });
  if (!stream) await body.allReady;
  return body;
}
