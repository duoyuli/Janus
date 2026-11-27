declare module "virtual:janus-strings" {
  /** Chinese key → English, from the Mac app's en.lproj/Localizable.strings. */
  const english: Record<string, string>;
  export default english;
}

interface Window {
  /** What the pages draw, for the end-to-end tests (`src/lib/testing.ts`). */
  __janus?: { state: () => Record<string, unknown> };
}
