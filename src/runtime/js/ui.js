// forge:ui: the Forge UI component library, built into the binary.
// Importing it brings its stylesheet along and connects links to the Forge router.
import "./ui/styles.css";
import { createElement } from "react";
import { rt } from "./shared.js";
import { usePathname } from "./navigation.js";
import { configure } from "./ui/config.ts";

configure({
  // rt.Link exists once the React adapter has loaded, which is before any component renders.
  Link: (props) => createElement(rt.Link, props),
  href: (to) => rt.href(to),
  usePathname,
  navigate: (to) => (rt.navigate ? rt.navigate(to) : location.assign(rt.href(to))),
});

export * from "./ui/index.ts";
