use std::net::IpAddr;

use url::Url;

/// Schemes, die der Reader grundsätzlich laden darf.
pub fn scheme_allowed(url: &Url) -> bool {
    matches!(url.scheme(), "http" | "https")
}

/// Literal eingetragene Hosts, die niemals automatisch erreicht werden dürfen.
pub fn host_literal_blocked(url: &Url) -> bool {
    let Some(host) = url.host_str() else {
        return true;
    };
    if host.eq_ignore_ascii_case("localhost") || host.ends_with(".localhost") {
        return true;
    }
    match host.parse::<IpAddr>() {
        Ok(ip) => !public_ip(ip),
        Err(_) => false,
    }
}

pub fn public_ip(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => {
            !(v4.is_loopback()
                || v4.is_private()
                || v4.is_link_local()
                || v4.is_broadcast()
                || v4.is_documentation()
                || v4.is_unspecified()
                || v4.octets()[0] == 0
                || v4.octets()[0] >= 240
                // Carrier-Grade NAT
                || (v4.octets()[0] == 100 && (64..128).contains(&v4.octets()[1]))
                // Benchmarking
                || (v4.octets()[0] == 198 && (18..20).contains(&v4.octets()[1])))
        }
        IpAddr::V6(v6) => {
            let seg = v6.segments();
            !(v6.is_loopback()
                || v6.is_unspecified()
                // Unique local
                || (seg[0] & 0xfe00) == 0xfc00
                // Link local
                || (seg[0] & 0xffc0) == 0xfe80
                // Multicast
                || (seg[0] & 0xff00) == 0xff00
                // IPv4-mapped: dieselben Regeln wie IPv4
                || v6.to_ipv4_mapped().map(|v| !public_ip(IpAddr::V4(v))).unwrap_or(false)
                || v6.to_ipv4().map(|v| !public_ip(IpAddr::V4(v))).unwrap_or(false))
        }
    }
}

/// Löst den Host auf und prüft **jede** zurückgegebene Adresse.
/// Absichtlich blockierend; die Aufrufer stehen in eigenen Worker-Threads.
pub fn host_resolves_public(host: &str) -> bool {
    use std::net::ToSocketAddrs;
    let Ok(mut addrs) = (host, 0u16).to_socket_addrs() else {
        return false;
    };
    let mut any = false;
    for addr in addrs.by_ref() {
        any = true;
        if !public_ip(addr.ip()) {
            return false;
        }
    }
    any
}

#[derive(Debug, thiserror::Error)]
pub enum PolicyError {
    #[error("unzulässiges Schema {0}")]
    Scheme(String),
    #[error("Zielhost nicht erreichbar: {0}")]
    HostBlocked(String),
    #[error("Auflösung fehlgeschlagen: {0}")]
    Resolve(String),
}

/// Prüft eine URL vor jedem Abruf und vor jeder Weiterleitung.
///
/// `trusted_origins` erlaubt bewusst hinzugefügte Intranet-Feeds für exakt
/// dieselbe Origin (Schema, Host, Port); andere Ziele bleiben gesperrt.
pub fn check_url(raw: &str, trusted_origins: &[String]) -> Result<Url, PolicyError> {
    let url = Url::parse(raw).map_err(|_| PolicyError::Resolve(raw.to_string()))?;
    if !scheme_allowed(&url) {
        return Err(PolicyError::Scheme(url.scheme().to_string()));
    }
    let origin = origin_of(&url);
    if trusted_origins.iter().any(|o| *o == origin) {
        return Ok(url);
    }
    if host_literal_blocked(&url) {
        return Err(PolicyError::HostBlocked(
            url.host_str().unwrap_or("?").to_string(),
        ));
    }
    let host = url.host_str().unwrap_or_default();
    if host.parse::<IpAddr>().is_err() && !host_resolves_public(host) {
        return Err(PolicyError::HostBlocked(host.to_string()));
    }
    Ok(url)
}

pub fn origin_of(url: &Url) -> String {
    let host = url.host_str().unwrap_or_default();
    match url.port() {
        Some(port) => format!("{}://{}:{}", url.scheme(), host, port),
        None => format!("{}://{}", url.scheme(), host),
    }
}

/// Löst im Client auf und gibt **nur** policy-konforme Adressen weiter. Damit kann
/// eine abweichende zweite DNS-Antwort die Vorprüfung nicht umgehen: was hier
/// herausfällt, ist die einzige Menge, mit der überhaupt verbunden wird.
pub struct PolicyResolver {
    trusted: Vec<String>,
}

impl PolicyResolver {
    pub fn new(trusted: Vec<String>) -> Self {
        Self { trusted }
    }

    fn host_trusted(&self, host: &str) -> bool {
        self.trusted
            .iter()
            .filter_map(|o| Url::parse(o).ok())
            .any(|u| {
                u.host_str()
                    .map(|h| h.eq_ignore_ascii_case(host))
                    .unwrap_or(false)
            })
    }
}

impl reqwest::dns::Resolve for PolicyResolver {
    fn resolve(&self, name: reqwest::dns::Name) -> reqwest::dns::Resolving {
        let host = name.as_str().to_string();
        let trusted = self.host_trusted(&host);
        Box::pin(async move {
            use std::net::ToSocketAddrs;
            type BoxError = Box<dyn std::error::Error + Send + Sync>;
            let resolved = tokio::task::spawn_blocking(move || {
                (host.as_str(), 0u16)
                    .to_socket_addrs()
                    .map(|iter| iter.collect::<Vec<_>>())
            })
            .await
            .map_err(|e| -> BoxError { Box::new(e) })?;
            let addrs = resolved.map_err(|e| -> BoxError { Box::new(e) })?;
            let allowed: Vec<std::net::SocketAddr> = addrs
                .into_iter()
                .filter(|a| trusted || public_ip(a.ip()))
                .collect();
            if allowed.is_empty() {
                return Err(Box::new(PolicyError::HostBlocked(
                    "DNS lieferte nur nicht freigegebene Adressen".to_string(),
                )) as BoxError);
            }
            Ok(Box::new(allowed.into_iter()) as reqwest::dns::Addrs)
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_http_schemes_pass() {
        assert!(scheme_allowed(
            &Url::parse("https://example.com/f.xml").unwrap()
        ));
        assert!(scheme_allowed(
            &Url::parse("http://example.com/f.xml").unwrap()
        ));
        assert!(!scheme_allowed(&Url::parse("file:///etc/passwd").unwrap()));
        assert!(!scheme_allowed(
            &Url::parse("ftp://example.com/f.xml").unwrap()
        ));
        assert!(!scheme_allowed(&Url::parse("javascript:alert(1)").unwrap()));
        assert!(check_url("file:///etc/passwd", &[]).is_err());
    }

    #[test]
    fn private_and_loopback_targets_are_blocked() {
        for raw in [
            "http://127.0.0.1/f.xml",
            "http://localhost:8080/f.xml",
            "http://[::1]/f.xml",
            "http://10.0.0.5/f.xml",
            "http://192.168.1.1/f.xml",
            "http://169.254.169.254/latest/meta-data",
            "http://100.64.0.1/f.xml",
            "http://0.0.0.0/f.xml",
        ] {
            assert!(check_url(raw, &[]).is_err(), "{raw} muss gesperrt sein");
        }
    }

    #[test]
    fn explicitly_trusted_intranet_origin_is_allowed_only_for_itself() {
        let trusted = vec!["http://192.168.1.10:8080".to_string()];
        assert!(check_url("http://192.168.1.10:8080/feed.xml", &trusted).is_ok());
        assert!(check_url("http://192.168.1.11:8080/feed.xml", &trusted).is_err());
        assert!(
            check_url("http://example.com/feed.xml", &trusted).is_ok(),
            "öffentliche Ziele bleiben erlaubt"
        );
    }

    #[test]
    fn public_addresses_pass() {
        assert!(public_ip("93.184.216.34".parse().unwrap()));
        assert!(public_ip("2a00:1450:4001:82f::200e".parse().unwrap()));
        assert!(!public_ip("172.16.0.1".parse().unwrap()));
        assert!(!public_ip("fc00::1".parse().unwrap()));
        assert!(!public_ip("fe80::1".parse().unwrap()));
        assert!(!public_ip("::ffff:127.0.0.1".parse().unwrap()));
    }
}
