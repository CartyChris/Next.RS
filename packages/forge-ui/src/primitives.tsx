import type { ComponentPropsWithRef, ReactNode } from "react";

export type ThemeMode = "system" | "light" | "dark";
export type Spacing = "xs" | "sm" | "md" | "lg" | "xl";
export type Tone = "neutral" | "info" | "success" | "warning" | "danger";

export interface ThemeProps extends ComponentPropsWithRef<"div"> {
  theme?: ThemeMode;
}

export function Theme({ theme = "system", className = "", ...props }: ThemeProps) {
  return <div {...props} className={`forge-ui ${className}`} data-theme={theme} />;
}

export interface ContainerProps extends ComponentPropsWithRef<"div"> {
  size?: "narrow" | "default" | "wide";
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

export interface GridProps extends StackProps {
  columns?: 1 | 2 | 3 | 4;
}

export function Grid({ columns = 3, gap = "md", className = "", ...props }: GridProps) {
  return <div {...props} className={`forge-ui-grid ${className}`} data-columns={columns} data-gap={gap} />;
}

export type ButtonVariant = "primary" | "secondary" | "ghost" | "danger";
export type ButtonSize = "sm" | "md" | "lg";

export interface ButtonAppearance {
  variant?: ButtonVariant;
  size?: ButtonSize;
  fullWidth?: boolean;
}

export interface ButtonProps extends ButtonAppearance, ComponentPropsWithRef<"button"> {}
export interface ButtonLinkProps extends ButtonAppearance, ComponentPropsWithRef<"a"> {
  href: string;
}

export function Button({
  variant = "primary", size = "md", fullWidth, className = "", type = "button", ...props
}: ButtonProps) {
  return <button {...props} type={type} className={`forge-ui-button forge-ui-button--${variant} ${className}`}
    data-size={size} data-full-width={fullWidth || undefined} />;
}

export function ButtonLink({
  variant = "primary", size = "md", fullWidth, className = "", ...props
}: ButtonLinkProps) {
  return <a {...props} className={`forge-ui-button forge-ui-button--${variant} ${className}`}
    data-size={size} data-full-width={fullWidth || undefined} />;
}

export type CardProps = ComponentPropsWithRef<"div">;

export function Card({ className = "", ...props }: CardProps) {
  return <div {...props} className={`forge-ui-card ${className}`} />;
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
  tone?: Tone;
}

export function Badge({ tone = "neutral", className = "", ...props }: BadgeProps) {
  return <span {...props} className={`forge-ui-badge ${className}`} data-tone={tone} />;
}

export interface AlertProps extends Omit<ComponentPropsWithRef<"div">, "title"> {
  title?: ReactNode;
  tone?: Tone;
  live?: "off" | "polite" | "assertive";
}

export function Alert({ tone = "info", live = "off", title, children, className = "", ...props }: AlertProps) {
  const role = live === "assertive" ? "alert" : live === "polite" ? "status" : undefined;
  return (
    <div role={role} {...props} className={`forge-ui-alert ${className}`} data-tone={tone}>
      {title != null && <div className="forge-ui-alert-title">{title}</div>}
      {children}
    </div>
  );
}

export interface NavProps extends ComponentPropsWithRef<"nav"> {
  label: string;
}

export function Nav({ label, className = "", ...props }: NavProps) {
  return <nav {...props} aria-label={label} className={`forge-ui-nav ${className}`} />;
}

export interface NavLinkProps extends ComponentPropsWithRef<"a"> {
  href: string;
  current?: boolean | "page" | "location";
}

export function NavLink({ current, className = "", ...props }: NavLinkProps) {
  const ariaCurrent = current === true ? "page" : current || props["aria-current"];
  return <a {...props} aria-current={ariaCurrent} className={`forge-ui-nav-link ${className}`} />;
}
