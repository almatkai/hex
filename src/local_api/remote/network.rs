//! Tailscale CLI detection, with bounded output and a hard child deadline.
use color_eyre::eyre::{Result, eyre};
use std::io::Read;
use std::net::IpAddr;
use std::process::Stdio;
use std::time::{Duration, Instant};

pub(super) fn detect() -> Result<IpAddr> {
    #[cfg(windows)]
    let program = std::env::var_os("ProgramFiles")
        .map(std::path::PathBuf::from)
        .map(|p| p.join("Tailscale/tailscale.exe"))
        .filter(|p| p.is_file())
        .unwrap_or_else(|| "tailscale.exe".into());
    #[cfg(not(windows))]
    let program = "tailscale";
    let mut child = crate::desktop::background_process::command(program)
        .args(["ip", "-4"])
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .stdin(Stdio::null())
        .spawn()?;
    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| eyre!("Tailscale stdout unavailable"))?;
    let reader = std::thread::spawn(move || {
        let mut bytes = Vec::new();
        stdout.take(4097).read_to_end(&mut bytes).map(|_| bytes)
    });
    let deadline = Instant::now() + Duration::from_secs(2);
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break Some(status),
            Ok(None) if Instant::now() < deadline => std::thread::sleep(Duration::from_millis(20)),
            _ => {
                let _ = child.kill();
                let _ = child.wait();
                break None;
            }
        }
    };
    let bytes = reader
        .join()
        .map_err(|_| eyre!("Tailscale output unavailable"))??;
    if !status.is_some_and(|s| s.success()) || bytes.len() > 4096 {
        return Err(eyre!("Tailscale unavailable"));
    }
    parse(&bytes)
}

fn parse(bytes: &[u8]) -> Result<IpAddr> {
    let ip: IpAddr = std::str::from_utf8(bytes)?.trim().parse()?;
    super::bind_address(ip, 47821)?;
    Ok(ip)
}

#[cfg(test)]
mod tests {
    #[test]
    fn remote_detection_refuses_non_tailnet_and_ambiguous_output() {
        for value in [
            "127.0.0.1",
            "0.0.0.0",
            "192.168.1.1",
            "100.63.1.1",
            "100.128.0.1",
            "100.64.1.1\n100.64.2.2",
        ] {
            assert!(super::parse(value.as_bytes()).is_err());
        }
        assert!(super::parse(b"100.64.114.14\r\n").is_ok());
    }
}
