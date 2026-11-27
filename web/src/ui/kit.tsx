// The web client's building blocks, each the counterpart of one in the Mac app's
// UI/Components.swift, with the same look (docs/architecture/20261031-frontend.md#design_tokens):
// cards are a neutral ground and a hairline, a status is a dot on a neutral capsule, the primary
// action is ink, not the accent. Pages compose these; they do not restyle them.

import type { ButtonHTMLAttributes, ComponentChildren } from "preact";
import { useEffect, useRef, useState } from "preact/hooks";
import type { Tone } from "../lib/status.ts";
import { Icon } from "./Icon.tsx";

export function Card({ children, pad = 16, class: cls }: { children: ComponentChildren; pad?: number; class?: string }) {
  return (
    <div class={"card" + (cls ? ` ${cls}` : "")} style={{ padding: `${pad}px` }}>
      {children}
    </div>
  );
}

/** A status or a short label: a colored dot and quiet text on a neutral capsule. */
export function Pill({ text, tone = "gray" }: { text: string; tone?: Tone }) {
  return (
    <span class="pill">
      {tone !== "gray" && <span class={`dot tone-${tone}`} aria-hidden="true" />}
      {text}
    </span>
  );
}

type ButtonProps = Omit<ButtonHTMLAttributes<HTMLButtonElement>, "class"> & {
  kind?: "primary" | "bordered" | "plain";
  small?: boolean;
  disabled?: boolean;
  children: ComponentChildren;
};

/** `primary` is the one way forward on a screen (ink); `bordered` the quieter choices. */
export function Button({ kind = "bordered", small, children, ...rest }: ButtonProps) {
  return (
    <button type="button" class={`btn btn-${kind}${small ? " btn-small" : ""}`} {...rest}>
      {children}
    </button>
  );
}

/** A button that is only a glyph: its label is read out and shown on hover. */
export function IconButton({ icon, label, onClick, size = 16, disabled }: { icon: string; label: string; onClick: () => void; size?: number; disabled?: boolean }) {
  return (
    <button type="button" class="icon-btn" aria-label={label} title={label} onClick={onClick} disabled={disabled}>
      <Icon name={icon} size={size} />
    </button>
  );
}

/** A page's or section's title, with what it covers and how many. */
export function SectionHeader({ title, sub, count }: { title: string; sub?: string; count?: number }) {
  return (
    <div class="section-header">
      <h1 class="display">{title}</h1>
      {count !== undefined && <span class="count">{count}</span>}
      {sub && <span class="sub">{sub}</span>}
    </div>
  );
}

/** A folding section: the whole title row opens and closes it (`RowDisclosureStyle`). */
export function Disclosure({ title, open, onToggle, children }: { title: ComponentChildren; open: boolean; onToggle: (open: boolean) => void; children: ComponentChildren }) {
  return (
    <section class="disclosure">
      <button type="button" class="disclosure-row" aria-expanded={open} onClick={() => onToggle(!open)}>
        <span class={"chevron" + (open ? " open" : "")} aria-hidden="true">
          <Icon name="chevron.right" size={11} />
        </span>
        <span class="disclosure-title">{title}</span>
      </button>
      {open && <div class="disclosure-body">{children}</div>}
    </section>
  );
}

/** The slate a role gets when it has no color of its own. */
const NEUTRAL = new Set(["", "#64748b", "#94a3b8"]);

/** A digital employee's tile (Mac: `Avatar`): its picture when it has one (`GET /api/roles/{id}/avatar`),
 * else its initials on its color. `role` is the role as `/api/setup` lists it. */
export function Avatar({ role, size = 22 }: { role: { id?: string; name?: string; init?: string; color?: string; avatar?: string } | undefined; size?: number }) {
  const radius = `${size * 0.3}px`;
  if (role?.avatar && role.id) {
    return <img class="avatar" src={`/api/roles/${encodeURIComponent(role.id)}/avatar?v=${role.avatar}`} alt="" width={size} height={size} style={{ borderRadius: radius, objectFit: "cover" }} />;
  }
  const color = (role?.color ?? "").toLowerCase();
  const text = tileText(initials(role?.init ?? "", role?.name ?? ""), size);
  return (
    <span
      class="avatar"
      style={{ width: `${size}px`, height: `${size}px`, borderRadius: radius, background: NEUTRAL.has(color) ? "#5b6675" : color, fontSize: `${Math.round(size * ([...text].length === 1 ? 0.5 : 0.4))}px` }}
      aria-hidden="true"
    >
      {text}
    </span>
  );
}

/** A digital employee's initials, or (none given) the first two letters of its name, without
 * spaces and in capitals (Mac: `Role.initials`). */
export function initials(given: string, name: string): string {
  return given || [...(name ?? "").replace(/ /g, "")].slice(0, 2).join("").toUpperCase() || "?";
}

/** Two CJK characters crowd a small tile; one reads at a glance. */
export function tileText(text: string, size: number): string {
  const first = text.codePointAt(0) ?? 0;
  return first >= 0x2e80 && size < 34 ? String.fromCodePoint(first) : text;
}

/** A menu that opens from a button: a short list of choices. Closes on a choice, Escape or a click outside. */
export function Menu({ icon, label, items }: { icon: string; label: string; items: { label: string; onSelect: () => void }[] }) {
  const [open, setOpen] = useState(false);
  const ref = useRef<HTMLDivElement>(null);
  useEffect(() => {
    if (!open) return;
    const close = (e: Event) => {
      if (e instanceof KeyboardEvent ? e.key === "Escape" : !ref.current?.contains(e.target as Node)) setOpen(false);
    };
    document.addEventListener("pointerdown", close);
    document.addEventListener("keydown", close);
    return () => {
      document.removeEventListener("pointerdown", close);
      document.removeEventListener("keydown", close);
    };
  }, [open]);
  return (
    <div class="menu" ref={ref}>
      <button type="button" class="icon-btn" aria-label={label} title={label} aria-haspopup="menu" aria-expanded={open} onClick={() => setOpen(!open)}>
        <Icon name={icon} />
      </button>
      {open && (
        <div class="menu-list" role="menu">
          {items.map((it) => (
            <button
              type="button"
              role="menuitem"
              class="menu-item"
              onClick={() => {
                setOpen(false);
                it.onSelect();
              }}
            >
              {it.label}
            </button>
          ))}
        </div>
      )}
    </div>
  );
}

/** A note on the page: a line that matters, with its tone's color for the mark. */
export function Note({ text, tone = "gray" }: { text: string; tone?: Tone }) {
  return (
    <div class={`note tone-border-${tone}`}>
      <span class={`dot tone-${tone}`} aria-hidden="true" />
      <span>{text}</span>
    </div>
  );
}

export function Spinner() {
  return <span class="spinner" role="progressbar" aria-busy="true" />;
}
