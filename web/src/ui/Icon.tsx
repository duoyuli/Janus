// The glyphs of the web client, named after the SF Symbols the Mac app draws in the same places,
// so a page ported from Swift keeps its names (`Image(systemName: "clock")` → `<Icon name="clock" />`).
// Line drawings on a 24-unit grid in the current text color; `fill` variants are the same shapes
// filled. A name not drawn here shows a dot (and says so in development).

const P: Record<string, string> = {
  tray: "M3 13l3-8h12l3 8v6a1 1 0 0 1-1 1H4a1 1 0 0 1-1-1z M3 13h5l1 3h6l1-3h5",
  "tray.full": "M3 13l3-8h12l3 8v6a1 1 0 0 1-1 1H4a1 1 0 0 1-1-1z M3 13h5l1 3h6l1-3h5 M8 8h8 M7 10.5h10",
  "checkmark.seal": "M12 2.5l2.4 1.8 3-.2 1 2.8 2.6 1.5-.9 2.9.9 2.9-2.6 1.5-1 2.8-3-.2L12 21.5l-2.4-1.8-3 .2-1-2.8-2.6-1.5.9-2.9-.9-2.9 2.6-1.5 1-2.8 3 .2z M8.5 12l2.5 2.5 4.5-5",
  "arrow.triangle.merge": "M6 3v6c0 3 2 4.5 6 6.5V21 M18 3v6c0 3-2 4.5-6 6.5 M3 6l3-3 3 3 M15 6l3-3 3 3",
  "hand.raised": "M8 13V5.5a1.5 1.5 0 0 1 3 0V11 M11 4.5a1.5 1.5 0 0 1 3 0V11 M14 5.5a1.5 1.5 0 0 1 3 0V12 M17 8.5a1.5 1.5 0 0 1 3 0V14a7 7 0 0 1-7 7h-1a6 6 0 0 1-5-2.7L4.3 14.6a1.5 1.5 0 0 1 2.4-1.8L8 14.5",
  "pause.circle": "M12 3a9 9 0 1 1 0 18a9 9 0 0 1 0-18z M10 9v6 M14 9v6",
  "xmark.octagon": "M8.3 3h7.4L21 8.3v7.4L15.7 21H8.3L3 15.7V8.3z M9 9l6 6 M15 9l-6 6",
  "exclamationmark.octagon": "M8.3 3h7.4L21 8.3v7.4L15.7 21H8.3L3 15.7V8.3z M12 7.5v5.5 M12 16.5v.01",
  "exclamationmark.circle": "M12 3a9 9 0 1 1 0 18a9 9 0 0 1 0-18z M12 7.5v5.5 M12 16.5v.01",
  "exclamationmark.triangle": "M12 3.5L22 20H2z M12 9.5v4.5 M12 17v.01",
  "questionmark.bubble": "M4 4h16v12H9l-5 4z M9.5 8.5a2.5 2.5 0 0 1 5 0c0 1.5-2.5 2-2.5 3.5 M12 14v.01",
  "questionmark.circle": "M12 3a9 9 0 1 1 0 18a9 9 0 0 1 0-18z M9.5 9.5a2.5 2.5 0 0 1 5 0c0 1.5-2.5 2-2.5 3.5 M12 16.5v.01",
  "text.bubble": "M4 4h16v12H9l-5 4z M8 8.5h8 M8 11.5h5",
  "square.and.pencil": "M11 4H5a1 1 0 0 0-1 1v14a1 1 0 0 0 1 1h14a1 1 0 0 0 1-1v-6 M17.5 3.5l3 3L12 15l-4 1 1-4z",
  hourglass: "M6 3h12 M6 21h12 M7 3c0 5 10 5 10 9s-10 4-10 9 M17 3c0 5-10 5-10 9s10 4 10 9",
  "doc.badge.ellipsis": "M6 3h8l4 4v14H6z M14 3v4h4 M9 15h.01 M12 15h.01 M15 15h.01",
  "arrow.right.to.line": "M3 12h13 M12 7l5 5-5 5 M20 5v14",
  "brain.head.profile": "M12 3a7 7 0 0 1 7 7c0 1.5.5 2 1.5 3.5L19 14v3a2 2 0 0 1-2 2h-2v2H8v-4.5A7 7 0 0 1 12 3z M10 9a2 2 0 0 1 4 0 M12 9v4",
  clock: "M12 3a9 9 0 1 1 0 18a9 9 0 0 1 0-18z M12 7v5l3 2",
  "clock.arrow.circlepath": "M3.5 12a8.5 8.5 0 1 0 2.5-6 M3 3v4h4 M12 8v4l3 2",
  "eye.slash": "M3 3l18 18 M10.6 6.1A9.8 9.8 0 0 1 12 6c5 0 9 6 9 6a15 15 0 0 1-2.6 3.1 M6.6 6.6C4.3 8.1 3 12 3 12s4 6 9 6c1.6 0 3-.4 4.3-1.1 M9.9 9.9a3 3 0 0 0 4.2 4.2",
  gearshape: "M19.03 10.43 L21.52 10.76 L21.52 13.24 L19.03 13.57 L18.08 15.86 L19.61 17.85 L17.85 19.61 L15.86 18.08 L13.57 19.03 L13.24 21.52 L10.76 21.52 L10.43 19.03 L8.14 18.08 L6.15 19.61 L4.39 17.85 L5.92 15.86 L4.97 13.57 L2.48 13.24 L2.48 10.76 L4.97 10.43 L5.92 8.14 L4.39 6.15 L6.15 4.39 L8.14 5.92 L10.43 4.97 L10.76 2.48 L13.24 2.48 L13.57 4.97 L15.86 5.92 L17.85 4.39 L19.61 6.15 L18.08 8.14Z M12 8.8a3.2 3.2 0 1 1 0 6.4a3.2 3.2 0 0 1 0-6.4z",
  plus: "M12 5v14 M5 12h14",
  "person.2": "M9 11a3.5 3.5 0 1 0 0-7a3.5 3.5 0 0 0 0 7z M2.5 20c.5-3.5 3-5.5 6.5-5.5s6 2 6.5 5.5 M16 4.3a3.5 3.5 0 0 1 0 6.4 M18 14.8c2 .8 3.2 2.6 3.5 5.2",
  "bubble.left.and.bubble.right": "M3 4h12v9H7l-4 3z M9 16v1h8l4 3V9h-3",
  folder: "M3 6a1 1 0 0 1 1-1h5l2 2h9a1 1 0 0 1 1 1v10a1 1 0 0 1-1 1H4a1 1 0 0 1-1-1z",
  "chevron.right": "M9 5l7 7-7 7",
  "chevron.left": "M15 5l-7 7 7 7",
  "chevron.down": "M5 9l7 7 7-7",
  xmark: "M6 6l12 12 M18 6L6 18",
  checkmark: "M5 12.5l4.5 4.5L19 7",
  "arrow.up": "M12 20V5 M6 11l6-6 6 6",
  "ellipsis": "M5 12h.01 M12 12h.01 M19 12h.01",
  "sidebar.left": "M4 4h16v16H4z M9 4v16",
  "line.3.horizontal": "M4 7h16 M4 12h16 M4 17h16",
  "wifi.slash": "M3 3l18 18 M8.5 16.5a5 5 0 0 1 7 0 M5 13a10 10 0 0 1 4.5-2.5 M14.5 10.5A10 10 0 0 1 19 13 M2 9.5a15 15 0 0 1 4-2.6 M12 20h.01",
  iphone: "M8 2.5h8a1.5 1.5 0 0 1 1.5 1.5v16a1.5 1.5 0 0 1-1.5 1.5H8A1.5 1.5 0 0 1 6.5 20V4A1.5 1.5 0 0 1 8 2.5z M11 18.5h2",
  "doc.text": "M6 3h8l4 4v14H6z M14 3v4h4 M9 12h6 M9 15h6 M9 18h4",
  brain: "M9 4a3 3 0 0 0-3 3 3 3 0 0 0-2 5 3 3 0 0 0 2 5 3 3 0 0 0 3 3h0a3 3 0 0 0 3-3V7a3 3 0 0 0-3-3z M15 4a3 3 0 0 1 3 3 3 3 0 0 1 2 5 3 3 0 0 1-2 5 3 3 0 0 1-3 3h0a3 3 0 0 1-3-3",
  "ellipsis.circle": "M12 3a9 9 0 1 1 0 18a9 9 0 0 1 0-18z M8 12h.01 M12 12h.01 M16 12h.01",
  "arrow.triangle.2.circlepath": "M4 12a8 8 0 0 1 13.7-5.6L20 9 M20 4v5h-5 M20 12a8 8 0 0 1-13.7 5.6L4 15 M4 20v-5h5",
  paperclip: "M20 11.5l-8.2 8.2a5 5 0 0 1-7.1-7.1l8.5-8.5a3.3 3.3 0 0 1 4.7 4.7l-8.5 8.5a1.7 1.7 0 0 1-2.4-2.4L14.8 7",
  "checkmark.circle": "M12 3a9 9 0 1 1 0 18a9 9 0 0 1 0-18z M8 12.5l3 3 5-6",
  circle: "M12 3a9 9 0 1 1 0 18a9 9 0 0 1 0-18z",
  "largecircle.fill.circle": "M12 3a9 9 0 1 1 0 18a9 9 0 0 1 0-18z M12 8a4 4 0 1 1 0 8a4 4 0 0 1 0-8z",
  minus: "M5 12h14",
  trash: "M4 7h16 M9 7V4h6v3 M6 7l1 13h10l1-13 M10 11v6 M14 11v6",
  "books.vertical": "M4 4h4v16H4z M10 4h4v16h-4z M16 5l3.5-1 2.5 15.5-3.5 1z",
  "list.bullet.rectangle": "M3 4h18v16H3z M7 9h.01 M7 12h.01 M7 15h.01 M10 9h7 M10 12h7 M10 15h7",
  shippingbox: "M12 3l8 4v10l-8 4-8-4V7z M4 7l8 4 8-4 M12 11v10",
  terminal: "M3 4h18v16H3z M7 9l3 3-3 3 M12 15h5",
  globe: "M12 3a9 9 0 1 1 0 18a9 9 0 0 1 0-18z M3 12h18 M12 3c2.5 2.5 3.5 6 3.5 9s-1 6.5-3.5 9 M12 3c-2.5 2.5-3.5 6-3.5 9s1 6.5 3.5 9",
  sparkles: "M10 3l1.6 4.4L16 9l-4.4 1.6L10 15l-1.6-4.4L4 9l4.4-1.6z M18 14l.8 2.2L21 17l-2.2.8L18 20l-.8-2.2L15 17l2.2-.8z",
  link: "M10 14a4 4 0 0 0 5.7 0l3-3a4 4 0 0 0-5.7-5.7l-1 1 M14 10a4 4 0 0 0-5.7 0l-3 3a4 4 0 0 0 5.7 5.7l1-1",
  eye: "M2.5 12S6.5 5.5 12 5.5 21.5 12 21.5 12 17.5 18.5 12 18.5 2.5 12 2.5 12z M12 9a3 3 0 1 1 0 6a3 3 0 0 1 0-6z",
  "exclamationmark.shield": "M12 3l8 3v6c0 4.5-3.5 8-8 9-4.5-1-8-4.5-8-9V6z M12 8v5 M12 16.5v.01",
  "checkmark.shield": "M12 3l8 3v6c0 4.5-3.5 8-8 9-4.5-1-8-4.5-8-9V6z M8.5 12l2.5 2.5 4.5-5",
  pencil: "M16.5 3.5l4 4L8 20H4v-4z M14 6l4 4",
  "xmark.circle": "M12 3a9 9 0 1 1 0 18a9 9 0 0 1 0-18z M9 9l6 6 M15 9l-6 6",
  "plus.circle": "M12 3a9 9 0 1 1 0 18a9 9 0 0 1 0-18z M12 8v8 M8 12h8",
  "minus.circle": "M12 3a9 9 0 1 1 0 18a9 9 0 0 1 0-18z M8 12h8",
};

/** Names drawn filled: the shape of the plain name, filled with the current color. */
const FILLED = new Set([
  "checkmark.seal",
  "hand.raised",
  "pause.circle",
  "xmark.octagon",
  "exclamationmark.octagon",
  "exclamationmark.circle",
  "questionmark.bubble",
  "questionmark.circle",
  "text.bubble",
  "checkmark.circle",
]);

export function Icon({ name, size = 16, label, class: cls }: { name: string; size?: number; label?: string; class?: string }) {
  const filled = name.endsWith(".fill");
  const base = filled ? name.slice(0, -5) : name;
  const d = P[base];
  if (!d && import.meta.env.DEV) console.warn(`Icon: ${name} is not drawn`);
  const solid = filled && FILLED.has(base);
  return (
    <svg
      class={"icon" + (cls ? ` ${cls}` : "")}
      width={size}
      height={size}
      viewBox="0 0 24 24"
      fill="none"
      stroke="currentColor"
      stroke-width={solid ? 1.6 : 1.8}
      stroke-linecap="round"
      stroke-linejoin="round"
      role={label ? "img" : undefined}
      aria-label={label}
      aria-hidden={label ? undefined : true}
    >
      {d ? (
        solid ? (
          <SolidPath d={d} />
        ) : (
          <path d={d} />
        )
      ) : (
        <circle cx="12" cy="12" r="3" fill="currentColor" />
      )}
    </svg>
  );
}

/** A filled glyph: its outline (the first subpath) filled, the marks inside drawn in the page's color. */
function SolidPath({ d }: { d: string }) {
  const [outline, ...marks] = d.split(/ (?=M)/);
  return (
    <>
      <path d={outline} fill="currentColor" />
      {marks.length > 0 && <path d={marks.join(" ")} stroke="var(--card)" stroke-width={2} />}
    </>
  );
}
