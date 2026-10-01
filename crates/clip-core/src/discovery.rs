//! Finding circle members and pairing hosts on the local network with mDNS.

use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use anyhow::{bail, Result};
use mdns_sd::{ServiceDaemon, ServiceEvent, ServiceInfo};

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
                    let Some(device) = info.get_property_val_str("device") else { continue };
                    if circle != Some(circle_id.as_str()) || device == self_id {
                        continue;
                    }
                    let addrs: Vec<SocketAddr> = info
                        .get_addresses()
                        .iter()
                        .map(|ip| SocketAddr::new(*ip, info.get_port()))
                        .collect();
                    tracing::info!(device, ?addrs, "found circle device");
                    by_fullname.insert(info.get_fullname().to_owned(), device.to_owned());
                    peers.lock().unwrap().insert(device.to_owned(), addrs);
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

/// Waits for a device that is currently showing a pairing code.
pub async fn find_pairing_host(timeout: Duration) -> Result<SocketAddr> {
    let daemon = ServiceDaemon::new()?;
    let events = daemon.browse(PAIR_SERVICE)?;
    let found = tokio::time::timeout(timeout, async {
        while let Ok(event) = events.recv_async().await {
            if let ServiceEvent::ServiceResolved(info) = event {
                if let Some(ip) = info.get_addresses().iter().find(|ip| ip.is_ipv4()) {
                    return Some(SocketAddr::new(*ip, info.get_port()));
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
