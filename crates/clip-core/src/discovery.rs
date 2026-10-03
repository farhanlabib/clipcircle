//! Finding circle members and pairing hosts on the local network with mDNS.

use std::collections::HashMap;
use std::net::{IpAddr, SocketAddr};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use anyhow::{bail, Result};
use mdns_sd::{ScopedIp, ServiceDaemon, ServiceEvent, ServiceInfo};

pub const SYNC_SERVICE: &str = "_clipcircle._tcp.local.";
pub const PAIR_SERVICE: &str = "_clipcircle-pair._tcp.local.";

/// Device id -> addresses it was last seen at.
pub type PeerMap = Arc<Mutex<HashMap<String, Vec<SocketAddr>>>>;

/// Announces a service; it stays registered while the daemon lives.
pub fn advertise(
    service: &str,
    instance: &str,
    port: u16,
    props: &[(&str, &str)],
) -> Result<ServiceDaemon> {
    let daemon = ServiceDaemon::new()?;
    let host = format!("{instance}.local.");
    let info = ServiceInfo::new(service, instance, &host, "", port, props)?.enable_addr_auto();
    daemon.register(info)?;
    Ok(daemon)
}

/// Addresses worth connecting to, most likely to work first: home/office LAN
/// addresses, then other IPv4, then VPN-style (CGNAT, e.g. Tailscale) and
/// link-local IPv4, then IPv6. IPv6 link-local addresses are dropped: mDNS
/// reports them without a scope id, so connecting fails with "no route to host".
pub fn dialable_addrs(ips: impl IntoIterator<Item = IpAddr>, port: u16) -> Vec<SocketAddr> {
    let mut addrs: Vec<SocketAddr> = ips
        .into_iter()
        .filter(|ip| match ip {
            IpAddr::V4(_) => true,
            IpAddr::V6(v6) => !v6.is_unicast_link_local(),
        })
        .map(|ip| SocketAddr::new(ip, port))
        .collect();
    addrs.sort_by_key(|a| (rank(a.ip()), *a));
    addrs
}

fn rank(ip: IpAddr) -> u8 {
    match ip {
        IpAddr::V4(v4) if v4.is_private() => 0,
        // 100.64.0.0/10: carrier-grade NAT, used by Tailscale and other VPNs.
        IpAddr::V4(v4) if v4.octets()[0] == 100 && (v4.octets()[1] & 0xc0) == 64 => 2,
        IpAddr::V4(v4) if v4.is_link_local() => 3,
        IpAddr::V4(_) => 1,
        IpAddr::V6(_) => 4,
    }
}

/// Keeps `peers` filled with devices advertising the given circle.
pub fn browse_circle(circle_id: String, self_id: String, peers: PeerMap) -> Result<ServiceDaemon> {
    let daemon = ServiceDaemon::new()?;
    let events = daemon.browse(SYNC_SERVICE)?;
    tokio::spawn(async move {
        let mut by_fullname: HashMap<String, String> = HashMap::new();
        while let Ok(event) = events.recv_async().await {
            match event {
                ServiceEvent::ServiceResolved(info) => {
                    let circle = info.get_property_val_str("circle");
                    let Some(device) = info.get_property_val_str("device") else {
                        continue;
                    };
                    if circle != Some(circle_id.as_str()) || device == self_id {
                        continue;
                    }
                    let addrs = dialable_addrs(
                        info.get_addresses().iter().map(ScopedIp::to_ip_addr),
                        info.get_port(),
                    );
                    // Early resolves can carry only link-local addresses; wait for a usable one.
                    if addrs.is_empty() {
                        continue;
                    }
                    by_fullname.insert(info.get_fullname().to_owned(), device.to_owned());
                    let previous = peers
                        .lock()
                        .unwrap()
                        .insert(device.to_owned(), addrs.clone());
                    // mDNS re-resolves once per address; only log real changes.
                    if previous.as_ref() != Some(&addrs) {
                        tracing::info!(device, ?addrs, "found circle device");
                    } else {
                        tracing::debug!(device, "circle device re-resolved");
                    }
                }
                ServiceEvent::ServiceRemoved(_, fullname) => {
                    if let Some(device) = by_fullname.remove(&fullname) {
                        tracing::info!(device, "circle device left");
                        peers.lock().unwrap().remove(&device);
                    }
                }
                _ => {}
            }
        }
    });
    Ok(daemon)
}

/// A sync service seen on the network.
#[derive(Debug, Clone)]
pub struct Announced {
    pub device: String,
    pub circle: String,
    pub addrs: Vec<SocketAddr>,
}

/// Lists every device announcing the sync service within `wait`, whatever
/// its circle. For diagnostics; the engine uses [`browse_circle`].
pub async fn scan(wait: Duration) -> Result<Vec<Announced>> {
    let daemon = ServiceDaemon::new()?;
    let events = daemon.browse(SYNC_SERVICE)?;
    let mut found: HashMap<String, Announced> = HashMap::new();
    let _ = tokio::time::timeout(wait, async {
        while let Ok(event) = events.recv_async().await {
            if let ServiceEvent::ServiceResolved(info) = event {
                let (Some(device), Some(circle)) = (
                    info.get_property_val_str("device"),
                    info.get_property_val_str("circle"),
                ) else {
                    continue;
                };
                let addrs = dialable_addrs(
                    info.get_addresses().iter().map(ScopedIp::to_ip_addr),
                    info.get_port(),
                );
                let entry = found.entry(device.to_owned()).or_insert(Announced {
                    device: device.to_owned(),
                    circle: circle.to_owned(),
                    addrs: Vec::new(),
                });
                for a in addrs {
                    if !entry.addrs.contains(&a) {
                        entry.addrs.push(a);
                    }
                }
            }
        }
    })
    .await;
    let _ = daemon.shutdown();
    let mut out: Vec<Announced> = found.into_values().collect();
    for a in &mut out {
        a.addrs.sort_by_key(|s| (s.is_ipv6(), *s));
    }
    out.sort_by(|a, b| a.device.cmp(&b.device));
    Ok(out)
}

/// Waits for a device that is currently showing a pairing code.
pub async fn find_pairing_host(timeout: Duration) -> Result<SocketAddr> {
    let daemon = ServiceDaemon::new()?;
    let events = daemon.browse(PAIR_SERVICE)?;
    let found = tokio::time::timeout(timeout, async {
        while let Ok(event) = events.recv_async().await {
            if let ServiceEvent::ServiceResolved(info) = event {
                let addrs = dialable_addrs(
                    info.get_addresses().iter().map(ScopedIp::to_ip_addr),
                    info.get_port(),
                );
                if let Some(addr) = addrs.into_iter().find(|a| a.is_ipv4()) {
                    return Some(addr);
                }
            }
        }
        None
    })
    .await;
    let _ = daemon.shutdown();
    match found {
        Ok(Some(addr)) => Ok(addr),
        _ => bail!("no device is showing a pairing code on this network"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dialable_addrs_drops_ipv6_link_local_and_puts_lan_first() {
        let ips: Vec<IpAddr> = [
            "fe80::1",
            "fd00::5",
            "100.75.104.51",
            "192.168.31.67",
            "169.254.3.4",
            "fe80::abcd",
        ]
        .iter()
        .map(|s| s.parse().unwrap())
        .collect();
        let addrs = dialable_addrs(ips, 47800);
        let got: Vec<String> = addrs.iter().map(|a| a.to_string()).collect();
        assert_eq!(
            got,
            [
                "192.168.31.67:47800",
                "100.75.104.51:47800",
                "169.254.3.4:47800",
                "[fd00::5]:47800"
            ]
        );
    }
}
