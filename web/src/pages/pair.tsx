// Pairing this browser with the Janus service. The Mac app (设置 › 手机访问) shows a code as a QR
// code whose link opens `/pair/<code>`; this page hands the code in (`POST /api/devices/claim`)
// and the service answers with the device's cookie (`janus_device`). Without it every request is
// refused (401) and the window shows how to pair instead of a page
// (docs/architecture/20261126-web.md#pairing).

import { useEffect, useState } from "preact/hooks";
import { api, unpaired } from "../lib/api.ts";
import { t } from "../lib/i18n.ts";
import { go } from "../lib/router.ts";
import { boot } from "../lib/store.ts";
import { follow } from "../lib/stream.ts";
import { useDrawn } from "../lib/testing.ts";
import { usePageTitle } from "../lib/title.ts";
import { Icon } from "../ui/Icon.tsx";
import { Card, Spinner } from "../ui/kit.tsx";

/** What this device is called in the Mac app's list of paired devices: its kind and browser. */
export function deviceName(ua: string): string {
  const device = /iPhone/.test(ua) ? "iPhone" : /iPad/.test(ua) ? "iPad" : /Android/.test(ua) ? "Android" : /Macintosh/.test(ua) ? "Mac" : /Windows/.test(ua) ? "Windows" : /Linux/.test(ua) ? "Linux" : "";
  const browser = /Edg\//.test(ua) ? "Edge" : /Firefox\//.test(ua) ? "Firefox" : /Chrome\//.test(ua) ? "Chrome" : /Safari\//.test(ua) ? "Safari" : "";
  return [device, browser].filter(Boolean).join(" · ") || "Browser";
}

export function PairPage({ code }: { code: string }) {
  usePageTitle(t`配对`);
  const [error, setError] = useState<string | null>(null);
  useDrawn("pair", () => ({ code, error }));
  useEffect(() => {
    let gone = false;
    api
      .post("/api/devices/claim", { code, name: deviceName(navigator.userAgent) })
      .then(async () => {
        if (gone) return;
        unpaired.value = false;
        go("/inbox", true);
        await boot();
        follow();
      })
      .catch((e: Error) => !gone && setError(e.message));
    return () => {
      gone = true;
    };
  }, [code]);
  return (
    <div class="pair">
      <Card>
        <div class="pair-body">
          <Icon name="iphone" size={28} />
          {error ? (
            <>
              <div class="h2">{t`配对没有成功`}</div>
              <div class="secondary">{error}</div>
              <div class="secondary">{t`在 Mac 上的 Janus 设置 › 手机访问 重新扫码。`}</div>
            </>
          ) : (
            <>
              <Spinner />
              <div class="secondary">{t`正在和 Janus 配对…`}</div>
            </>
          )}
        </div>
      </Card>
    </div>
  );
}

/** The window while this browser is not paired: where to pair it. */
export function UnpairedPage() {
  usePageTitle(t`配对`);
  useDrawn("unpaired", () => ({ unpaired: true }));
  return (
    <div class="pair">
      <Card>
        <div class="pair-body">
          <Icon name="iphone" size={28} />
          <div class="h2">{t`这个浏览器还没有和 Janus 配对`}</div>
          <div class="secondary">{t`请在 Mac 上的 Janus 设置 › 手机访问 扫码配对`}</div>
        </div>
      </Card>
    </div>
  );
}
