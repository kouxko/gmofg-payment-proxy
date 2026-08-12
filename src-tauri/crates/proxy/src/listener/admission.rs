use std::{net::IpAddr, sync::Arc};

use tokio::sync::{OwnedSemaphorePermit, Semaphore, TryAcquireError};

use crate::{ErrorCode, ProxyError, Result};

#[derive(Debug)]
pub(crate) enum AdmissionDecision {
    Admitted(OwnedSemaphorePermit),
    NetworkDenied,
    CapacityExhausted,
}

#[derive(Debug, Clone)]
pub(crate) struct ListenerAdmission {
    networks: Arc<Vec<ClientNetwork>>,
    capacity: ListenerCapacity,
}

#[derive(Debug, Clone)]
pub(crate) struct ListenerCapacity {
    permits: Arc<Semaphore>,
}

impl ListenerCapacity {
    pub(crate) fn new(capacity: usize) -> Result<Self> {
        if capacity == 0 {
            return Err(ProxyError::new(
                ErrorCode::ConfigInvalid,
                "listener connection capacity must be greater than zero",
            ));
        }
        Ok(Self {
            permits: Arc::new(Semaphore::new(capacity)),
        })
    }

    fn try_acquire(&self) -> std::result::Result<OwnedSemaphorePermit, TryAcquireError> {
        Arc::clone(&self.permits).try_acquire_owned()
    }
}

impl ListenerAdmission {
    pub(crate) fn new(
        allowed_client_cidrs: impl IntoIterator<Item = String>,
        capacity: ListenerCapacity,
    ) -> Result<Self> {
        let networks = allowed_client_cidrs
            .into_iter()
            .map(|value| ClientNetwork::parse(&value))
            .collect::<Result<Vec<_>>>()?;
        Ok(Self {
            networks: Arc::new(networks),
            capacity,
        })
    }

    pub(crate) fn admit(&self, peer: IpAddr) -> AdmissionDecision {
        if !peer_is_allowed(peer, &self.networks) {
            return AdmissionDecision::NetworkDenied;
        }
        match self.capacity.try_acquire() {
            Ok(permit) => AdmissionDecision::Admitted(permit),
            Err(TryAcquireError::NoPermits | TryAcquireError::Closed) => {
                AdmissionDecision::CapacityExhausted
            }
        }
    }
}

#[derive(Clone, Copy, Debug)]
struct ClientNetwork {
    address: IpAddr,
    prefix: u8,
}

impl ClientNetwork {
    fn parse(value: &str) -> Result<Self> {
        let (address, prefix) = value.split_once('/').ok_or_else(|| invalid_cidr(value))?;
        let address = address.parse::<IpAddr>().map_err(|_| invalid_cidr(value))?;
        let prefix = prefix.parse::<u8>().map_err(|_| invalid_cidr(value))?;
        let width = if address.is_ipv4() { 32 } else { 128 };
        if prefix > width {
            return Err(invalid_cidr(value));
        }
        Ok(Self { address, prefix })
    }

    fn contains(self, candidate: IpAddr) -> bool {
        match (self.address, canonical_ip(candidate)) {
            (IpAddr::V4(network), IpAddr::V4(candidate)) => {
                masked(u128::from(u32::from(network)), self.prefix, 32)
                    == masked(u128::from(u32::from(candidate)), self.prefix, 32)
            }
            (IpAddr::V6(network), IpAddr::V6(candidate)) => {
                masked(u128::from(network), self.prefix, 128)
                    == masked(u128::from(candidate), self.prefix, 128)
            }
            _ => false,
        }
    }
}

fn peer_is_allowed(peer: IpAddr, networks: &[ClientNetwork]) -> bool {
    let peer = canonical_ip(peer);
    peer.is_loopback()
        || networks.is_empty()
        || networks.iter().any(|network| network.contains(peer))
}

fn canonical_ip(peer: IpAddr) -> IpAddr {
    match peer {
        IpAddr::V6(address) => address.to_ipv4_mapped().map_or(peer, IpAddr::V4),
        IpAddr::V4(_) => peer,
    }
}

fn masked(value: u128, prefix: u8, width: u8) -> u128 {
    if prefix == 0 {
        return 0;
    }
    value & (u128::MAX << (width - prefix))
}

fn invalid_cidr(value: &str) -> ProxyError {
    ProxyError::new(
        ErrorCode::ConfigInvalid,
        format!("invalid listener client CIDR: {value}"),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cidr_is_checked_before_capacity() {
        let admission =
            ListenerAdmission::new(["10.0.0.0/8".to_owned()], ListenerCapacity::new(1).unwrap())
                .unwrap();
        let _permit = match admission.admit("10.1.2.3".parse().unwrap()) {
            AdmissionDecision::Admitted(permit) => permit,
            decision => panic!("expected admitted, got {decision:?}"),
        };
        assert!(matches!(
            admission.admit("192.168.1.2".parse().unwrap()),
            AdmissionDecision::NetworkDenied
        ));
        assert!(matches!(
            admission.admit("10.9.8.7".parse().unwrap()),
            AdmissionDecision::CapacityExhausted
        ));
    }

    #[test]
    fn empty_cidr_list_allows_all_and_mapped_loopback_is_local() {
        let allow_all = ListenerAdmission::new([], ListenerCapacity::new(2).unwrap()).unwrap();
        assert!(matches!(
            allow_all.admit("203.0.113.9".parse().unwrap()),
            AdmissionDecision::Admitted(_)
        ));

        let restricted =
            ListenerAdmission::new(["10.0.0.0/8".to_owned()], ListenerCapacity::new(1).unwrap())
                .unwrap();
        assert!(matches!(
            restricted.admit("::ffff:127.0.0.1".parse().unwrap()),
            AdmissionDecision::Admitted(_)
        ));
    }
}
