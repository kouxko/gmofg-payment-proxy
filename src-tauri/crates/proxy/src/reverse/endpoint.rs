use super::{Duration, ErrorCode, ProxyError, Result, SocketAddr, Uri};

#[derive(Clone, Debug)]
pub(super) struct UpstreamEndpoint {
    pub(super) address: SocketAddr,
    pub(super) host: String,
    pub(super) host_header: String,
    pub(super) uses_tls: bool,
}

impl UpstreamEndpoint {
    pub(super) async fn parse(value: &str, timeout: Duration) -> Result<Self> {
        let uri = value.parse::<Uri>().map_err(|error| {
            ProxyError::new(
                ErrorCode::ConfigInvalid,
                format!("invalid reverse upstream origin: {error}"),
            )
        })?;
        let scheme = uri.scheme_str().ok_or_else(|| {
            ProxyError::new(
                ErrorCode::ConfigInvalid,
                "reverse upstream scheme is missing",
            )
        })?;
        let uses_tls = match scheme {
            "http" => false,
            "https" => true,
            _ => {
                return Err(ProxyError::new(
                    ErrorCode::ConfigInvalid,
                    "reverse upstream must use http or https",
                ));
            }
        };
        if uri
            .path_and_query()
            .is_some_and(|value| value.as_str() != "/")
            || value.contains('#')
            || uri
                .authority()
                .is_some_and(|authority| authority.as_str().contains('@'))
        {
            return Err(ProxyError::new(
                ErrorCode::ConfigInvalid,
                "reverse upstream must be an origin without path, query, fragment, or userinfo",
            ));
        }
        let uri_host = uri.host().ok_or_else(|| {
            ProxyError::new(ErrorCode::ConfigInvalid, "reverse upstream host is missing")
        })?;
        let host = uri_host.trim_matches(['[', ']']).to_owned();
        let port = uri.port_u16().unwrap_or(if uses_tls { 443 } else { 80 });
        let address = tokio::time::timeout(timeout, tokio::net::lookup_host((host.as_str(), port)))
            .await
            .map_err(|_| {
                ProxyError::new(
                    ErrorCode::UpstreamConnectTimeout,
                    "reverse upstream DNS resolution timed out",
                )
            })?
            .map_err(|error| {
                ProxyError::new(
                    ErrorCode::ConfigInvalid,
                    format!("cannot resolve reverse upstream: {error}"),
                )
            })?
            .next()
            .ok_or_else(|| {
                ProxyError::new(
                    ErrorCode::ConfigInvalid,
                    "reverse upstream resolved to no addresses",
                )
            })?;
        Ok(Self {
            address,
            host: host.clone(),
            host_header: if (uses_tls && port == 443) || (!uses_tls && port == 80) {
                if host.contains(':') {
                    format!("[{host}]")
                } else {
                    host
                }
            } else if host.contains(':') {
                format!("[{host}]:{port}")
            } else {
                format!("{host}:{port}")
            },
            uses_tls,
        })
    }
}
