//! `forge run` and `forge test`: scripts and tests in the embedded engine.

mod common;

use common::project;
use nextrs::{
    build::script,
    config::{Mode, Project, Target},
};

#[tokio::test(flavor = "multi_thread")]
async fn runs_typescript_scripts_and_tests() {
    let root = project(
        "script",
        &[
            (
                "lib/cart.ts",
                "export interface Item { price: number; qty: number }\nexport const total = (items: Item[]): number => items.reduce((sum, i) => sum + i.price * i.qty, 0);\nexport async function slow(): Promise<string> { await new Promise((r) => setTimeout(r, 20)); return 'done'; }\n",
            ),
            (
                "lib/cart.test.ts",
                r#"import { describe, test, expect, beforeEach } from "forge:test";
import { total, slow } from "./cart";

let calls = 0;
beforeEach(() => { calls += 1; });

describe("cart", () => {
  test("adds line totals", () => {
    expect(total([{ price: 2, qty: 3 }, { price: 5, qty: 1 }])).toBe(11);
    expect(total([])).toBe(0);
    expect({ a: [1, { b: 2 }] }).toEqual({ a: [1, { b: 2 }] });
    expect([1, 2, 3]).toContain(2);
    expect(() => { throw new Error("boom"); }).toThrow("boom");
    expect(0.1 + 0.2).toBeCloseTo(0.3);
    expect("forge").not.toMatch(/x/);
  });
  test("awaits async work", async () => {
    expect(await slow()).toBe("done");
    expect(calls).toBe(2);
  });
  test("reports a failure", () => {
    expect(total([{ price: 1, qty: 1 }])).toBe(2);
  });
  test.skip("not yet");
});
"#,
            ),
            (
                "lib/format.spec.js",
                "import { test, expect } from 'forge:test';\ntest('formats like the browser', () => { expect((1234.5).toLocaleString('en-US')).toBe('1,234.5'); });\n",
            ),
            (
                "lib/runner.test.ts",
                r#"import { afterEach, beforeEach, describe, expect, test } from "forge:test";

let open = 0;
const cleaned: string[] = [];

describe("teardown", () => {
  beforeEach(() => { open += 1; });
  afterEach(() => { open -= 1; });
  afterEach(() => { if (cleaned.length === 1) throw new Error("teardown broke"); });
  afterEach(() => { cleaned.push("last hook"); });

  test("fails with a resource open", () => { expect(open).toBe(99); });
  test("starts clean after a failure", () => { expect(open).toBe(1); expect(cleaned).toHaveLength(1); });
  test("still ran the last hook", () => { expect(cleaned).toHaveLength(2); });
});

test("hooks stay inside their group", () => { expect(open).toBe(0); expect(cleaned).toHaveLength(3); });

test("built-in objects compare by content", () => {
  expect(new Date(0)).not.toEqual({});
  expect({}).not.toEqual(new Date(0));
  expect(new Map([["id", 1]])).not.toEqual({});
  expect(/yes/).not.toEqual(/no/);
  expect([1]).not.toEqual({ 0: 1 });
  expect(new Uint8Array([1, 2])).not.toEqual(new Uint8Array([1, 3]));
  expect(new Date(5)).not.toEqual(new Date(6));
  expect(new Date(5)).toEqual(new Date(5));
  expect(/a/g).toEqual(/a/g);
  expect(new Set([1, { x: 2 }])).toEqual(new Set([{ x: 2 }, 1]));
  expect(new Set([1, 2])).not.toEqual(new Set([1, 3]));
  expect(new Map([["a", { x: 1 }]])).toEqual(new Map([["a", { x: 1 }]]));
  expect(new Map([["a", 1]])).not.toEqual(new Map([["a", 2]]));
  expect(new Uint8Array([1, 2])).toEqual(new Uint8Array([1, 2]));
  expect(new Error("x")).not.toEqual(new Error("y"));
});
"#,
            ),
            ("lib/broken.test.ts", "import { missing } from './nowhere-at-runtime';\nmissing();\n"),
            (
                "scripts/report.ts",
                "import { total } from '../lib/cart';\nexport default async function main() { return { total: total([{ price: 4, qty: 2 }]), env: typeof process }; }\n",
            ),
            ("scripts/text.js", "export default () => 'plain text';\n"),
            ("scripts/fails.js", "export default () => { throw new Error('nope'); };\n"),
            ("scripts/evil.js", "export default () => eval('1');\n"),
        ],
    );
    let project = Project::load(&root, Target::Server, Mode::Prod).unwrap();

    // Scripts: a default export is called; objects print as JSON, strings as they are.
    let report = script::run(&project, &root.join("scripts/report.ts")).await.unwrap().unwrap();
    assert_eq!(serde_json::from_str::<serde_json::Value>(&report).unwrap()["total"], 8, "{report}");
    assert_eq!(script::run(&project, &root.join("scripts/text.js")).await.unwrap().as_deref(), Some("plain text"));
    let err = script::run(&project, &root.join("scripts/fails.js")).await.unwrap_err();
    assert!(format!("{err:#}").contains("nope"), "{err:#}");
    // The security scan applies to scripts too.
    let err = script::run(&project, &root.join("scripts/evil.js")).await.unwrap_err();
    assert!(format!("{err:#}").contains("`eval` is denied"), "{err:#}");

    // Tests: found by name, filtered by path, run in order with hooks; failures carry a message.
    let all = script::find_tests(&root, None);
    let names: Vec<String> =
        all.iter().map(|p| p.strip_prefix(&root).unwrap().to_string_lossy().into_owned()).collect();
    assert_eq!(names, ["lib/broken.test.ts", "lib/cart.test.ts", "lib/format.spec.js", "lib/runner.test.ts"]);
    let cart = script::find_tests(&root, Some("cart"));
    assert_eq!(cart.len(), 1);
    let results = script::test(&project, &[cart[0].clone(), root.join("lib/format.spec.js")]).await.unwrap();
    let summary: Vec<(&str, bool, bool)> = results.iter().map(|r| (r.name.as_str(), r.ok, r.skipped)).collect();
    assert_eq!(
        summary,
        [
            ("cart › adds line totals", true, false),
            ("cart › awaits async work", true, false),
            ("cart › reports a failure", false, false),
            ("cart › not yet", false, true),
            ("formats like the browser", true, false),
        ]
    );
    assert_eq!(results[2].error, "AssertionError: expected 1 to be 2");
    assert_eq!(results[4].file, "lib/format.spec.js");
    // Teardown runs after a failing test, every hook runs even if one throws (and that
    // failure is reported), hooks stay in their group, and built-ins compare by content.
    let runner = script::test(&project, &[root.join("lib/runner.test.ts")]).await.unwrap();
    let summary: Vec<(&str, bool)> = runner.iter().map(|r| (r.name.as_str(), r.ok)).collect();
    assert_eq!(
        summary,
        [
            ("teardown › fails with a resource open", false),
            ("teardown › starts clean after a failure", false),
            ("teardown › still ran the last hook", true),
            ("hooks stay inside their group", true),
            ("built-in objects compare by content", true),
        ],
        "{runner:?}"
    );
    assert_eq!(runner[0].error, "AssertionError: expected 1 to be 99");
    assert_eq!(runner[1].error, "Error: teardown broke");
    // A file that cannot be bundled fails the run with the reason.
    let err = script::test(&project, &all).await.unwrap_err();
    assert!(format!("{err:#}").contains("nowhere-at-runtime"), "{err:#}");
    std::fs::remove_dir_all(root).ok();
}
