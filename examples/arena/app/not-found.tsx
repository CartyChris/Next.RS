import { Link } from "forge:navigation";

export const meta = { title: "Lost in the grid · Forge Arena" };

export default function NotFound() {
  return (
    <main className="lobby">
      <section className="hero">
        <h1>
          404
          <br />
          <span>Lost in the grid.</span>
        </h1>
        <p className="lede">
          <Link to="/">Back to the lobby</Link>
        </p>
      </section>
    </main>
  );
}
