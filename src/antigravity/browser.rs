//! Ask the OS to open the browser; never launch a helper executable.
use color_eyre::eyre::{Result, eyre};
#[cfg(windows)]
pub(super) fn open(url: &str) -> Result<()> {
    use windows::Win32::UI::{Shell::ShellExecuteW, WindowsAndMessaging::SW_SHOWNORMAL};
    use windows::core::PCWSTR;
    let url: Vec<u16> = url.encode_utf16().chain(Some(0)).collect();
    // SAFETY: the nul-terminated URL lives for the duration of ShellExecuteW;
    // the shell owns the browser launch and no process handle is retained.
    let result = unsafe {
        ShellExecuteW(
            None,
            windows::core::w!("open"),
            PCWSTR(url.as_ptr()),
            None,
            None,
            SW_SHOWNORMAL,
        )
    };
    if result.0 as isize <= 32 {
        return Err(eyre!("Could not open Google sign-in in your browser"));
    }
    Ok(())
}
#[cfg(target_os = "macos")]
pub(super) fn open(url: &str) -> Result<()> {
    use objc2_foundation::{NSString, NSURL};
    let url = NSURL::URLWithString(&NSString::from_str(url))
        .ok_or_else(|| eyre!("Invalid Google sign-in URL"))?;
    if !objc2_app_kit::NSWorkspace::sharedWorkspace().openURL(&url) {
        return Err(eyre!("Could not open Google sign-in in your browser"));
    }
    Ok(())
}
#[cfg(target_os = "linux")]
pub(super) fn open(url: &str) -> Result<()> {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?;
    runtime.block_on(async {
        let operation = async {
            let connection = zbus::Connection::session().await?;
            let proxy = zbus::Proxy::new(
                &connection,
                "org.freedesktop.portal.Desktop",
                "/org/freedesktop/portal/desktop",
                "org.freedesktop.portal.OpenURI",
            )
            .await?;
            let options = std::collections::HashMap::<&str, zbus::zvariant::Value<'_>>::new();
            let _: zbus::zvariant::OwnedObjectPath =
                proxy.call("OpenURI", &("", url, options)).await?;
            Ok::<_, zbus::Error>(())
        };
        tokio::time::timeout(std::time::Duration::from_secs(10), operation)
            .await
            .map_err(|_| eyre!("Opening Google sign-in timed out"))?
            .map_err(|_| eyre!("Could not open Google sign-in in your browser"))
    })
}
