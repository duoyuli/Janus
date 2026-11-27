// Pairing this browser with the Janus service. The Mac app (设置 › 手机访问) shows a code as a QR
// code whose link opens `/pair/<code>`; this page hands the code in (`POST /api/devices/claim`)
// and the service answers with the device's cookie (`janus_device`). Without it every request is
// refused (401) and the window shows how to pair instead of a page. Settings › 手机访问 lists the
// paired devices and unpairs this one (docs/architecture/20261126-web.md#pairing).

import { useEffect, useState } from "preact/hooks";
import { api, type Json, unpaired } from "../lib/api.ts";
import { ago } from "../lib/format.ts";
import { t } from "../lib/i18n.ts";
import { go } from "../lib/router.ts";
import { boot, show } from "../lib/store.ts";
import { follow, onNotice, stop } from "../lib/stream.ts";
import { useDrawn } from "../lib/testing.ts";
import { usePageTitle } from "../lib/title.ts";
import { Icon } from "../ui/Icon.tsx";
import { Button, Card, Spinner } from "../ui/kit.tsx";

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

/** Settings › 手机访问 as a paired device sees it (Mac: `MobileAccessView`): the devices paired with
 * Janus, this one marked, and unpairing this one. The switch and the QR codes are on the Mac only:
 * a device may not open Janus to its network or pair others. */
export function PhoneAccess() {
  const [me, setMe] = useState<Json>(null);
  const [devices, setDevices] = useState<Json[]>([]);
  const [now, setNow] = useState(Date.now());
  useEffect(() => {
    const read = () => {
      api.get("/api/devices/me").then(setMe, () => {});
      api.get("/api/devices").then((d: Json[]) => {
        setDevices(d ?? []);
        setNow(Date.now());
      }, () => {});
    };
    read();
    return onNotice((name) => name === "devices" && read());
  }, []);
  useDrawn("phoneAccess", () => ({ me: me?.id ?? null, devices: devices.map((d) => d.id) }));
  const unpair = async () => {
    try {
      await api.delete(`/api/devices/${encodeURIComponent(me.id)}`);
      stop();
      unpaired.value = true;
    } catch (e) {
      show((e as Error).message);
    }
  };
  return (
    <Card>
      <div class="h2">{t`手机访问`}</div>
      <div class="secondary" style={{ margin: "4px 0 8px" }}>{t`打开或关闭手机访问、配对新的设备，在 Mac 上的 Janus 里。`}</div>
      <div class="caption">{t`已配对的设备`}</div>
      <div class="rows">
        {devices.map((d) => (
          <div class="row device-row" data-device={d.id}>
            <div class="grow">
              <div class="ellipsis">
                {d.name}
                {d.id === me?.id && <span class="secondary">{t`（这台设备）`}</span>}
              </div>
              <div class="caption">{t`配对于 ${ago(d.createdAt ?? 0, now)} · 最近使用 ${ago(d.lastSeenAt ?? 0, now)}`}</div>
            </div>
            {d.id === me?.id && <Button onClick={() => void unpair()}>{t`取消配对`}</Button>}
          </div>
        ))}
      </div>
    </Card>
  );
}
