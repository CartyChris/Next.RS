export const notes = [
  { id: "rust", title: "Why Rust", body: "One binary serves pages, data and APIs." },
  { id: "html", title: "One file", body: "forge build --target single-html makes dist/index.html." },
  { id: "games", title: "Games", body: "Pages can export mount(el) and own a canvas." },
];

export const escape = (s) => String(s).replace(/[&<>"]/g, (c) => ({ "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;" })[c]);

export const nav = `<nav><a href="/">Home</a> · <a href="/about">About</a> · <a href="/notes/rust">Notes</a></nav>`;
