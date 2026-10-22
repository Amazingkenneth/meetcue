//! Platform services behind small, testable interfaces.
//!
//! - [`paths`]: where settings, logs and practice records live (portable
//!   next to the executable on Windows, otherwise the user profile).
//! - [`credentials`]: API keys in the system credential store only.
//! - [`speech`]: offline system text-to-speech for practice mode.
//! - [`window`]: native window styles the webview cannot set (Windows).

pub mod credentials;
pub mod paths;
pub mod speech;
pub mod window;

/// The operating system's preferred UI locale, e.g. `zh-CN` or `en-US`.
pub fn system_locale() -> String {
    sys_locale::get_locale().unwrap_or_else(|| "en-US".into())
}
