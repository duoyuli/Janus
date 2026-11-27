//! Browsers and phones the person paired with Janus, and the switch that lets phones on the local
//! network reach it. The app on this Mac needs neither: it is the client every other one is paired
//! from. A device is paired with a one-time code the app shows (a QR code); claiming the code gives
//! the device a token in a cookie, and every request it makes carries that token. Janus keeps only
//! the token's SHA-256, in an owner-only file per device, so neither the database nor a backup of
//! it holds anything a request could be made with.
// @project-doc docs/operations/20261031-security.md#paired_devices

use crate::db::{ConfigKind, Setting};
use crate::orchestrator::{App, Change};
use crate::util::now_ms;
use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::net::{IpAddr, Ipv4Addr, UdpSocket};
use std::sync::Mutex;

/// The cookie that carries a paired device's token.
pub const COOKIE: &str = "janus_device";
/// How long a pairing code stays good.
pub const CODE_TTL_MS: i64 = 5 * 60_000;
/// Wrong claims tolerated before every open code is voided and a new one has to be shown.
pub const MAX_FAILURES: u32 = 5;
/// Codes open at once; showing one more drops the oldest.
const MAX_CODES: usize = 8;
/// How often a device's last use is written down; in between it is kept in memory.
const SEEN_EVERY_MS: i64 = 10 * 60_000;
/// The letters a code is made of: no `0`/`o`, `1`/`l`/`i`, so it can be typed from a screen.
const CODE_LETTERS: &[u8] = b"abcdefghjkmnpqrstuvwxyz23456789";
/// 10 letters of 31: about 50 bits, and at most `MAX_FAILURES` guesses per shown code.
const CODE_LEN: usize = 10;

/// One paired device, as clients see it.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Device {
    pub id: String,
    pub name: String,
    pub created_at: i64,
    pub last_seen_at: i64,
}

/// A device's file: what clients see and the hash of its token.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct DeviceFile {
    #[serde(flatten)]
    device: Device,
    token_sha256: String,
}

#[derive(Default)]
struct Inner {
    /// Read from the files on first use.
    loaded: bool,
    /// Paired devices by the hash of their token.
    devices: HashMap<String, DeviceFile>,
    /// Open pairing codes and when each expires.
    codes: Vec<(String, i64)>,
    /// Wrong claims since a code was last shown.
    failures: u32,
}

/// What Janus holds about paired devices while it runs; `epoch` moves on each revocation and each
/// turn of the network switch, so open streams can check whether they may go on.
pub struct DeviceState {
    inner: Mutex<Inner>,
    pub epoch: tokio::sync::watch::Sender<u64>,
    /// The network listener, while it runs (`http::lan`).
    pub lan: Mutex<LanListener>,
}

impl Default for DeviceState {
    fn default() -> Self {
        DeviceState { inner: Mutex::default(), epoch: tokio::sync::watch::channel(0).0, lan: Mutex::default() }
    }
}

/// The listener on the local network, as `http::lan` keeps it.
#[derive(Default)]
pub struct LanListener {
    /// Tells the running listener to stop and close its connections.
    pub stop: Option<tokio::sync::watch::Sender<bool>>,
    /// Why the last start failed (the port taken on the network side).
    pub error: Option<String>,
}

/// Whether phones on the local network may reach Janus: one row; none means off.
#[derive(Serialize, Deserialize)]
struct LanAccess {
    enabled: bool,
}

impl Setting for LanAccess {
    const KIND: ConfigKind = ConfigKind::LanAccess;
    fn id(&self) -> &str {
        "app"
    }
}

/// Why a claim was refused.
#[derive(Debug, PartialEq)]
pub enum ClaimRefused {
    /// The body is not a claim.
    Invalid(String),
    /// The code is wrong, used or old, or too many wrong ones voided it.
    BadCode,
}

/// `n` bytes from the system's random source; without it there is no secret worth the name.
fn random_bytes<const N: usize>() -> Result<[u8; N]> {
    use std::io::Read;
    let mut b = [0u8; N];
    std::fs::File::open("/dev/urandom")
        .and_then(|mut f| f.read_exact(&mut b))
        .context("读取系统随机数失败，无法配对设备")?;
    Ok(b)
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

fn hash(token: &str) -> String {
    hex(&Sha256::digest(token.as_bytes()))
}

/// The local network addresses of this Mac, the way packets to the local network, the Internet and
/// a Tailscale network would leave it. Connecting a UDP socket sends nothing; it only asks the
/// system which address it would use.
pub fn lan_addresses() -> Vec<IpAddr> {
    let probes = [
        Ipv4Addr::new(192, 168, 255, 255),
        Ipv4Addr::new(10, 255, 255, 255),
        Ipv4Addr::new(172, 31, 255, 255),
        Ipv4Addr::new(8, 8, 8, 8),
        // Tailscale's own address range.
        Ipv4Addr::new(100, 100, 100, 100),
    ];
    let mut out: Vec<IpAddr> = vec![];
    for p in probes {
        let Ok(s) = UdpSocket::bind((Ipv4Addr::UNSPECIFIED, 0)) else { continue };
        if s.connect((p, 9)).is_err() {
            continue;
        }
        if let Ok(a) = s.local_addr() {
            let ip = a.ip();
            if !ip.is_loopback() && !ip.is_unspecified() && !out.contains(&ip) {
                out.push(ip);
            }
        }
    }
    out
}

impl App {
    fn with_devices<R>(&self, f: impl FnOnce(&mut Inner) -> R) -> R {
        let mut inner = self.devices.inner.lock().unwrap();
        if !inner.loaded {
            inner.loaded = true;
            let dir = self.cfg.layout.devices();
            for entry in std::fs::read_dir(&dir).into_iter().flatten().flatten() {
                let read = std::fs::read_to_string(entry.path()).ok();
                match read.and_then(|t| serde_json::from_str::<DeviceFile>(&t).ok()) {
                    Some(d) => {
                        inner.devices.insert(d.token_sha256.clone(), d);
                    }
                    None => crate::config::log(format!("janus: 读不懂设备文件 {}", entry.path().display())),
                }
            }
        }
        f(&mut inner)
    }

    fn write_device(&self, d: &DeviceFile) -> Result<()> {
        let dir = self.cfg.layout.devices();
        std::fs::create_dir_all(&dir)?;
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o700))?;
        // The temporary file `write_whole` writes through is owner-only, and so is the file.
        crate::util::write_whole(&self.cfg.layout.device(&d.device.id), serde_json::to_string_pretty(d)?.as_bytes())
    }

    /// A new pairing code, good once for `CODE_TTL_MS`, and the addresses a device can open it at:
    /// the local network's while the switch is on, then this Mac's own.
    pub fn new_pairing(&self) -> Result<Value> {
        let bytes = random_bytes::<CODE_LEN>()?;
        let code: String = bytes.iter().map(|b| CODE_LETTERS[*b as usize % CODE_LETTERS.len()] as char).collect();
        let expires = now_ms() + CODE_TTL_MS;
        self.with_devices(|inner| {
            let now = now_ms();
            inner.codes.retain(|(_, e)| *e > now);
            if inner.codes.len() >= MAX_CODES {
                inner.codes.remove(0);
            }
            inner.codes.push((code.clone(), expires));
            inner.failures = 0;
        });
        let urls: Vec<String> = self.base_urls().iter().map(|b| format!("{b}/pair/{code}")).collect();
        Ok(json!({ "code": code, "expiresAt": expires, "urls": urls }))
    }

    /// Where clients reach Janus: each local network address while the listener runs, then this
    /// Mac's own address.
    pub fn base_urls(&self) -> Vec<String> {
        let port = self.port();
        let mut out = vec![];
        if self.lan_listening() {
            out.extend(lan_addresses().into_iter().map(|ip| format!("http://{ip}:{port}")));
        }
        out.push(format!("http://127.0.0.1:{port}"));
        out
    }

    /// The port Janus listens on, on this Mac and (with the switch on) on the network.
    pub fn port(&self) -> u16 {
        self.cfg.addr.rsplit(':').next().and_then(|p| p.parse().ok()).unwrap_or(7878)
    }

    /// Pair a device with `code`: a new device and the token its requests carry from now on.
    pub fn claim_device(&self, body: &Value) -> Result<(Device, String), ClaimRefused> {
        let code = body["code"].as_str().map(str::trim).unwrap_or("");
        if code.is_empty() {
            return Err(ClaimRefused::Invalid("缺少配对码 code".into()));
        }
        let name: String = body["name"].as_str().map(str::trim).unwrap_or("").chars().take(60).collect();
        let name = if name.is_empty() { "浏览器".to_string() } else { name };
        let now = now_ms();
        let good = self.with_devices(|inner| {
            inner.codes.retain(|(_, e)| *e > now);
            match inner.codes.iter().position(|(c, _)| c == code) {
                Some(i) => {
                    inner.codes.remove(i);
                    true
                }
                None => {
                    inner.failures += 1;
                    if inner.failures >= MAX_FAILURES {
                        // Too many guesses: no open code is worth keeping.
                        inner.codes.clear();
                    }
                    false
                }
            }
        });
        if !good {
            return Err(ClaimRefused::BadCode);
        }
        let token = random_bytes::<32>().map(|b| hex(&b)).map_err(|e| ClaimRefused::Invalid(format!("{e:#}")))?;
        let device = Device { id: crate::util::new_id("d"), name, created_at: now, last_seen_at: now };
        let file = DeviceFile { device: device.clone(), token_sha256: hash(&token) };
        self.write_device(&file).map_err(|e| ClaimRefused::Invalid(format!("保存设备失败：{e:#}")))?;
        self.with_devices(|inner| inner.devices.insert(file.token_sha256.clone(), file));
        let _ = self.changes.send(Change::Devices);
        Ok((device, token))
    }

    /// The device `token` belongs to, now marked as seen; none for a token that is not one.
    pub fn device_by_token(&self, token: &str) -> Option<Device> {
        if token.is_empty() {
            return None;
        }
        let now = now_ms();
        let (device, write) = self.with_devices(|inner| {
            let d = inner.devices.get_mut(&hash(token))?;
            let write = (now - d.device.last_seen_at >= SEEN_EVERY_MS).then(|| d.clone());
            d.device.last_seen_at = now;
            Some((d.device.clone(), write))
        })?;
        if let Some(mut d) = write {
            d.device.last_seen_at = now;
            if let Err(err) = self.write_device(&d) {
                crate::config::log(format!("janus: 记不下设备 {} 的使用时间：{err:#}", d.device.id));
            }
        }
        Some(device)
    }

    /// Whether device `id` is still paired.
    pub fn device_paired(&self, id: &str) -> bool {
        self.with_devices(|inner| inner.devices.values().any(|d| d.device.id == id))
    }

    /// Every paired device, the first paired first.
    pub fn devices(&self) -> Vec<Device> {
        let mut out: Vec<Device> =
            self.with_devices(|inner| inner.devices.values().map(|d| d.device.clone()).collect());
        out.sort_by(|a, b| (a.created_at, &a.id).cmp(&(b.created_at, &b.id)));
        out
    }

    /// Unpair device `id`: its token stops working at once, and its open streams end.
    pub fn revoke_device(&self, id: &str) -> Result<()> {
        let removed = self.with_devices(|inner| {
            let key = inner.devices.iter().find(|(_, d)| d.device.id == id).map(|(k, _)| k.clone());
            key.and_then(|k| inner.devices.remove(&k))
        });
        if removed.is_none() {
            bail!("没有这台设备");
        }
        std::fs::remove_file(self.cfg.layout.device(id)).or_else(|e| match e.kind() {
            std::io::ErrorKind::NotFound => Ok(()),
            _ => Err(e),
        })?;
        self.devices.epoch.send_modify(|e| *e += 1);
        let _ = self.changes.send(Change::Devices);
        Ok(())
    }

    /// Whether the person let phones on the local network reach Janus.
    pub fn lan_enabled(&self) -> bool {
        self.db.get::<LanAccess>("app").ok().flatten().is_some_and(|s| s.enabled)
    }

    /// Whether the listener on the local network runs now.
    pub fn lan_listening(&self) -> bool {
        self.devices.lan.lock().unwrap().stop.is_some()
    }

    /// Keep the switch; the caller starts or stops the listener (`http::lan::apply`).
    pub fn save_lan_access(&self, body: &Value) -> Result<()> {
        let enabled = body["enabled"].as_bool().context("enabled 必须是 true 或 false")?;
        self.db.put(&LanAccess { enabled }, now_ms())?;
        Ok(())
    }

    /// The switch, whether the listener runs, why it does not, and where it is reached.
    pub fn lan_access(&self) -> Value {
        let error = self.devices.lan.lock().unwrap().error.clone();
        json!({
            "enabled": self.lan_enabled(),
            "listening": self.lan_listening(),
            "port": self.port(),
            "urls": self.base_urls(),
            "error": error,
        })
    }
}

#[cfg(test)]
impl App {
    /// Let a test serve `http::router(.., Via::Lan)` on a port of its own, as though the listener
    /// on the network ran: binding every interface beside the loopback one works on macOS only.
    pub fn pretend_lan_listening(&self) {
        self.devices.lan.lock().unwrap().stop = Some(tokio::sync::watch::channel(false).0);
    }

    /// Make every open code `ms` older.
    pub fn age_codes(&self, ms: i64) {
        self.with_devices(|inner| inner.codes.iter_mut().for_each(|(_, e)| *e -= ms));
    }
}

#[cfg(test)]
mod tests;
