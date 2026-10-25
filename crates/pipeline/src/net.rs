//! Network plumbing shared by all providers.
//!
//! - TLS uses the operating system's certificate store, so HTTPS-inspecting
//!   corporate proxies with a company root certificate work.
//! - Proxies: an explicit proxy from the settings, else `HTTPS_PROXY` /
//!   `ALL_PROXY`, else the system proxy the app passes in. WebSocket
//!   connections go through an HTTP `CONNECT` tunnel.

use std::collections::VecDeque;
use std::net::SocketAddr;
use std::sync::{Arc, OnceLock, RwLock};
use std::time::Duration;

use base64::Engine;
use futures::stream::{FuturesUnordered, StreamExt};
use meetcue_core::{codes, MeetCueError};
use rustls::ClientConfig;
use rustls_platform_verifier::ConfigVerifierExt;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio_tungstenite::tungstenite::client::IntoClientRequest;
use tokio_tungstenite::tungstenite::http::{HeaderName, HeaderValue};
use tokio_tungstenite::{MaybeTlsStream, WebSocketStream};

pub type Ws = WebSocketStream<MaybeTlsStream<TcpStream>>;

pub const CONNECT_TIMEOUT: Duration = Duration::from_secs(10);

/// Process-wide TLS configuration with the platform verifier.
pub fn tls_config() -> Result<Arc<ClientConfig>, MeetCueError> {
    static CONFIG: OnceLock<Result<Arc<ClientConfig>, String>> = OnceLock::new();
    CONFIG
        .get_or_init(|| {
            let _ = rustls::crypto::aws_lc_rs::default_provider().install_default();
            ClientConfig::with_platform_verifier()
                .map(Arc::new)
                .map_err(|e| e.to_string())
        })
        .clone()
        .map_err(|e| MeetCueError::new(codes::NET_TLS, e))
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Proxy {
    pub host: String,
    pub port: u16,
    /// `user:password` for basic authentication.
    pub auth: Option<String>,
}

impl Proxy {
    /// Parses `http://user:pass@host:port`, `host:port` and similar forms.
    pub fn parse(s: &str) -> Option<Proxy> {
        let s = s.trim();
        if s.is_empty() {
            return None;
        }
        let rest = s.split_once("://").map(|(_, r)| r).unwrap_or(s);
        let rest = rest.trim_end_matches('/');
        let (auth, hostport) = match rest.rsplit_once('@') {
            Some((a, h)) => (Some(a.to_string()), h),
            None => (None, rest),
        };
        let (host, port) = match hostport.rsplit_once(':') {
            Some((h, p)) => (h.to_string(), p.parse().ok()?),
            None => (hostport.to_string(), 80),
        };
        if host.is_empty() {
            return None;
        }
        Some(Proxy { host, port, auth })
    }
}

fn env_proxy() -> Option<String> {
    ["HTTPS_PROXY", "https_proxy", "ALL_PROXY", "all_proxy"]
        .iter()
        .find_map(|k| std::env::var(k).ok().filter(|v| !v.trim().is_empty()))
}

fn no_proxy(host: &str) -> bool {
    let list = std::env::var("NO_PROXY")
        .or_else(|_| std::env::var("no_proxy"))
        .unwrap_or_default();
    list.split(',')
        .map(str::trim)
        .filter(|e| !e.is_empty())
        .any(|e| {
            e == "*"
                || host == e.trim_start_matches('.')
                || host.ends_with(&format!(".{}", e.trim_start_matches('.')))
        })
}

/// The system proxy (`ProxyServer`, `ProxyOverride` on Windows), set by the app.
static SYSTEM_PROXY: RwLock<Option<(String, String)>> = RwLock::new(None);

/// Sets or clears the system proxy used when no explicit or environment
/// proxy applies.
pub fn set_system_proxy(proxy: Option<(String, String)>) {
    *SYSTEM_PROXY.write().unwrap() = proxy;
}

/// The HTTPS entry of a system proxy setting: `host:port`, or the `https=`
/// (else `http=`) part of `http=a:80;https=b:443`.
fn system_server(server: &str) -> Option<&str> {
    if !server.contains('=') {
        return Some(server.trim());
    }
    let entries: Vec<(&str, &str)> = server
        .split(';')
        .filter_map(|e| e.split_once('='))
        .map(|(k, v)| (k.trim(), v.trim()))
        .collect();
    ["https", "http"]
        .iter()
        .find_map(|scheme| entries.iter().find(|(k, _)| k.eq_ignore_ascii_case(scheme)))
        .map(|(_, v)| *v)
}

/// Whether `host` matches the system bypass list (`*` wildcards, `<local>`
/// for names without a dot).
fn system_bypass(host: &str, bypass: &str) -> bool {
    let host = host.to_ascii_lowercase();
    bypass.split(';').map(str::trim).any(|p| {
        if p.eq_ignore_ascii_case("<local>") {
            return !host.contains('.');
        }
        let p = p.to_ascii_lowercase();
        match p.split_once('*') {
            None => !p.is_empty() && host == p,
            Some((pre, post)) => {
                host.len() >= pre.len() + post.len()
                    && host.starts_with(pre)
                    && host.ends_with(post)
            }
        }
    })
}

/// The proxy to use for `host`: explicit setting first, then environment,
/// then the system proxy.
pub fn proxy_for(host: &str, explicit: Option<&str>) -> Option<Proxy> {
    if let Some(p) = explicit.and_then(Proxy::parse) {
        return Some(p);
    }
    if no_proxy(host) {
        return None;
    }
    if let Some(p) = env_proxy().as_deref().and_then(Proxy::parse) {
        return Some(p);
    }
    let system = SYSTEM_PROXY.read().unwrap().clone()?;
    if system_bypass(host, &system.1) {
        return None;
    }
    system_server(&system.0).and_then(Proxy::parse)
}

fn connect_err(host: &str, e: impl std::fmt::Display) -> MeetCueError {
    MeetCueError::new(codes::ASR_CONNECT, format!("{host}: {e}"))
}

/// Delay before the next address is tried while earlier attempts are still
/// pending (RFC 8305 "Happy Eyeballs").
const ATTEMPT_DELAY: Duration = Duration::from_millis(250);

/// Resolved addresses with the families alternating, the first family first.
fn interleave(addrs: Vec<SocketAddr>) -> Vec<SocketAddr> {
    let first_v6 = addrs.first().is_some_and(SocketAddr::is_ipv6);
    let (mut a, mut b): (VecDeque<_>, VecDeque<_>) =
        addrs.into_iter().partition(|x| x.is_ipv6() == first_v6);
    let mut out = Vec::with_capacity(a.len() + b.len());
    while !a.is_empty() || !b.is_empty() {
        out.extend(a.pop_front());
        out.extend(b.pop_front());
    }
    out
}

/// Connects to the first address of `host` that answers. Attempts start
/// [`ATTEMPT_DELAY`] apart, alternating IPv6 and IPv4, so an address family
/// that is configured but not routed (common with IPv6) doesn't use up the
/// whole connect timeout as trying the addresses one after another would.
async fn connect_any(host: &str, port: u16) -> std::io::Result<TcpStream> {
    let addrs = interleave(tokio::net::lookup_host((host, port)).await?.collect());
    let mut pending = addrs.into_iter();
    let mut attempts = FuturesUnordered::new();
    let mut last_err = None;
    loop {
        if let Some(addr) = pending.next() {
            attempts.push(TcpStream::connect(addr));
        }
        if attempts.is_empty() {
            break;
        }
        // An attempt finishes, or the next one is due.
        tokio::select! {
            r = attempts.next() => match r {
                Some(Ok(s)) => return Ok(s),
                // Failed: the next address is tried at once.
                Some(Err(e)) => last_err = Some(e),
                None => {}
            },
            _ = tokio::time::sleep(ATTEMPT_DELAY), if pending.len() > 0 => {}
        }
    }
    Err(last_err
        .unwrap_or_else(|| std::io::Error::new(std::io::ErrorKind::NotFound, "no addresses found")))
}

/// Opens a TCP connection to `host:port`, through an HTTP CONNECT tunnel
/// when a proxy is configured.
pub async fn tcp(host: &str, port: u16, proxy: Option<&Proxy>) -> Result<TcpStream, MeetCueError> {
    let fut = async {
        match proxy {
            None => connect_any(host, port)
                .await
                .map_err(|e| connect_err(host, e)),
            Some(p) => {
                let mut s = connect_any(&p.host, p.port)
                    .await
                    .map_err(|e| connect_err(&p.host, e))?;
                let mut req = format!("CONNECT {host}:{port} HTTP/1.1\r\nHost: {host}:{port}\r\n");
                if let Some(auth) = &p.auth {
                    let token = base64::engine::general_purpose::STANDARD.encode(auth);
                    req.push_str(&format!("Proxy-Authorization: Basic {token}\r\n"));
                }
                req.push_str("\r\n");
                s.write_all(req.as_bytes())
                    .await
                    .map_err(|e| connect_err(&p.host, e))?;
                let mut buf = Vec::with_capacity(256);
                let mut byte = [0u8; 1];
                while !buf.ends_with(b"\r\n\r\n") {
                    if s.read(&mut byte)
                        .await
                        .map_err(|e| connect_err(&p.host, e))?
                        == 0
                        || buf.len() > 8192
                    {
                        return Err(connect_err(&p.host, "proxy closed the connection"));
                    }
                    buf.push(byte[0]);
                }
                let status = String::from_utf8_lossy(&buf);
                let ok = status
                    .lines()
                    .next()
                    .is_some_and(|l| l.split_whitespace().nth(1) == Some("200"));
                if !ok {
                    let first = status.lines().next().unwrap_or("").to_string();
                    return Err(connect_err(host, format!("proxy refused: {first}")));
                }
                Ok(s)
            }
        }
    };
    tokio::time::timeout(CONNECT_TIMEOUT, fut)
        .await
        .map_err(|_| connect_err(host, "connection timed out"))?
}

/// Connects a WebSocket (`wss://`) with extra headers.
pub async fn websocket(
    url: &str,
    headers: &[(&str, String)],
    proxy: Option<&str>,
) -> Result<(Ws, u16), MeetCueError> {
    let mut request = url
        .into_client_request()
        .map_err(|e| MeetCueError::new(codes::ASR_CONNECT, e.to_string()))?;
    for (k, v) in headers {
        let value = HeaderValue::from_str(v)
            .map_err(|e| MeetCueError::new(codes::ASR_AUTH, e.to_string()))?;
        let name = HeaderName::from_bytes(k.as_bytes())
            .map_err(|e| MeetCueError::new(codes::ASR_CONNECT, e.to_string()))?;
        request.headers_mut().insert(name, value);
    }
    let host = request.uri().host().unwrap_or_default().to_string();
    let port = request.uri().port_u16().unwrap_or(443);
    let proxy = proxy_for(&host, proxy);
    let stream = tcp(&host, port, proxy.as_ref()).await?;
    let _ = stream.set_nodelay(true);
    let connector = tokio_tungstenite::Connector::Rustls(tls_config()?);
    let fut =
        tokio_tungstenite::client_async_tls_with_config(request, stream, None, Some(connector));
    match tokio::time::timeout(CONNECT_TIMEOUT, fut).await {
        Err(_) => Err(connect_err(&host, "handshake timed out")),
        Ok(Ok((ws, resp))) => Ok((ws, resp.status().as_u16())),
        Ok(Err(tokio_tungstenite::tungstenite::Error::Http(resp))) => {
            let status = resp.status().as_u16();
            let body = resp
                .body()
                .as_ref()
                .map(|b| {
                    String::from_utf8_lossy(b)
                        .chars()
                        .take(300)
                        .collect::<String>()
                })
                .unwrap_or_default();
            let code = match status {
                400 => codes::ASR_PROTOCOL,
                401 | 403 => codes::ASR_AUTH,
                429 => codes::ASR_RATE_LIMIT,
                _ => codes::ASR_CONNECT,
            };
            Err(MeetCueError::new(code, format!("HTTP {status}: {body}")))
        }
        Ok(Err(tokio_tungstenite::tungstenite::Error::Tls(e))) => {
            Err(MeetCueError::new(codes::NET_TLS, e.to_string()))
        }
        Ok(Err(e)) => Err(connect_err(&host, e)),
    }
}

/// HTTP client for REST and streaming calls. Uses the system proxy settings
/// unless an explicit proxy is given.
pub fn http_client(proxy: Option<&str>) -> Result<reqwest::Client, MeetCueError> {
    let mut b = reqwest::Client::builder()
        .connect_timeout(CONNECT_TIMEOUT)
        .pool_idle_timeout(Duration::from_secs(90))
        .tcp_nodelay(true)
        .user_agent(concat!("MeetCue/", env!("CARGO_PKG_VERSION")));
    if let Some(p) = proxy.filter(|p| !p.trim().is_empty()) {
        let url = if p.contains("://") {
            p.to_string()
        } else {
            format!("http://{p}")
        };
        b = b.proxy(
            reqwest::Proxy::all(url)
                .map_err(|e| MeetCueError::new(codes::NET_TLS, e.to_string()))?,
        );
    }
    b.build()
        .map_err(|e| MeetCueError::new(codes::NET_TLS, e.to_string()))
}

/// Maps an HTTP status from a model API to an error code.
pub fn llm_status_code(status: u16) -> &'static str {
    match status {
        401 | 403 => codes::LLM_AUTH,
        404 => codes::LLM_MODEL_UNAVAILABLE,
        408 | 504 => codes::LLM_TIMEOUT,
        429 | 529 => codes::LLM_RATE_LIMIT,
        _ => codes::LLM_REQUEST,
    }
}

/// Maps a transport error from reqwest.
pub fn reqwest_error(e: &reqwest::Error) -> MeetCueError {
    let code = if e.is_timeout() {
        codes::LLM_TIMEOUT
    } else if format!("{e:?}").to_lowercase().contains("certificate") {
        codes::NET_TLS
    } else {
        codes::LLM_REQUEST
    };
    MeetCueError::new(code, e.to_string())
}

/// Waits before retry `attempt` (0-based): exponential backoff with jitter,
/// or the server's `Retry-After` when given.
pub fn backoff(attempt: u32, retry_after: Option<Duration>) -> Duration {
    if let Some(r) = retry_after {
        return r.min(Duration::from_secs(30));
    }
    let base = 250u64 * (1 << attempt.min(6));
    let jitter = rand::random::<u64>() % (base / 2 + 1);
    Duration::from_millis(base + jitter)
}

pub fn retry_after(headers: &reqwest::header::HeaderMap) -> Option<Duration> {
    headers
        .get(reqwest::header::RETRY_AFTER)?
        .to_str()
        .ok()?
        .trim()
        .parse::<u64>()
        .ok()
        .map(Duration::from_secs)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_proxies() {
        assert_eq!(
            Proxy::parse("http://user:p%40ss@proxy.local:3128/"),
            Some(Proxy {
                host: "proxy.local".into(),
                port: 3128,
                auth: Some("user:p%40ss".into())
            })
        );
        assert_eq!(Proxy::parse("10.0.0.1:8080").unwrap().port, 8080);
        assert_eq!(Proxy::parse("proxy.local").unwrap().port, 80);
        assert!(Proxy::parse("").is_none());
        assert!(Proxy::parse("http://:80").is_none());
        assert_eq!(proxy_for("x", Some("p:1")).unwrap().host, "p");
    }

    #[test]
    fn backoff_grows_and_honours_retry_after() {
        assert!(backoff(0, None) >= Duration::from_millis(250));
        assert!(backoff(3, None) >= Duration::from_millis(2000));
        assert_eq!(
            backoff(9, Some(Duration::from_secs(2))),
            Duration::from_secs(2)
        );
        assert_eq!(
            backoff(0, Some(Duration::from_secs(600))),
            Duration::from_secs(30)
        );
    }

    #[test]
    fn system_proxy_entries_and_bypass() {
        assert_eq!(system_server("proxy.corp:8080"), Some("proxy.corp:8080"));
        assert_eq!(
            system_server("http=a.corp:80;https=b.corp:443;ftp=c:21"),
            Some("b.corp:443")
        );
        assert_eq!(system_server("http=a.corp:80"), Some("a.corp:80"));
        assert_eq!(system_server("ftp=c:21"), None);
        assert!(system_bypass("intranet", "<local>"));
        assert!(!system_bypass("api.example.com", "<local>"));
        assert!(system_bypass("wiki.corp.example", "*.corp.example;<local>"));
        assert!(system_bypass("10.1.2.3", "10.*"));
        assert!(!system_bypass("api.example.com", "*.corp.example"));
        assert!(!system_bypass("api.example.com", ""));
    }

    #[test]
    fn address_families_alternate() {
        let a = |s: &str| s.parse::<SocketAddr>().unwrap();
        let v6 = [a("[2001:db8::1]:443"), a("[2001:db8::2]:443")];
        let v4 = [a("192.0.2.1:443"), a("192.0.2.2:443"), a("192.0.2.3:443")];
        assert_eq!(
            interleave(vec![v6[0], v6[1], v4[0], v4[1], v4[2]]),
            vec![v6[0], v4[0], v6[1], v4[1], v4[2]]
        );
        assert_eq!(
            interleave(vec![v4[0], v4[1], v6[0]]),
            vec![v4[0], v6[0], v4[1]]
        );
        assert!(interleave(Vec::new()).is_empty());
    }

    #[tokio::test]
    async fn connects_by_name_to_a_listening_address() {
        use tokio::net::TcpListener;
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        tokio::spawn(async move {
            let _ = listener.accept().await;
        });
        // `localhost` may resolve to ::1 first, where nothing listens.
        let s = tcp("localhost", port, None).await.unwrap();
        assert_eq!(s.peer_addr().unwrap().port(), port);
    }

    #[tokio::test]
    async fn connect_through_proxy_tunnel() {
        use tokio::net::TcpListener;
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            let (mut s, _) = listener.accept().await.unwrap();
            let mut buf = vec![0u8; 512];
            let n = s.read(&mut buf).await.unwrap();
            let req = String::from_utf8_lossy(&buf[..n]).to_string();
            s.write_all(b"HTTP/1.1 200 Connection established\r\n\r\n")
                .await
                .unwrap();
            req
        });
        let proxy = Proxy {
            host: "127.0.0.1".into(),
            port: addr.port(),
            auth: Some("a:b".into()),
        };
        tcp("api.example.com", 443, Some(&proxy)).await.unwrap();
        let req = server.await.unwrap();
        assert!(req.starts_with("CONNECT api.example.com:443 HTTP/1.1\r\n"));
        assert!(req.contains("Proxy-Authorization: Basic YTpi"));
    }

    #[tokio::test]
    async fn proxy_refusal_is_reported() {
        use tokio::net::TcpListener;
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            let (mut s, _) = listener.accept().await.unwrap();
            let mut buf = vec![0u8; 512];
            let _ = s.read(&mut buf).await;
            let _ = s.write_all(b"HTTP/1.1 403 Forbidden\r\n\r\n").await;
        });
        let proxy = Proxy {
            host: "127.0.0.1".into(),
            port: addr.port(),
            auth: None,
        };
        let e = tcp("api.example.com", 443, Some(&proxy)).await.unwrap_err();
        assert_eq!(e.code, codes::ASR_CONNECT);
        assert!(e.detail.contains("403"));
    }
}
