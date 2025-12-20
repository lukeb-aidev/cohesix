// Author: Lukas Bower

//! Self-test destination selection for different QEMU networking modes.

use smoltcp::wire::Ipv4Address;

const LOOPBACK: [u8; 4] = [127, 0, 0, 1];

/// Networking environment for the VM.
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum NetMode {
    /// QEMU slirp user networking with hostfwd for host→guest access.
    SlirpHostFwd,
    /// Bridged or tap networking with a reachable peer or gateway.
    Bridged { test_target: [u8; 4] },
}

impl NetMode {
    /// Compile-time default for dev-virt.
    pub const fn default() -> Self {
        Self::SlirpHostFwd
    }

    /// Select a mode using compile-time environment hints.
    pub fn from_env(default_gateway: Option<[u8; 4]>) -> Self {
        match option_env!("COHESIX_NET_MODE") {
            Some("bridged") => {
                if let Some(addr) = env_ip("COHESIX_NET_TEST_DST").or(default_gateway) {
                    NetMode::Bridged { test_target: addr }
                } else {
                    NetMode::default()
                }
            }
            _ => NetMode::default(),
        }
    }

    /// Return destinations for UDP echo and TCP smoke tests.
    pub fn destinations(&self) -> NetTestDestinations {
        match self {
            NetMode::SlirpHostFwd => NetTestDestinations {
                udp_echo: Ipv4Address::from(LOOPBACK),
                tcp_smoke: Ipv4Address::from(LOOPBACK),
                outbound_allowed: false,
            },
            NetMode::Bridged { test_target } => NetTestDestinations {
                udp_echo: Ipv4Address::from(*test_target),
                tcp_smoke: Ipv4Address::from(*test_target),
                outbound_allowed: true,
            },
        }
    }
}

/// Destinations used by self-tests.
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub struct NetTestDestinations {
    pub udp_echo: Ipv4Address,
    pub tcp_smoke: Ipv4Address,
    pub outbound_allowed: bool,
}

fn env_ip(key: &str) -> Option<[u8; 4]> {
    option_env!(key).and_then(parse_ipv4)
}

fn parse_ipv4(addr: &str) -> Option<[u8; 4]> {
    let mut parts = [0u8; 4];
    let mut idx = 0;
    for part in addr.split('.') {
        if idx >= 4 {
            return None;
        }
        parts[idx] = part.parse().ok()?;
        idx += 1;
    }
    if idx == 4 {
        Some(parts)
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_mode_prefers_slirp() {
        let mode = NetMode::default();
        let dest = mode.destinations();
        assert_eq!(mode, NetMode::SlirpHostFwd);
        assert_eq!(dest.udp_echo, Ipv4Address::from(LOOPBACK));
        assert!(!dest.outbound_allowed);
    }

    #[test]
    fn bridged_mode_uses_gateway() {
        let mode = NetMode::Bridged {
            test_target: [10, 0, 2, 2],
        };
        let dest = mode.destinations();
        assert_eq!(dest.udp_echo, Ipv4Address::new(10, 0, 2, 2));
        assert!(dest.outbound_allowed);
    }
}
