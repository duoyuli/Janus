// Form parts, the counterparts of the Mac app's sheets and their controls: a form page (a sheet on
// a wide screen, the whole screen on a phone, with its buttons in a bar at the bottom), labeled
// fields, text inputs, a switch, a stepper, radio rows and a picker of images.

import type { ComponentChildren } from "preact";
import { useRef } from "preact/hooks";
import { t } from "../lib/i18n.ts";
import { Icon } from "./Icon.tsx";
import "./form.css";

/** A form on a page of its own: its title and what it is about, its fields, and its buttons kept in sight at the bottom. */
export function FormPage({ title, sub, children, footer }: { title: string; sub?: ComponentChildren; children: ComponentChildren; footer: ComponentChildren }) {
  return (
    <div class="page form-page">
      <div class="form-sheet card">
        <div class="form-head">
          <h1 class="display form-title">{title}</h1>
          {sub && <div class="secondary form-sub">{sub}</div>}
        </div>
        <div class="form-body">{children}</div>
        <div class="form-foot">{footer}</div>
      </div>
    </div>
  );
}

/** A field: its name over its control, with a line under it when there is something to say. */
export function Field({ label, note, tone, children }: { label?: string; note?: ComponentChildren; tone?: "red" | "amber"; children: ComponentChildren }) {
  return (
    <label class="field">
      {label && <span class="field-label">{label}</span>}
      {children}
      {note && <span class={"caption" + (tone ? ` fg-${tone}` : "")}>{note}</span>}
    </label>
  );
}

/** A line of text. `mono` is for paths and expressions: monospaced, and not corrected or capitalized as typed. */
export function TextInput({ value, onInput, placeholder, mono, label }: { value: string; onInput: (v: string) => void; placeholder?: string; mono?: boolean; label?: string }) {
  return (
    <input
      class={"input" + (mono ? " mono" : "")}
      value={value}
      placeholder={placeholder}
      aria-label={label}
      autocapitalize={mono ? "off" : undefined}
      autocorrect={mono ? "off" : undefined}
      spellcheck={mono ? false : undefined}
      onInput={(e) => onInput((e.target as HTMLInputElement).value)}
    />
  );
}

export function TextArea({ value, onInput, placeholder, rows = 6, invalid, label }: { value: string; onInput: (v: string) => void; placeholder?: string; rows?: number; invalid?: boolean; label?: string }) {
  return <textarea class={"input textarea" + (invalid ? " invalid" : "")} rows={rows} value={value} placeholder={placeholder} aria-label={label} onInput={(e) => onInput((e.target as HTMLTextAreaElement).value)} />;
}

/** A switch (Mac: `Toggle`), labeled for whoever reads it out. */
export function Switch({ on, onChange, label, disabled }: { on: boolean; onChange: (v: boolean) => void; label: string; disabled?: boolean }) {
  return (
    <button type="button" role="switch" class={"switch" + (on ? " on" : "")} aria-checked={on} aria-label={label} title={label} disabled={disabled} onClick={() => onChange(!on)}>
      <span class="knob" />
    </button>
  );
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
export function Select<T extends string | number>({ value, options, onChange, label }: { value: T; options: [T, string][]; onChange: (v: T) => void; label: string }) {
  return (
    <select
      class="input select"
      aria-label={label}
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
