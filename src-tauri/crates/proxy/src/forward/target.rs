//! 正向代理目标地址与 authority 的纯解析逻辑。

use http::Uri;

use super::config_error;
use crate::Result;

#[derive(Debug)]
pub(super) struct HttpTarget {
    pub(super) connect_authority: String,
    pub(super) host_header: String,
}

pub(super) fn absolute_http_target(uri: &Uri) -> Result<HttpTarget> {
    if uri.scheme_str() != Some("http") {
        return Err(config_error(
            "non-CONNECT forward proxy requests require an absolute http URI",
        ));
    }
    let authority = uri
        .authority()
        .ok_or_else(|| config_error("absolute request URI is missing authority"))?;
    if authority.as_str().contains('@') {
        return Err(config_error(
            "forward proxy target must not contain userinfo",
        ));
    }
    let host = uri
        .host()
        .ok_or_else(|| config_error("target host is missing"))?;
    let port = uri.port_u16().unwrap_or(80);
    if port == 0 {
        return Err(config_error("target port must be greater than zero"));
    }
    let host = unbracket_host(host);
    let connect_authority = format_authority(host, port);
    let host_header = if port == 80 {
        format_host(host)
    } else {
        connect_authority.clone()
    };
    Ok(HttpTarget {
        connect_authority,
        host_header,
    })
}

/// 将正向代理 absolute-form request-target 转为上游需要的 origin-form。
pub fn absolute_uri_to_origin_form(uri: &Uri) -> Result<Uri> {
    if uri.scheme_str() != Some("http") || uri.authority().is_none() {
        return Err(config_error("request-target is not an absolute HTTP URI"));
    }
    let origin = uri
        .path_and_query()
        .map_or("/", http::uri::PathAndQuery::as_str);
    origin
        .parse()
        .map_err(|error| config_error(format!("invalid origin-form request-target: {error}")))
}

/// 精确主机/IP 或 `*.example.test` 后缀匹配。通配符不匹配裸根域，且边界必须是 `.`。
pub(crate) fn authority_is_allowed(host: &str, patterns: &[String]) -> bool {
    let host = host.trim_end_matches('.').to_ascii_lowercase();
    patterns.iter().any(|pattern| {
        let pattern = pattern.trim_end_matches('.').to_ascii_lowercase();
        if let Some(suffix) = pattern.strip_prefix("*.") {
            host.len() > suffix.len()
                && host.ends_with(suffix)
                && host.as_bytes().get(host.len() - suffix.len() - 1) == Some(&b'.')
        } else {
            host == pattern
        }
    })
}

fn unbracket_host(host: &str) -> &str {
    host.strip_prefix('[')
        .and_then(|value| value.strip_suffix(']'))
        .unwrap_or(host)
}

fn format_host(host: &str) -> String {
    if host.contains(':') {
        format!("[{host}]")
    } else {
        host.to_owned()
    }
}

fn format_authority(host: &str, port: u16) -> String {
    format!("{}:{port}", format_host(host))
}
