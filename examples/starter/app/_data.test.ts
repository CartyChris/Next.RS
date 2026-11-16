// Run with `forge test`. Tests use the same engine as loaders and API routes.
import { expect, test } from "forge:test";
import { projects } from "./_data";
import { loader } from "./loader";

test("every project has a unique id", () => {
  expect(new Set(projects.map((project) => project.id)).size).toBe(projects.length);
});

test("the loader returns the projects", () => {
  expect(loader().projects).toHaveLength(4);
  expect(loader().projects[0]).toHaveProperty("name", "Atlas");
});
