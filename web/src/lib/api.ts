// The Janus HTTP API as the web client calls it: same-origin, with the paired device's cookie
// (`janus_device`, set by `POST /api/devices/claim`). A 401 means this browser is not paired (or
// was revoked): every page gives way to the pairing page. Failures carry the service's message,
// which is written to be shown as it is (docs/interfaces/20261031-http_api.md#errors).

import { signal } from "@preact/signals";

// The service's JSON, read field by field as the Mac app's `JSON` does.
export type Json = any;

/** This browser is not paired with the service (a 401 came back). */
export const unpaired = signal(false);

export class ApiError extends Error {
  readonly status: number;
  constructor(status: number, message: string) {
    super(message);
    this.status = status;
  }
}

async function call(method: string, path: string, body?: unknown): Promise<Json> {
  const init: RequestInit = { method, credentials: "same-origin", headers: {} };
  if (body !== undefined) {
    init.body = JSON.stringify(body);
    (init.headers as Record<string, string>)["content-type"] = "application/json";
  }
  let res: Response;
  try {
    res = await fetch(path, init);
  } catch (e) {
    throw new ApiError(0, e instanceof Error ? e.message : String(e));
  }
  if (res.status === 401) unpaired.value = true;
  const text = await res.text();
  if (!res.ok) throw new ApiError(res.status, text || res.statusText);
  return text ? JSON.parse(text) : null;
}

export const api = {
  get: (path: string) => call("GET", path),
  post: (path: string, body: unknown = {}) => call("POST", path, body),
  put: (path: string, body: unknown) => call("PUT", path, body),
  delete: (path: string) => call("DELETE", path),
};

/** `?a=b&…` from the values that are set. */
export function query(params: Record<string, string | number | boolean | null | undefined>): string {
  const q = new URLSearchParams();
  for (const [k, v] of Object.entries(params)) if (v !== null && v !== undefined && v !== "") q.set(k, String(v));
  const s = q.toString();
  return s ? `?${s}` : "";
}
