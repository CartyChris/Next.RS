import type { Accent, Density, Radius, ThemeMode, ThemeTokens } from "./primitives";

/** How panels are drawn: opaque, or translucent glass that blurs what is behind it. */
export type Surface = "solid" | "glass";

export interface ThemeSettings {
  theme?: ThemeMode;
  accent?: Accent;
  radius?: Radius;
  density?: Density;
  surface?: Surface;
  tokens?: ThemeTokens;
}

export interface ThemePreset {
  /** Pass to `<Theme preset="…">`. */
  name: string;
  label: string;
  /** What it looks like and what it suits. */
  description: string;
  /** Words people use when asking for this look. */
  keywords: string[];
  /** The `Theme` props this preset stands for. Any prop you pass yourself wins. */
  settings: ThemeSettings;
}

// The catalog is plain JSON between the markers: `forge mcp` and `forge docs` read it too.
export const themes: readonly ThemePreset[] = /* themes:start */ [
  {
    "name": "forge",
    "label": "Forge",
    "description": "The default. Rust-orange accent on warm paper neutrals, medium corners. A confident product look for apps, dashboards and tools.",
    "keywords": ["default", "warm", "rust", "orange", "ember", "product", "saas", "dashboard", "friendly"],
    "settings": { "accent": "ember", "radius": "md", "density": "comfortable", "surface": "solid" }
  },
  {
    "name": "liquid-glass",
    "label": "Liquid glass",
    "description": "Translucent panels that blur and brighten what is behind them, soft highlights on every edge, large corners, a clear blue accent over a quiet tinted backdrop. The look of current Apple platforms.",
    "keywords": ["glass", "liquid glass", "apple", "ios", "macos", "visionos", "frosted", "translucent", "blur", "glassmorphism", "airy", "modern"],
    "settings": { "accent": "cobalt", "radius": "lg", "density": "comfortable", "surface": "glass" }
  },
  {
    "name": "graphite",
    "label": "Graphite",
    "description": "Near-monochrome: a muted steel accent, tight corners and compact spacing. For dense professional tools such as admin panels, IDE-like apps and data tables.",
    "keywords": ["neutral", "gray", "grey", "monochrome", "minimal", "dense", "compact", "developer", "admin", "linear", "serious", "enterprise"],
    "settings": { "accent": "steel", "radius": "sm", "density": "compact", "surface": "solid" }
  },
  {
    "name": "paper",
    "label": "Paper",
    "description": "Editorial: serif headings, generous spacing, square-ish corners and an ink-gold accent. For blogs, documentation, long reads and portfolios.",
    "keywords": ["editorial", "serif", "blog", "docs", "documentation", "reading", "magazine", "newspaper", "classic", "elegant", "portfolio"],
    "settings": {
      "accent": "gold", "radius": "sm", "density": "spacious", "surface": "solid",
      "tokens": { "font-display": "\"Iowan Old Style\", \"Palatino Linotype\", Palatino, Georgia, serif" }
    }
  },
  {
    "name": "clinic",
    "label": "Clinic",
    "description": "Calm and trustworthy: a teal accent, roomy spacing and medium corners on light, clean surfaces. For health, finance, booking and anything that should feel safe.",
    "keywords": ["calm", "clean", "teal", "green", "health", "medical", "finance", "fintech", "bank", "trust", "light", "wellness"],
    "settings": { "accent": "verdigris", "radius": "md", "density": "spacious", "surface": "solid" }
  },
  {
    "name": "midnight",
    "label": "Midnight",
    "description": "Always dark, with a violet accent and large corners. For media, music, analytics at night and creative tools.",
    "keywords": ["dark", "night", "violet", "purple", "media", "music", "video", "creative", "studio", "analytics"],
    "settings": { "theme": "dark", "accent": "violet", "radius": "lg", "density": "comfortable", "surface": "solid" }
  },
  {
    "name": "terminal",
    "label": "Terminal",
    "description": "Always dark, monospaced type everywhere, square corners, compact spacing and a phosphor-green accent. For developer consoles, logs, CLIs in the browser and retro tools.",
    "keywords": ["terminal", "console", "cli", "hacker", "monospace", "mono", "retro", "green", "matrix", "code", "logs", "devtools"],
    "settings": {
      "theme": "dark", "accent": 150, "radius": "none", "density": "compact", "surface": "solid",
      "tokens": {
        "font": "ui-monospace, \"SFMono-Regular\", \"Cascadia Code\", Consolas, monospace",
        "font-display": "ui-monospace, \"SFMono-Regular\", \"Cascadia Code\", Consolas, monospace"
      }
    }
  },
  {
    "name": "arcade",
    "label": "Arcade",
    "description": "Always dark glass with an electric cyan accent and pill-shaped controls. For game menus, HUDs, launchers and playful apps.",
    "keywords": ["game", "gaming", "arcade", "neon", "playful", "fun", "hud", "launcher", "cyan", "electric", "esports"],
    "settings": { "theme": "dark", "accent": 205, "radius": "full", "density": "comfortable", "surface": "glass" }
  }
] /* themes:end */;

/** The preset with this name, if there is one. */
export function getTheme(name: string): ThemePreset | undefined {
  return themes.find((theme) => theme.name === name);
}

/** Presets that match words such as `"frosted apple"` or `"dense admin"`, best first. */
export function findThemes(query: string): ThemePreset[] {
  const words = query.toLowerCase().split(/[^a-z0-9]+/).filter((word) => word.length > 1);
  return themes
    .map((theme) => {
      const haystack = `${theme.name} ${theme.label} ${theme.keywords.join(" ")}`.toLowerCase();
      const text = theme.description.toLowerCase();
      const score = words.reduce((sum, word) => sum + (haystack.includes(word) ? 3 : text.includes(word) ? 1 : 0), 0);
      return { theme, score };
    })
    .filter((entry) => entry.score > 0)
    .sort((a, b) => b.score - a.score)
    .map((entry) => entry.theme);
}
