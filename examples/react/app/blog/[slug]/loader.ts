import { notFound } from "forge:navigation";
import { posts } from "../../_lib/posts";

export function loader({ params }: ForgeContext<{ slug: string }>) {
  return posts.find((p) => p.slug === params.slug) ?? notFound();
}

export const staticParams = () => posts.map((p) => ({ slug: p.slug }));
