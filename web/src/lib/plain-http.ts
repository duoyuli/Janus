// What the web client uses from the browser that a page served over plain HTTP still has. A phone
// opens Janus at a local network or Tailscale address (`http://100.x.y.z:7878`): not a secure
// context, so `crypto.randomUUID` and `navigator.clipboard` are not there (only `127.0.0.1` and
// `localhost` count as secure). Everything else goes through these.
// @project-doc docs/architecture/20261126-web.md#plain_http

/** A random id (a person's action's `commandId`), from `crypto.getRandomValues`, which every context has. */
export function randomId(): string {
  const b = crypto.getRandomValues(new Uint8Array(16));
  return [...b].map((x) => x.toString(16).padStart(2, "0")).join("");
}

/** Put `text` on the clipboard: the Clipboard API where the page may use it, otherwise a hidden
 * text area copied the old way. Rejects when neither works. */
export async function copyText(text: string): Promise<void> {
  if (window.isSecureContext && navigator.clipboard) {
    await navigator.clipboard.writeText(text);
    return;
  }
  const area = document.createElement("textarea");
  area.value = text;
  area.setAttribute("readonly", "");
  area.style.cssText = "position:fixed;top:0;left:0;opacity:0;pointer-events:none";
  document.body.appendChild(area);
  area.select();
  area.setSelectionRange(0, text.length);
  try {
    if (!document.execCommand("copy")) throw new Error("copy refused");
  } finally {
    area.remove();
  }
}
