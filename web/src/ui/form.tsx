// Form parts, the counterparts of the Mac app's sheets and their controls: a form page (a sheet on
// a wide screen, the whole screen on a phone, with its buttons in a bar at the bottom), labeled
// fields, text inputs, a switch, a stepper, radio rows, segmented choices, checkboxes and a picker
// of images.

import type { ComponentChildren } from "preact";
import { useRef } from "preact/hooks";
import { t } from "../lib/i18n.ts";
import { Icon } from "./Icon.tsx";
import "./form.css";

/** A form on a page of its own: its title and what it is about, its fields, and its buttons kept in sight at the bottom. */
export function FormPage({ title, sub, icon, children, footer }: { title: string; sub?: ComponentChildren; icon?: ComponentChildren; children: ComponentChildren; footer: ComponentChildren }) {
  return (
    <div class="page form-page">
      <div class="form-sheet card">
        <div class={"form-head" + (icon ? " with-icon" : "")}>
          {icon}
          <div class="grow">
            <h1 class="display form-title">{title}</h1>
            {sub && <div class="secondary form-sub">{sub}</div>}
          </div>
        </div>
        <div class="form-body">{children}</div>
        <div class="form-foot">{footer}</div>
      </div>
    </div>
  );
}

/** A field: its name over its control, with a line under it when there is something to say. One
 * holding several controls (`group`), or with something beside its name (`accessory`), is not a
 * label, so a click on its name does not reach the first of them. */
export function Field({ label, note, tone, accessory, group, children }: { label?: string; note?: ComponentChildren; tone?: "red" | "amber"; accessory?: ComponentChildren; group?: boolean; children: ComponentChildren }) {
  const head = accessory ? (
    <span class="field-label field-head">
      <span>{label}</span>
      {accessory}
    </span>
  ) : (
    label && <span class="field-label">{label}</span>
  );
  const body = (
    <>
      {head}
      {children}
      {note && <span class={"caption" + (tone ? ` fg-${tone}` : "")}>{note}</span>}
    </>
  );
  return group || accessory ? <div class="field">{body}</div> : <label class="field">{body}</label>;
}

/** A line of text. `mono` is for paths and expressions: monospaced, and not corrected or capitalized
 * as typed. `secret` hides what is typed; `onEnter` runs on Return. */
export function TextInput({ value, onInput, placeholder, mono, label, secret, disabled, onEnter }: { value: string; onInput: (v: string) => void; placeholder?: string; mono?: boolean; label?: string; secret?: boolean; disabled?: boolean; onEnter?: () => void }) {
  const attrs = {
    class: "input" + (mono ? " mono" : ""),
    value,
    placeholder,
    disabled,
    "aria-label": label ?? placeholder,
    autocapitalize: mono ? ("off" as const) : undefined,
    autocorrect: mono ? "off" : undefined,
    spellcheck: mono || secret ? false : undefined,
    autocomplete: secret ? "off" : undefined,
    onInput: (e: Event) => onInput((e.target as HTMLInputElement).value),
    onKeyDown: (e: KeyboardEvent) => {
      if (e.key === "Enter" && onEnter) onEnter();
    },
  };
  return secret ? <input type="password" {...attrs} /> : <input type="text" {...attrs} />;
}

export function TextArea({ value, onInput, placeholder, rows = 6, invalid, label, mono }: { value: string; onInput: (v: string) => void; placeholder?: string; rows?: number; invalid?: boolean; label?: string; mono?: boolean }) {
  return (
    <textarea
      class={"input textarea" + (invalid ? " invalid" : "") + (mono ? " mono" : "")}
      rows={rows}
      value={value}
      placeholder={placeholder}
      aria-label={label ?? placeholder}
      spellcheck={mono ? false : undefined}
      onInput={(e) => onInput((e.target as HTMLTextAreaElement).value)}
    />
  );
}

/** A switch (Mac: `Toggle`), labeled for whoever reads it out. */
export function Switch({ on, onChange, label, disabled }: { on: boolean; onChange: (v: boolean) => void; label: string; disabled?: boolean }) {
  return (
    <button type="button" role="switch" class={"switch" + (on ? " on" : "")} aria-checked={on} aria-label={label} title={label} disabled={disabled} onClick={() => onChange(!on)}>
      <span class="knob" />
    </button>
  );
}

/** A switch with its title and a line under it (Mac: a `Toggle` with a two-line label). */
export function SwitchRow({ on, onChange, title, sub, disabled }: { on: boolean; onChange: (on: boolean) => void; title: string; sub?: string; disabled?: boolean }) {
  return (
    <div class={"switch-row" + (disabled ? " disabled" : "")}>
      <span class="grow">
        <span>{title}</span>
        {sub && <span class="caption block">{sub}</span>}
      </span>
      <Switch on={on} onChange={onChange} label={title} disabled={disabled} />
    </div>
  );
}

/** A row of choices, one selected (the Mac's segmented control). */
export function Segmented<T extends string>({ value, options, onChange, label }: { value: T; options: [T, string][]; onChange: (v: T) => void; label: string }) {
  return (
    <div class="choices" role="radiogroup" aria-label={label}>
      {options.map(([v, l]) => (
        <button type="button" role="radio" aria-checked={v === value} class={"choice" + (v === value ? " on" : "")} onClick={() => onChange(v)}>
          {l}
        </button>
      ))}
    </div>
  );
}

/** A checkbox with its text, for picking several. */
export function Check({ on, onChange, children }: { on: boolean; onChange: (on: boolean) => void; children: ComponentChildren }) {
  return (
    <label class="check-row">
      <input type="checkbox" checked={on} onChange={(e) => onChange((e.target as HTMLInputElement).checked)} />
      <span class="grow">{children}</span>
    </label>
  );
}

/** Ask before something that can't be taken back: the browser's own dialog. */
export function confirmed(question: string, detail = ""): boolean {
  return window.confirm(detail ? `${question}\n\n${detail}` : question);
}

/** A number in a range, with − and + beside it (Mac: `Stepper`). */
export function Stepper({ value, min, max, onChange, label, children }: { value: number; min: number; max: number; onChange: (v: number) => void; label: string; children: ComponentChildren }) {
  return (
    <span class="stepper" role="group" aria-label={label}>
      <span class="caption">{children}</span>
      <button type="button" class="icon-btn" aria-label={t`减少`} disabled={value <= min} onClick={() => onChange(Math.max(min, value - 1))}>
        <Icon name="minus" size={13} />
      </button>
      <button type="button" class="icon-btn" aria-label={t`增加`} disabled={value >= max} onClick={() => onChange(Math.min(max, value + 1))}>
        <Icon name="plus" size={13} />
      </button>
    </span>
  );
}

/** One of a few choices, as a radio row with its glyph. */
export function Radio({ on, onSelect, children, disabled, label }: { on: boolean; onSelect: () => void; children: ComponentChildren; disabled?: boolean; label?: string }) {
  return (
    <button type="button" role="radio" class={"radio" + (on ? " on" : "")} aria-checked={on} aria-label={label} disabled={disabled} onClick={onSelect}>
      <Icon name={on ? "largecircle.fill.circle" : "circle"} size={16} />
      <span>{children}</span>
    </button>
  );
}

/** A menu of values (Mac: `Picker`). */
export function Select<T extends string | number>({ value, options, onChange, label, disabled }: { value: T; options: [T, string][]; onChange: (v: T) => void; label: string; disabled?: boolean }) {
  return (
    <select
      class="input select"
      aria-label={label}
      disabled={disabled}
      value={String(value)}
      onChange={(e) => {
        const raw = (e.target as HTMLSelectElement).value;
        const hit = options.find(([v]) => String(v) === raw);
        if (hit) onChange(hit[0]);
      }}
    >
      {options.map(([v, name]) => (
        <option value={String(v)}>{name}</option>
      ))}
    </select>
  );
}

/** An image the person added, as the service takes it (Mac: `PastedImage.json`). */
export type PickedImage = { data: string; name: string; mimeType: string; url: string };

/** The service's limits on images sent with a message (Mac: `PastedImage.maxImages`, `maxBytes`). */
export const MAX_IMAGES = 10;
export const MAX_IMAGE_BYTES = 3_750_000;

/** Read image files as the service takes them; ones too large are left out and named in `refused`. */
export async function readImages(files: Iterable<File>): Promise<{ images: PickedImage[]; refused: string[] }> {
  const images: PickedImage[] = [];
  const refused: string[] = [];
  for (const f of files) {
    if (!f.type.startsWith("image/")) continue;
    if (f.size > MAX_IMAGE_BYTES) {
      refused.push(f.name);
      continue;
    }
    const buf = new Uint8Array(await f.arrayBuffer());
    let bin = "";
    for (let i = 0; i < buf.length; i += 0x8000) bin += String.fromCharCode(...buf.subarray(i, i + 0x8000));
    images.push({ data: btoa(bin), name: f.name || "image.png", mimeType: f.type, url: URL.createObjectURL(f) });
  }
  return { images, refused };
}

/** The images added to a message, each with a way to take it out, and a button to add more. */
export function ImageStrip({ images, onChange }: { images: PickedImage[]; onChange: (v: PickedImage[]) => void }) {
  if (images.length === 0) return null;
  return (
    <div class="image-strip">
      {images.map((im, i) => (
        <span class="image-chip">
          <img src={im.url} alt={im.name} />
          <button type="button" class="icon-btn" aria-label={t`移除 ${im.name}`} onClick={() => onChange(images.filter((_, j) => j !== i))}>
            <Icon name="xmark" size={11} />
          </button>
        </span>
      ))}
    </div>
  );
}

/** A button that picks images from the device (on a phone: the camera roll or the camera). */
export function AddImages({ onPick, disabled }: { onPick: (files: File[]) => void; disabled?: boolean }) {
  const input = useRef<HTMLInputElement>(null);
  return (
    <>
      <button type="button" class="icon-btn" aria-label={t`添加图片`} title={t`添加图片`} disabled={disabled} onClick={() => input.current?.click()}>
        <Icon name="paperclip" />
      </button>
      <input
        ref={input}
        type="file"
        accept="image/*"
        multiple
        hidden
        onChange={(e) => {
          const el = e.target as HTMLInputElement;
          onPick([...(el.files ?? [])]);
          el.value = "";
        }}
      />
    </>
  );
}
