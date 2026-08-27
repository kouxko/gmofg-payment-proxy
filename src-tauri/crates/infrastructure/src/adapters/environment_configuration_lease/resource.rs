use intercept_proxy_application::{ProtocolPackageRef, ProtocolPackageVersion};
use uuid::Uuid;

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum EnvironmentApplyLeaseResourceKey {
    Listener(Uuid),
    AndroidOwner { profile_id: String, serial: String },
    ExactPackage(ProtocolPackageRef),
}

impl Ord for EnvironmentApplyLeaseResourceKey {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        resource_key_cmp(self, other)
    }
}

impl PartialOrd for EnvironmentApplyLeaseResourceKey {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

pub(in crate::adapters) fn resource_key_cmp(
    left: &EnvironmentApplyLeaseResourceKey,
    right: &EnvironmentApplyLeaseResourceKey,
) -> std::cmp::Ordering {
    use EnvironmentApplyLeaseResourceKey::{AndroidOwner, ExactPackage, Listener};
    let rank = |key: &EnvironmentApplyLeaseResourceKey| match key {
        Listener(_) => 0_u8,
        AndroidOwner { .. } => 1,
        ExactPackage(_) => 2,
    };
    rank(left)
        .cmp(&rank(right))
        .then_with(|| match (left, right) {
            (Listener(left), Listener(right)) => left.cmp(right),
            (
                AndroidOwner {
                    profile_id: left_profile,
                    serial: left_serial,
                },
                AndroidOwner {
                    profile_id: right_profile,
                    serial: right_serial,
                },
            ) => left_profile
                .cmp(right_profile)
                .then_with(|| left_serial.cmp(right_serial)),
            (ExactPackage(left), ExactPackage(right)) => package_cmp(left, right),
            _ => std::cmp::Ordering::Equal,
        })
}

fn package_cmp(left: &ProtocolPackageRef, right: &ProtocolPackageRef) -> std::cmp::Ordering {
    left.id.as_str().cmp(right.id.as_str()).then_with(|| {
        ProtocolPackageVersion::semantic_cmp(&left.version, &right.version)
            .then_with(|| left.version.as_str().cmp(right.version.as_str()))
    })
}

impl EnvironmentApplyLeaseResourceKey {
    #[cfg(test)]
    pub(crate) fn package_ref(&self) -> Option<&ProtocolPackageRef> {
        match self {
            Self::ExactPackage(package) => Some(package),
            _ => None,
        }
    }
}
