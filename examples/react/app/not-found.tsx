import { Link } from "forge:navigation";

export const meta = { title: "Not found" };

export default function NotFound() {
  return (
    <main>
      <h1>404</h1>
      <p>
        Nothing here. <Link to="/">Go home</Link>
      </p>
    </main>
  );
}
