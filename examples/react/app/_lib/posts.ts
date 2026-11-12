export type Post = { slug: string; title: string; body: string };

export const posts: Post[] = [
  { slug: "hello", title: "Hello, Forge", body: "Pages are React components; data comes from loader.ts." },
  { slug: "streaming", title: "Streaming SSR", body: "Suspense boundaries stream as they resolve." },
  { slug: "single-file", title: "One HTML file", body: "forge build --target single-html inlines everything." },
];
