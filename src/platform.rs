//! Small, synchronous Windows FFI boundary. Every pool worker owns its handle;
//! stack buffers remain alive until IcmpSendEcho2Ex returns. No async buffer lifetime.
use crate::model::{Network, Outcome};
use anyhow::{Result, bail};
use std::{
    net::{Ipv4Addr, ToSocketAddrs},
    ptr,
};
use windows_sys::Win32::{
    Foundation::{GetLastError, HANDLE, INVALID_HANDLE_VALUE},
    NetworkManagement::IpHelper::*,
    Networking::WinSock::*,
};

fn native_ip(ip: Ipv4Addr) -> u32 {
    u32::from_ne_bytes(ip.octets())
}
fn ip_from_native(ip: u32) -> Ipv4Addr {
    Ipv4Addr::from(ip.to_ne_bytes())
}
fn sockaddr(ip: Ipv4Addr) -> SOCKADDR_INET {
    let mut address = SOCKADDR_INET::default();
    address.Ipv4 = SOCKADDR_IN {
        sin_family: AF_INET,
        sin_addr: IN_ADDR {
            S_un: IN_ADDR_0 {
                S_addr: native_ip(ip),
            },
        },
        ..Default::default()
    };
    address
}
pub fn resolve_google() -> Result<Ipv4Addr> {
    ("google.com", 0)
        .to_socket_addrs()?
        .find_map(|s| match s.ip() {
            std::net::IpAddr::V4(a) => Some(a),
            _ => None,
        })
        .ok_or_else(|| anyhow::anyhow!("No IPv4 address returned for google.com"))
}
pub fn route(destination: Ipv4Addr) -> Result<Network> {
    let mut row = MIB_IPFORWARD_ROW2::default();
    let mut source = SOCKADDR_INET::default();
    // SAFETY: all structures are initialized and valid for the synchronous API call.
    let code = unsafe {
        GetBestRoute2(
            ptr::null(),
            0,
            ptr::null(),
            &sockaddr(destination),
            0,
            &mut row,
            &mut source,
        )
    };
    if code != 0 {
        bail!("No IPv4 route (Windows {code})");
    }
    let mut adapter = MIB_IF_ROW2 {
        InterfaceIndex: row.InterfaceIndex,
        ..Default::default()
    };
    // SAFETY: owned output structure; source/next-hop are IPv4 for this AF_INET query.
    unsafe {
        GetIfEntry2(&mut adapter);
    }
    let end = adapter
        .Alias
        .iter()
        .position(|v| *v == 0)
        .unwrap_or(adapter.Alias.len());
    Ok(Network {
        destination,
        source: unsafe { ip_from_native(source.Ipv4.sin_addr.S_un.S_addr) },
        interface_index: row.InterfaceIndex,
        adapter: String::from_utf16_lossy(&adapter.Alias[..end]),
        next_hop: unsafe { ip_from_native(row.NextHop.Ipv4.sin_addr.S_un.S_addr) },
    })
}
pub fn gateway() -> Result<Ipv4Addr> {
    let mut table = ptr::null_mut();
    // SAFETY: Windows allocates the table; it is freed exactly once after all reads.
    let status = unsafe { GetIpForwardTable2(AF_INET, &mut table) };
    if status != 0 || table.is_null() {
        bail!("Cannot read IPv4 routes (Windows {status})");
    }
    let selected = unsafe {
        let rows = std::slice::from_raw_parts(
            ptr::addr_of!((*table).Table).cast::<MIB_IPFORWARD_ROW2>(),
            (*table).NumEntries as usize,
        );
        let mut candidates = Vec::new();
        for row in rows
            .iter()
            .filter(|r| r.DestinationPrefix.PrefixLength == 0 && !r.Loopback)
        {
            let mut iface = MIB_IPINTERFACE_ROW {
                Family: AF_INET,
                InterfaceIndex: row.InterfaceIndex,
                ..Default::default()
            };
            if GetIpInterfaceEntry(&mut iface) == 0
                && iface.Connected
                && !iface.DisableDefaultRoutes
            {
                let ip = ip_from_native(row.NextHop.Ipv4.sin_addr.S_un.S_addr);
                candidates.push((
                    row.Metric as u64 + iface.Metric as u64,
                    row.InterfaceIndex,
                    ip,
                ));
            }
        }
        candidates.sort_unstable();
        let selected = candidates.first().copied();
        FreeMibTable(table.cast());
        selected
    };
    match selected {
        Some((_, _, ip)) if !ip.is_unspecified() => Ok(ip),
        Some(_) => bail!("Selected default route is on-link/tunnel; no pingable gateway"),
        None => bail!("No connected IPv4 default route"),
    }
}

pub struct Icmp {
    handle: HANDLE,
}
impl Icmp {
    pub fn new() -> Result<Self> {
        // SAFETY: no inputs, returned handle is owned by this object/thread.
        let handle = unsafe { IcmpCreateFile() };
        if handle == INVALID_HANDLE_VALUE || handle.is_null() {
            bail!("ICMP handle: {}", unsafe { GetLastError() });
        }
        Ok(Self { handle })
    }
    pub fn probe(
        &self,
        network: &Network,
        ttl: u8,
        timeout: u32,
    ) -> (Outcome, u32, Option<Ipv4Addr>, Option<u32>) {
        let payload = *b"WANanalyzer network measurement!!";
        let options = IP_OPTION_INFORMATION {
            Ttl: ttl,
            ..Default::default()
        };
        // usize storage ensures alignment, and accommodates reply + payload + ICMP error + IO block.
        let mut buffer = [0usize; 128];
        // SAFETY: synchronous operation (null event/APC); stack data survives until return.
        let count = unsafe {
            IcmpSendEcho2Ex(
                self.handle,
                ptr::null_mut(),
                None,
                ptr::null(),
                native_ip(network.source),
                native_ip(network.destination),
                payload.as_ptr().cast(),
                payload.len() as u16,
                &options,
                buffer.as_mut_ptr().cast(),
                std::mem::size_of_val(&buffer) as u32,
                timeout,
            )
        };
        if count == 0 {
            let code = unsafe { GetLastError() };
            return (
                if code == IP_REQ_TIMED_OUT {
                    Outcome::Timeout
                } else {
                    Outcome::LocalError
                },
                code,
                None,
                None,
            );
        }
        // SAFETY: a successful call contains the reply prefix (Address, Status, RTT).
        // Those first three u32 fields have the same layout in ICMP_ECHO_REPLY and REPLY32.
        // We deliberately do not dereference returned data/options pointers.
        let reply = unsafe { &*buffer.as_ptr().cast::<ICMP_ECHO_REPLY>() };
        let responder = ip_from_native(reply.Address);
        let outcome = match reply.Status {
            0 => Outcome::Reply,
            IP_TTL_EXPIRED_TRANSIT => Outcome::Hop,
            IP_REQ_TIMED_OUT => Outcome::Timeout,
            _ if !responder.is_unspecified() => Outcome::RemoteError,
            _ => Outcome::LocalError,
        };
        (
            outcome,
            reply.Status,
            outcome.is_response().then_some(responder),
            outcome.is_response().then_some(reply.RoundTripTime),
        )
    }
}
impl Drop for Icmp {
    fn drop(&mut self) {
        unsafe {
            IcmpCloseHandle(self.handle);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn address_layout_round_trips() {
        for ip in [Ipv4Addr::new(8, 8, 8, 8), Ipv4Addr::new(192, 168, 24, 1)] {
            assert_eq!(ip_from_native(native_ip(ip)), ip);
        }
    }
    #[test]
    fn real_loopback_echo_and_ttl_layout() {
        let icmp = Icmp::new().unwrap();
        let n = route(Ipv4Addr::LOCALHOST).unwrap();
        for ttl in [1, 30, 128] {
            let r = icmp.probe(&n, ttl, 1000);
            assert_eq!(r.0, Outcome::Reply, "loopback TTL {ttl}: {r:?}");
            assert_eq!(r.2, Some(Ipv4Addr::LOCALHOST));
        }
    }
}
