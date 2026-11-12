import { Link } from "forge:navigation";
import { useData } from "forge:data";

export const meta = { title: "Blog · Forge" };

export default function Blog() {
  const posts = useData<{ slug: string; title: string }[]>();
  return (
    <main>
      <h1>Blog</h1>
      <ul>
        {posts.map((p) => (
          <li key={p.slug}>
            <Link to={`/blog/${p.slug}`}>{p.title}</Link>
          </li>
        ))}
      </ul>
    </main>
  );
}
