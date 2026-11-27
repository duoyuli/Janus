// What this browser keeps for its own drawing (language, appearance, folded sections): never
// something the service owns. Everything else lives on the service.

const prefix = "janus.";

export function load<T>(name: string, fallback: T): T {
  try {
    const v = globalThis.localStorage?.getItem(prefix + name);
    return v === null || v === undefined ? fallback : (JSON.parse(v) as T);
  } catch {
    return fallback;
  }
}

export function save(name: string, value: unknown) {
  try {
    globalThis.localStorage?.setItem(prefix + name, JSON.stringify(value));
  } catch {
    // A private window or full storage: the choice lasts this visit.
  }
}
