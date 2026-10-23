//! The system proxy set in Windows Settings (Network & internet › Proxy ›
//! Manual proxy setup). Automatic configuration scripts are not evaluated.

/// `ProxyServer` and `ProxyOverride` values when a manual proxy is enabled.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SystemProxy {
    /// `host:port`, or per-scheme entries like `http=a:80;https=b:443`.
    pub server: String,
    /// Hosts that bypass the proxy, separated by `;` (`*.corp`, `<local>`).
    pub bypass: String,
}

#[cfg(target_os = "windows")]
pub fn system_proxy() -> Option<SystemProxy> {
    let key = windows_registry::CURRENT_USER
        .open(r"Software\Microsoft\Windows\CurrentVersion\Internet Settings")
        .ok()?;
    if key.get_u32("ProxyEnable").ok()? == 0 {
        return None;
    }
    let server = key.get_string("ProxyServer").ok()?;
    if server.trim().is_empty() {
        return None;
    }
    Some(SystemProxy {
        server,
        bypass: key.get_string("ProxyOverride").unwrap_or_default(),
    })
}

/// Other platforms use the proxy environment variables, which the network
/// layer reads itself.
#[cfg(not(target_os = "windows"))]
pub fn system_proxy() -> Option<SystemProxy> {
    None
}
