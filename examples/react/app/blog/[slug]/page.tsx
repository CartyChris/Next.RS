import { Link } from "forge:navigation";
import { useData } from "forge:data";
import type { Post } from "../../_lib/posts";

export const meta = ({ data }: { data: Post }) => ({ title: `${data.title} · Blog`, description: data.body });

export default function PostPage() {
  const post = useData<Post>();
  return (
    <main>
      <p>
        <Link to="/blog">← Blog</Link>
      </p>
      <h1>{post.title}</h1>
      <p>{post.body}</p>
    </main>
  );
}
