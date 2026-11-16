import { useEffect, useRef, useState } from "react";
import type { CSSProperties, ComponentPropsWithRef, ReactNode } from "react";
import { cx, getAdapter, isExternal, pathOf } from "./config";
import { Icon } from "./icon";
import { getTheme } from "./themes";
import type { Surface } from "./themes";

export type ThemeMode = "system" | "light" | "dark" | "inherit";
export type Spacing = "xs" | "sm" | "md" | "lg" | "xl";
export type Tone = "neutral" | "info" | "success" | "warning" | "danger";

/** Built-in accent palettes; `ember` is the default. Any CSS color or an OKLCH hue (0–360) also works. */
export type AccentPreset = "ember" | "violet" | "cobalt" | "verdigris" | "gold" | "rose" | "steel";
export type Accent = AccentPreset | (string & {}) | number;
export type Radius = "none" | "sm" | "md" | "lg" | "full";
export type Density = "compact" | "comfortable" | "spacious";

/** Design tokens that can be overridden per `Theme` (each maps to `--forge-ui-<name>`). */
export type ThemeToken =
  | "font" | "font-mono" | "font-display"
  | "bg" | "surface" | "surface-raised" | "subtle" | "edge" | "control-edge" | "text" | "muted"
  | "focus" | "accent" | "accent-hover" | "on-accent" | "accent-soft" | "accent-text"
  | "radius" | "radius-panel" | "shadow" | "shadow-raised" | "control-size";
export type ThemeTokens = Partial<Record<ThemeToken, string>>;

const ACCENTS: readonly string[] = ["ember", "violet", "cobalt", "verdigris", "gold", "rose", "steel"];

export interface ThemeProps extends ComponentPropsWithRef<"div"> {
  /**
   * A named look from the catalog (`themes`): `"forge"`, `"liquid-glass"`, `"graphite"`,
   * `"paper"`, `"clinic"`, `"midnight"`, `"terminal"`, `"arcade"`. Other props override it.
   */
  preset?: string;
  /** `glass` draws panels translucent, blurring what is behind them. */
  surface?: Surface;
  /** Color scheme. `inherit` keeps the surrounding theme's scheme (for nested themes). */
  theme?: ThemeMode;
  /** A preset name, any CSS color (an accessible palette is derived from its hue), or a hue. */
  accent?: Accent;
  radius?: Radius;
  density?: Density;
  /** Override individual tokens, e.g. `{ font: "Inter, sans-serif", surface: "#fff" }`. */
  tokens?: ThemeTokens;
}

export function Theme({ preset, className = "", style, ...rest }: ThemeProps) {
  const base = preset ? getTheme(preset)?.settings : undefined;
  const {
    theme = base?.theme ?? "system", accent = base?.accent, radius = base?.radius, density = base?.density,
    surface = base?.surface, tokens: ownTokens, ...props
  } = rest;
  const tokens = base?.tokens || ownTokens ? { ...base?.tokens, ...ownTokens } : undefined;
  const vars: Record<string, string | number> = {};
  let accentKind: string | undefined;
  if (typeof accent === "number") {
    accentKind = "hue";
    vars["--forge-ui-accent-h"] = accent;
  } else if (accent && ACCENTS.includes(accent)) {
    accentKind = accent;
  } else if (accent) {
    accentKind = "from";
    vars["--forge-ui-accent-source"] = accent;
  }
  for (const [name, value] of Object.entries(tokens ?? {})) {
    if (value != null) vars[`--forge-ui-${name}`] = value;
  }
  const merged = Object.keys(vars).length ? { ...(vars as CSSProperties), ...style } : style;
  return (
    <div {...props} className={`forge-ui ${className}`} style={merged} data-theme={theme} data-preset={preset}
      data-accent={accentKind} data-radius={radius} data-density={density}
      data-surface={surface === "glass" ? "glass" : undefined} />
  );
}

export interface ContainerProps extends ComponentPropsWithRef<"div"> {
  size?: "narrow" | "default" | "wide" | "full";
}

export function Container({ size = "default", className = "", ...props }: ContainerProps) {
  return <div {...props} className={`forge-ui-container ${className}`} data-size={size} />;
}

export interface StackProps extends ComponentPropsWithRef<"div"> {
  gap?: Spacing;
}

export function Stack({ gap = "md", className = "", ...props }: StackProps) {
  return <div {...props} className={`forge-ui-stack ${className}`} data-gap={gap} />;
}

export interface RowProps extends ComponentPropsWithRef<"div"> {
  gap?: Spacing;
  align?: "start" | "center" | "end" | "baseline" | "stretch";
  justify?: "start" | "center" | "end" | "between";
  /** Wrap onto more lines when space runs out (default). */
  wrap?: boolean;
}

/** A horizontal group: toolbar, title + actions, inline metadata. */
export function Row({ gap = "sm", align = "center", justify = "start", wrap = true, className = "", ...props }: RowProps) {
  return <div {...props} className={`forge-ui-row ${className}`} data-gap={gap} data-align={align}
    data-justify={justify} data-nowrap={wrap ? undefined : true} />;
}

export interface GridProps extends StackProps {
  columns?: 1 | 2 | 3 | 4;
}

export function Grid({ columns = 3, gap = "md", className = "", ...props }: GridProps) {
  return <div {...props} className={`forge-ui-grid ${className}`} data-columns={columns} data-gap={gap} />;
}

export interface DividerProps extends ComponentPropsWithRef<"hr"> {
  orientation?: "horizontal" | "vertical";
}

export function Divider({ orientation = "horizontal", className = "", ...props }: DividerProps) {
  return <hr {...props} className={`forge-ui-divider ${className}`} data-orientation={orientation}
    aria-orientation={orientation === "vertical" ? "vertical" : undefined} />;
}

export interface HeadingProps extends ComponentPropsWithRef<"h2"> {
  /** The semantic level (`h1`–`h4`). Keep levels in document order. */
  level?: 1 | 2 | 3 | 4;
  /** Visual size, independent of the level. */
  size?: "sm" | "md" | "lg" | "xl" | "display";
}

export function Heading({ level = 2, size, className = "", ...props }: HeadingProps) {
  const Tag = `h${level}` as "h1" | "h2" | "h3" | "h4";
  const fallback = (["xl", "lg", "md", "sm"] as const)[level - 1];
  return <Tag {...props} className={`forge-ui-heading ${className}`} data-size={size ?? fallback} />;
}

export interface TextProps extends ComponentPropsWithRef<"p"> {
  as?: "p" | "span" | "div" | "small" | "strong";
  tone?: "default" | "muted" | "accent" | Exclude<Tone, "neutral">;
  size?: "xs" | "sm" | "md" | "lg";
  /** Clip to one line with an ellipsis. */
  truncate?: boolean;
}

export function Text({ as: Tag = "p", tone = "default", size = "md", truncate, className = "", ...props }: TextProps) {
  return <Tag {...(props as ComponentPropsWithRef<"p">)} className={`forge-ui-text ${className}`} data-tone={tone}
    data-size={size} data-truncate={truncate || undefined} />;
}

export function Kbd({ className = "", ...props }: ComponentPropsWithRef<"kbd">) {
  return <kbd {...props} className={`forge-ui-kbd ${className}`} />;
}

export function Code({ className = "", ...props }: ComponentPropsWithRef<"code">) {
  return <code {...props} className={`forge-ui-code ${className}`} />;
}

export type ButtonVariant = "primary" | "secondary" | "soft" | "ghost" | "danger" | "link";
export type ButtonSize = "sm" | "md" | "lg";

export interface ButtonAppearance {
  variant?: ButtonVariant;
  size?: ButtonSize;
  fullWidth?: boolean;
  /** Content before the label, usually an `Icon`. */
  iconStart?: ReactNode;
  /** Content after the label. */
  iconEnd?: ReactNode;
}

export interface ButtonProps extends ButtonAppearance, ComponentPropsWithRef<"button"> {
  /** Shows a spinner, sets `aria-busy`, and ignores clicks until it is false again. */
  loading?: boolean;
}
export interface ButtonLinkProps extends ButtonAppearance, ComponentPropsWithRef<"a"> {
  href: string;
}

export function Button({
  variant = "primary", size = "md", fullWidth, iconStart, iconEnd, loading, className = "", type = "button",
  children, onClick, ...props
}: ButtonProps) {
  return (
    <button {...props} type={type} className={`forge-ui-button forge-ui-button--${variant} ${className}`}
      data-size={size} data-full-width={fullWidth || undefined} aria-busy={loading || undefined}
      aria-disabled={loading || props["aria-disabled"]} onClick={loading ? (event) => event.preventDefault() : onClick}>
      {loading ? <Spinner size="sm" label="" /> : iconStart}
      {children}
      {iconEnd}
    </button>
  );
}

/** An anchor for in-app paths (routed through the app's link component) and external URLs. */
export function AppLink({ href, ...props }: ComponentPropsWithRef<"a"> & { href: string }) {
  const { Link, href: toHref } = getAdapter();
  if (isExternal(href)) return <a {...props} href={href} />;
  if (Link) return <Link {...props} href={href} />;
  return <a {...props} href={toHref ? toHref(href) : href} />;
}

export function ButtonLink({
  variant = "primary", size = "md", fullWidth, iconStart, iconEnd, className = "", children, ...props
}: ButtonLinkProps) {
  return (
    <AppLink {...props} className={`forge-ui-button forge-ui-button--${variant} ${className}`}
      data-size={size} data-full-width={fullWidth || undefined}>
      {iconStart}
      {children}
      {iconEnd}
    </AppLink>
  );
}

export interface IconButtonProps extends Omit<ButtonProps, "children" | "iconStart" | "iconEnd" | "fullWidth"> {
  /** The accessible name; also shown as a native tooltip. */
  label: string;
  children: ReactNode;
}

/** A square button whose only content is an icon. */
export function IconButton({ label, variant = "ghost", className = "", children, ...props }: IconButtonProps) {
  return (
    <Button {...props} variant={variant} aria-label={label} title={props.title ?? label}
      className={`forge-ui-icon-button ${className}`}>
      {children}
    </Button>
  );
}

export interface ButtonGroupProps extends ComponentPropsWithRef<"div"> {
  /** Names the group for assistive technology. */
  label: string;
}

/** Joins adjacent buttons into one segmented control. */
export function ButtonGroup({ label, className = "", ...props }: ButtonGroupProps) {
  return <div {...props} role="group" aria-label={label} className={`forge-ui-button-group ${className}`} />;
}

export interface CardProps extends ComponentPropsWithRef<"div"> {
  /** `spotlight` adds the accent edge; `ghost` drops the surface; `raised` lifts it. */
  variant?: "outline" | "raised" | "ghost" | "spotlight";
  /** Adds hover and focus-within feedback, for cards that contain one main link. */
  interactive?: boolean;
}

export function Card({ variant = "outline", interactive, className = "", ...props }: CardProps) {
  return <div {...props} className={`forge-ui-card ${className}`} data-variant={variant}
    data-interactive={interactive || undefined} />;
}

export function CardHeader({ className = "", ...props }: ComponentPropsWithRef<"div">) {
  return <div {...props} className={`forge-ui-card-header ${className}`} />;
}

export interface CardTitleProps extends ComponentPropsWithRef<"h2"> {
  as?: "h2" | "h3" | "h4";
}

export function CardTitle({ as: Heading = "h2", className = "", ...props }: CardTitleProps) {
  return <Heading {...props} className={`forge-ui-card-title ${className}`} />;
}

export function CardDescription({ className = "", ...props }: ComponentPropsWithRef<"p">) {
  return <p {...props} className={`forge-ui-card-description ${className}`} />;
}

export function CardContent({ className = "", ...props }: ComponentPropsWithRef<"div">) {
  return <div {...props} className={`forge-ui-card-content ${className}`} />;
}

export function CardFooter({ className = "", ...props }: ComponentPropsWithRef<"div">) {
  return <div {...props} className={`forge-ui-card-footer ${className}`} />;
}

export interface BadgeProps extends ComponentPropsWithRef<"span"> {
  tone?: Tone | "accent";
  /** A leading status dot. */
  dot?: boolean;
}

export function Badge({ tone = "neutral", dot, className = "", children, ...props }: BadgeProps) {
  return (
    <span {...props} className={`forge-ui-badge ${className}`} data-tone={tone}>
      {dot && <span className="forge-ui-badge-dot" aria-hidden="true" />}
      {children}
    </span>
  );
}

export interface AlertProps extends Omit<ComponentPropsWithRef<"div">, "title"> {
  title?: ReactNode;
  tone?: Tone;
  live?: "off" | "polite" | "assertive";
  /** Buttons or links shown under the message. */
  actions?: ReactNode;
}

export function Alert({ tone = "info", live = "off", title, actions, children, className = "", ...props }: AlertProps) {
  const role = live === "assertive" ? "alert" : live === "polite" ? "status" : undefined;
  return (
    <div role={role} {...props} className={`forge-ui-alert ${className}`} data-tone={tone}>
      {title != null && <div className="forge-ui-alert-title">{title}</div>}
      {children}
      {actions != null && <div className="forge-ui-alert-actions">{actions}</div>}
    </div>
  );
}

export interface NavProps extends ComponentPropsWithRef<"nav"> {
  label: string;
  orientation?: "horizontal" | "vertical";
}

export function Nav({ label, orientation = "horizontal", className = "", ...props }: NavProps) {
  return <nav {...props} aria-label={label} className={`forge-ui-nav ${className}`} data-orientation={orientation} />;
}

export interface NavLinkProps extends ComponentPropsWithRef<"a"> {
  href: string;
  /** Marks the current page. Detected from the router when omitted. */
  current?: boolean | "page" | "location";
  /** With automatic detection, also match pages below this path. */
  matchPrefix?: boolean;
}

function useCurrent(href: string, current: NavLinkProps["current"], prefix?: boolean) {
  const pathname = getAdapter().usePathname?.();
  if (current !== undefined) return current === true ? "page" : current || undefined;
  if (pathname === undefined || isExternal(href) && !href.startsWith("#/")) return undefined;
  const target = pathOf(href);
  const here = pathOf(pathname);
  if (here === target) return "page";
  return prefix && target !== "/" && here.startsWith(`${target}/`) ? "page" : undefined;
}

export function NavLink({ current, matchPrefix, className = "", ...props }: NavLinkProps) {
  const ariaCurrent = useCurrent(props.href, current, matchPrefix) ?? props["aria-current"];
  return <AppLink {...props} aria-current={ariaCurrent} className={`forge-ui-nav-link ${className}`} />;
}

export interface SpinnerProps extends ComponentPropsWithRef<"span"> {
  size?: "sm" | "md" | "lg";
  /** Announced to assistive technology. Pass `""` when a parent already says what is loading. */
  label?: string;
}

export function Spinner({ size = "md", label = "Loading", className = "", ...props }: SpinnerProps) {
  return (
    <span {...props} className={`forge-ui-spinner ${className}`} data-size={size}
      role={label ? "status" : undefined} aria-hidden={label ? undefined : true}>
      {label && <span className="forge-ui-visually-hidden">{label}</span>}
    </span>
  );
}

export interface SkeletonProps extends ComponentPropsWithRef<"span"> {
  width?: string;
  height?: string;
  /** Render this many text lines instead of one block. */
  lines?: number;
  circle?: boolean;
}

/** A placeholder with the size of the content that is loading. Hidden from assistive technology. */
export function Skeleton({ width, height, lines, circle, className = "", style, ...props }: SkeletonProps) {
  if (lines && lines > 1) {
    return (
      <span {...props} className={`forge-ui-skeleton-lines ${className}`} aria-hidden="true" style={style}>
        {Array.from({ length: lines }, (_, index) => (
          <span key={index} className="forge-ui-skeleton" style={index === lines - 1 ? { width: "62%" } : undefined} />
        ))}
      </span>
    );
  }
  return <span {...props} className={`forge-ui-skeleton ${className}`} aria-hidden="true"
    data-circle={circle || undefined} style={{ width, height, ...style }} />;
}

export interface AvatarProps extends Omit<ComponentPropsWithRef<"span">, "children"> {
  /** The person or thing shown. Used for the accessible name and the initials. */
  name: string;
  src?: string;
  size?: "sm" | "md" | "lg";
}

export function Avatar({ name, src, size = "md", className = "", ...props }: AvatarProps) {
  const initials = name.split(/\s+/).filter(Boolean).slice(0, 2).map((part) => part[0]?.toUpperCase()).join("");
  return (
    <span {...props} className={`forge-ui-avatar ${className}`} data-size={size} role="img" aria-label={name}>
      {src ? <img src={src} alt="" loading="lazy" decoding="async" /> : <span aria-hidden="true">{initials}</span>}
    </span>
  );
}

export interface ProgressProps extends Omit<ComponentPropsWithRef<"div">, "children"> {
  /** What is progressing. */
  label: string;
  /** Omit for an indeterminate bar. */
  value?: number;
  max?: number;
  tone?: Tone | "accent";
  /** Show the label and percentage above the bar. */
  showValue?: boolean;
}

export function Progress({ label, value, max = 100, tone = "accent", showValue, className = "", ...props }: ProgressProps) {
  const ratio = value === undefined ? undefined : Math.min(1, Math.max(0, value / max));
  return (
    <div {...props} className={`forge-ui-progress ${className}`} data-tone={tone}>
      {showValue && (
        <div className="forge-ui-progress-head" aria-hidden="true">
          <span>{label}</span>
          {ratio !== undefined && <span>{Math.round(ratio * 100)}%</span>}
        </div>
      )}
      <div className="forge-ui-progress-track" role="progressbar" aria-label={label} aria-valuemin={0}
        aria-valuemax={max} aria-valuenow={value} data-indeterminate={ratio === undefined || undefined}>
        <div className="forge-ui-progress-fill" style={ratio === undefined ? undefined : { inlineSize: `${ratio * 100}%` }} />
      </div>
    </div>
  );
}

export interface StatProps extends Omit<ComponentPropsWithRef<"div">, "children"> {
  label: ReactNode;
  value: ReactNode;
  /** A change such as `"+12%"`; colored by `trend`. */
  change?: ReactNode;
  trend?: "up" | "down" | "flat";
  hint?: ReactNode;
}

/** One headline number with its label and change. */
export function Stat({ label, value, change, trend = "flat", hint, className = "", ...props }: StatProps) {
  return (
    <div {...props} className={`forge-ui-stat ${className}`}>
      <div className="forge-ui-stat-label">{label}</div>
      <div className="forge-ui-stat-value">{value}</div>
      {(change != null || hint != null) && (
        <div className="forge-ui-stat-foot">
          {change != null && (
            <span className="forge-ui-stat-change" data-trend={trend}>
              {trend !== "flat" && <Icon name={trend === "up" ? "arrow-up" : "arrow-down"} size={12} />}
              {change}
            </span>
          )}
          {hint != null && <span className="forge-ui-stat-hint">{hint}</span>}
        </div>
      )}
    </div>
  );
}

export interface EmptyStateProps extends Omit<ComponentPropsWithRef<"div">, "title"> {
  title: ReactNode;
  description?: ReactNode;
  icon?: ReactNode;
  /** The next step: usually one primary button. */
  action?: ReactNode;
}

export function EmptyState({ title, description, icon, action, className = "", children, ...props }: EmptyStateProps) {
  return (
    <div {...props} className={`forge-ui-empty ${className}`}>
      {icon != null && <div className="forge-ui-empty-icon" aria-hidden="true">{icon}</div>}
      <div className="forge-ui-empty-title">{title}</div>
      {description != null && <div className="forge-ui-empty-description">{description}</div>}
      {children}
      {action != null && <div className="forge-ui-empty-action">{action}</div>}
    </div>
  );
}

export interface Crumb {
  label: ReactNode;
  href?: string;
}

export interface BreadcrumbsProps extends Omit<ComponentPropsWithRef<"nav">, "children"> {
  /** From the top level to the current page. The last item is the current page. */
  items: readonly Crumb[];
  label?: string;
}

export function Breadcrumbs({ items, label = "Breadcrumb", className = "", ...props }: BreadcrumbsProps) {
  return (
    <nav {...props} aria-label={label} className={`forge-ui-breadcrumbs ${className}`}>
      <ol>
        {items.map((item, index) => {
          const last = index === items.length - 1;
          return (
            <li key={index}>
              {item.href && !last
                ? <AppLink href={item.href}>{item.label}</AppLink>
                : <span aria-current={last ? "page" : undefined}>{item.label}</span>}
            </li>
          );
        })}
      </ol>
    </nav>
  );
}

export interface PaginationProps extends Omit<ComponentPropsWithRef<"nav">, "children"> {
  /** The current page, starting at 1. */
  page: number;
  pageCount: number;
  /** Called with the chosen page. Use this or `hrefFor`. */
  onPageChange?: (page: number) => void;
  /** Render links instead of buttons, e.g. `(page) => "/posts?page=" + page`. */
  hrefFor?: (page: number) => string;
  /** Pages shown on each side of the current one. */
  siblings?: number;
  label?: string;
}

function pageList(page: number, count: number, siblings: number): Array<number | "gap"> {
  const pages = new Set<number>([1, count]);
  for (let p = page - siblings; p <= page + siblings; p++) if (p >= 1 && p <= count) pages.add(p);
  const sorted = [...pages].sort((a, b) => a - b);
  const out: Array<number | "gap"> = [];
  sorted.forEach((p, index) => {
    if (index > 0 && p - sorted[index - 1] > 1) out.push(p - sorted[index - 1] === 2 ? p - 1 : "gap");
    out.push(p);
  });
  return out;
}

export function Pagination({
  page, pageCount, onPageChange, hrefFor, siblings = 1, label = "Pagination", className = "", ...props
}: PaginationProps) {
  const item = (target: number, content: ReactNode, name: string, current = false) => {
    const disabled = target < 1 || target > pageCount;
    const shared = {
      className: "forge-ui-button forge-ui-button--ghost forge-ui-page",
      "aria-label": name,
      "aria-current": current ? ("page" as const) : undefined,
      "data-size": "sm",
    };
    if (hrefFor && !disabled) return <AppLink {...shared} href={hrefFor(target)}>{content}</AppLink>;
    return (
      <button {...shared} type="button" disabled={disabled} onClick={() => onPageChange?.(target)}>
        {content}
      </button>
    );
  };
  return (
    <nav {...props} aria-label={label} className={`forge-ui-pagination ${className}`}>
      <ul>
        <li>{item(page - 1, <Icon name="chevron-left" />, "Previous page")}</li>
        {pageList(page, pageCount, siblings).map((entry, index) => (
          <li key={entry === "gap" ? `gap-${index}` : entry}>
            {entry === "gap"
              ? <span className="forge-ui-page-gap" aria-hidden="true">…</span>
              : item(entry, entry, `Page ${entry}`, entry === page)}
          </li>
        ))}
        <li>{item(page + 1, <Icon name="chevron-right" />, "Next page")}</li>
      </ul>
    </nav>
  );
}

export interface ImageProps extends Omit<ComponentPropsWithRef<"img">, "alt" | "width" | "height" | "placeholder"> {
  src: string;
  /** Describe the image, or pass `""` when it is decorative. */
  alt: string;
  /** Intrinsic size in pixels. Reserves the space so the page does not jump. */
  width: number;
  height: number;
  /** How the image fills its box when CSS changes its shape. */
  fit?: "cover" | "contain";
  /** Load immediately with high priority (the main image above the fold). */
  priority?: boolean;
  /** A CSS color or gradient shown until the image has loaded. */
  placeholder?: string;
  rounded?: boolean;
}

/**
 * An image that reserves its space, loads lazily, and fades in over a placeholder.
 * In single-file builds, `import photo from "./photo.jpg"` gives a data URL that works here.
 */
export function Image({
  src, alt, width, height, fit = "cover", priority, placeholder, rounded, className = "", style, onLoad, ...props
}: ImageProps) {
  const ref = useRef<HTMLImageElement>(null);
  const [loaded, setLoaded] = useState(false);
  // The image may finish before React attaches the handler (cached, or server-rendered).
  useEffect(() => {
    if (ref.current?.complete) setLoaded(true);
  }, [src]);
  return (
    <span className={cx("forge-ui-image", className)} data-loaded={loaded || undefined} data-rounded={rounded || undefined}
      style={{ aspectRatio: `${width} / ${height}`, background: placeholder, ...style }}>
      <img {...props} ref={ref} src={src} alt={alt} width={width} height={height} data-fit={fit}
        loading={priority ? "eager" : "lazy"} decoding="async" fetchPriority={priority ? "high" : undefined}
        onLoad={(event) => {
          setLoaded(true);
          onLoad?.(event);
        }} />
    </span>
  );
}

export interface PageHeaderProps extends Omit<ComponentPropsWithRef<"header">, "title"> {
  /** Rendered as the page's `h1`. */
  title: ReactNode;
  description?: ReactNode;
  /** Buttons for the page's main actions, shown beside the title. */
  actions?: ReactNode;
  /** Usually `<Breadcrumbs />`, shown above the title. */
  eyebrow?: ReactNode;
}

export function PageHeader({ title, description, actions, eyebrow, className = "", children, ...props }: PageHeaderProps) {
  return (
    <header {...props} className={`forge-ui-page-header ${className}`}>
      {eyebrow != null && <div className="forge-ui-page-header-eyebrow">{eyebrow}</div>}
      <div className="forge-ui-page-header-row">
        <div className="forge-ui-page-header-text">
          <h1 className="forge-ui-heading" data-size="xl">{title}</h1>
          {description != null && <p className="forge-ui-page-header-description">{description}</p>}
        </div>
        {actions != null && <div className="forge-ui-page-header-actions">{actions}</div>}
      </div>
      {children}
    </header>
  );
}
