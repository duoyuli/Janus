// A path that names nothing.

import { t } from "../lib/i18n.ts";
import { usePageTitle } from "../lib/title.ts";
import { Note } from "../ui/kit.tsx";

export function MissingPage() {
  usePageTitle("");
  return (
    <div class="page">
      <Note text={t`没有这个页面。`} />
    </div>
  );
}
