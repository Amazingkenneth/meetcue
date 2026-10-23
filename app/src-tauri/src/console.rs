//! Console output for a GUI-subsystem executable on Windows: attach to the
//! parent console so `--version` and `--self-test` print where they were run.

#[cfg(windows)]
pub fn attach() {
    use windows::Win32::System::Console::{AttachConsole, ATTACH_PARENT_PROCESS};
    // Fails harmlessly when there is no parent console or output is redirected.
    unsafe {
        let _ = AttachConsole(ATTACH_PARENT_PROCESS);
    }
}

#[cfg(not(windows))]
pub fn attach() {}

/// Shows a blocking native error box (used when the webview cannot start).
#[cfg(windows)]
pub fn fatal_box(title: &str, text: &str) {
    use windows::core::HSTRING;
    use windows::Win32::UI::WindowsAndMessaging::{MessageBoxW, MB_ICONERROR, MB_OK};
    unsafe {
        MessageBoxW(
            None,
            &HSTRING::from(text),
            &HSTRING::from(title),
            MB_OK | MB_ICONERROR,
        );
    }
}

#[cfg(not(windows))]
pub fn fatal_box(title: &str, text: &str) {
    eprintln!("{title}: {text}");
}
