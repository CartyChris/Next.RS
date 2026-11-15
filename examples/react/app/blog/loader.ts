import { posts } from "../_lib/posts";

export function loader() {
  return posts.map(({ slug, title }) => ({ slug, title }));
}
