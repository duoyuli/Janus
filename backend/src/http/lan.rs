//! The listener on the local network and the tailnet, while the person lets phones reach Janus. It
//! listens on every interface, IPv4 and IPv6 (`[::]`, dual-stack; `0.0.0.0` where there is no
//! IPv6), on Janus's own port, beside the listener on `127.0.0.1`: the system hands a connection to
//! the most specific address bound, so this Mac's own requests still reach the loopback listener,
//! and everything arriving here is from the network and is asked for a device cookie
//! (`devices::guard`), and refused when it comes from neither a private network nor the tailnet.
//! Listening on every interface follows a new Wi-Fi or a tailnet coming up with nothing to redo.
//! Turning the switch off stops it accepting, closes its idle connections and ends its open
//! streams.
// @project-doc docs/operations/20261031-security.md#network_exposure

use super::{Via, router};
use crate::orchestrator::{App, Change};
use std::net::SocketAddr;
use std::sync::Arc;

/// Start or stop the listener to match the switch. A port someone else holds on the network side
/// is kept as the reason it does not listen.
pub fn apply(app: &Arc<App>) {
    let want = app.lan_enabled();
    {
        let mut lan = app.devices.lan.lock().unwrap();
        if want && lan.stop.is_none() {
            match bind(app.port()) {
                Ok(listener) => {
                    let (stop, mut stopped) = tokio::sync::watch::channel(false);
                    lan.stop = Some(stop);
                    lan.error = None;
                    let service = router(app.clone(), Via::Lan).into_make_service_with_connect_info::<SocketAddr>();
                    tokio::spawn(async move {
                        let shutdown = async move {
                            let _ = stopped.wait_for(|s| *s).await;
                        };
                        if let Err(err) = axum::serve(listener, service).with_graceful_shutdown(shutdown).await {
                            crate::config::log(format!("janus: 局域网监听停止：{err:#}"));
                        }
                    });
                }
                Err(err) => lan.error = Some(err),
            }
        } else if !want {
            if let Some(stop) = lan.stop.take() {
                let _ = stop.send(true);
            }
            lan.error = None;
        }
    }
    app.devices.epoch.send_modify(|e| *e += 1);
    let _ = app.changes.send(Change::Devices);
}

fn bind(port: u16) -> Result<tokio::net::TcpListener, String> {
    let taken = |e: std::io::Error| format!("没能在局域网上监听端口 {port}：{e}");
    let listener = std::net::TcpListener::bind(("::", port))
        .or_else(|_| std::net::TcpListener::bind(("0.0.0.0", port)))
        .map_err(taken)?;
    listener.set_nonblocking(true).map_err(taken)?;
    tokio::net::TcpListener::from_std(listener).map_err(taken)
}
