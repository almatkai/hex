//! One-use pairing grants and revocable, hashed device credentials.
use std::path::PathBuf;
use std::time::{Duration, Instant};

use color_eyre::eyre::{Result, bail};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

const MAX_DEVICES: usize = 32;
const PAIR_LIFETIME: Duration = Duration::from_secs(300);
pub(super) const MAX_PAIR_BYTES: usize = 1024;

#[derive(Clone, Deserialize, Serialize)]
pub(crate) struct Device {
    pub id: String,
    pub name: String,
    pub last_use_ms: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    token_hash: Option<[u8; 32]>,
}

impl Device {
    pub(super) fn public(&self) -> Self {
        Self {
            token_hash: None,
            ..self.clone()
        }
    }
}

pub(super) struct Devices {
    path: PathBuf,
    devices: Vec<Device>,
    pairing: Option<([u8; 32], Instant)>,
    failures: u32,
    window: Instant,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct PairRequest {
    pub code: String,
    pub name: String,
}

impl Devices {
    pub(super) fn load(path: PathBuf) -> Result<Self> {
        // Bound even a damaged or manually edited store before parsing it.
        let devices: Vec<Device> = match std::fs::metadata(&path) {
            Ok(meta) if meta.len() <= 32 * 1024 => serde_json::from_slice(&std::fs::read(&path)?)?,
            Ok(_) => bail!("remote device store is too large"),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Vec::new(),
            Err(error) => return Err(error.into()),
        };
        if devices.len() > MAX_DEVICES
            || devices
                .iter()
                .any(|d| d.token_hash.is_none() || !valid_name(&d.name))
        {
            bail!("invalid remote device store");
        }
        Ok(Self {
            path,
            devices,
            pairing: None,
            failures: 0,
            window: Instant::now(),
        })
    }

    pub(super) fn clear_pair(&mut self) {
        self.pairing = None;
    }

    pub(super) fn list(&self) -> Vec<Device> {
        self.devices.iter().map(Device::public).collect()
    }

    pub(super) fn begin_pair(&mut self, now: Instant) -> Result<String> {
        let code = super::super::generate_token()?;
        self.pairing = Some((hash(&code), now + PAIR_LIFETIME));
        Ok(code)
    }

    /// A global bounded bucket also limits attempts from rotating tailnet peers.
    pub(super) fn limited(&mut self, now: Instant) -> bool {
        if now.duration_since(self.window) >= Duration::from_secs(60) {
            self.window = now;
            self.failures = 0;
        }
        self.failures >= 10
    }

    pub(super) fn fail(&mut self) {
        self.failures = self.failures.saturating_add(1);
    }

    pub(super) fn pair(&mut self, request: PairRequest, now: Instant) -> Result<(String, String)> {
        if !valid_name(&request.name) || self.devices.len() >= MAX_DEVICES {
            bail!("invalid device name or device limit reached");
        }
        let digest = hash(&request.code);
        if !self
            .pairing
            .as_ref()
            .is_some_and(|(expected, deadline)| now < *deadline && equal(expected, &digest))
        {
            self.fail();
            bail!("invalid or expired pairing code");
        }
        let token = super::super::generate_token()?;
        let id = super::super::generate_token()?;
        let mut devices = self.devices.clone();
        devices.push(Device {
            id: id.clone(),
            name: request.name,
            last_use_ms: crate::desktop::events::now_ms(),
            token_hash: Some(hash(&token)),
        });
        self.save(&devices)?;
        self.devices = devices;
        self.pairing = None;
        Ok((id, token))
    }

    pub(super) fn authenticate(&mut self, token: &str) -> Option<Device> {
        let digest = hash(token);
        // Compare all fixed-size digests without early exit on matching bytes.
        let index = self
            .devices
            .iter()
            .position(|d| d.token_hash.as_ref().is_some_and(|h| equal(h, &digest)));
        let Some(index) = index else {
            self.fail();
            return None;
        };
        let now = crate::desktop::events::now_ms();
        if now.saturating_sub(self.devices[index].last_use_ms) >= 60_000 {
            let mut next = self.devices.clone();
            next[index].last_use_ms = now;
            if self.save(&next).is_ok() {
                self.devices = next;
            }
        }
        Some(self.devices[index].public())
    }

    pub(super) fn revoke(&mut self, id: &str) -> Result<bool> {
        let next: Vec<_> = self
            .devices
            .iter()
            .filter(|d| d.id != id)
            .cloned()
            .collect();
        if next.len() == self.devices.len() {
            return Ok(false);
        }
        self.save(&next)?;
        self.devices = next;
        Ok(true)
    }

    fn save(&self, devices: &[Device]) -> Result<()> {
        crate::durable::write_private(&self.path, &serde_json::to_vec(devices)?)?;
        Ok(())
    }
}

fn valid_name(name: &str) -> bool {
    !name.trim().is_empty() && name.len() <= 80 && !name.chars().any(char::is_control)
}
fn hash(token: &str) -> [u8; 32] {
    Sha256::digest(token.as_bytes()).into()
}
fn equal(left: &[u8; 32], right: &[u8; 32]) -> bool {
    let mut difference = 0_u8;
    for index in 0..32 {
        difference |= std::hint::black_box(left[index] ^ right[index]);
    }
    std::hint::black_box(difference) == 0
}

#[cfg(test)]
mod tests {
    use super::*;
    fn store() -> Devices {
        let path = std::env::temp_dir().join(format!(
            "hex-remote-{}-{}.json",
            std::process::id(),
            super::super::super::generate_token().unwrap()
        ));
        Devices::load(path).unwrap()
    }
    #[test]
    fn pairing_is_one_use_hashed_durable_and_revocable() {
        let mut store = store();
        let now = Instant::now();
        let code = store.begin_pair(now).unwrap();
        let request = || PairRequest {
            code: code.clone(),
            name: "Phone".into(),
        };
        let (id, token) = store.pair(request(), now).unwrap();
        assert!(store.pair(request(), now).is_err());
        assert!(store.authenticate("wrong").is_none());
        assert!(store.authenticate(&token).is_some());
        let saved = std::fs::read_to_string(&store.path).unwrap();
        assert!(!saved.contains(&token));
        assert!(!saved.contains(&code));
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(
                std::fs::metadata(&store.path).unwrap().permissions().mode() & 0o777,
                0o600
            );
        }
        let mut reloaded = Devices::load(store.path.clone()).unwrap();
        assert!(reloaded.authenticate(&token).is_some());
        assert!(reloaded.revoke(&id).unwrap());
        assert!(reloaded.authenticate(&token).is_none());
        assert!(
            Devices::load(store.path.clone())
                .unwrap()
                .authenticate(&token)
                .is_none()
        );
        std::fs::remove_file(store.path).unwrap();
    }
    #[test]
    fn pairing_expires_and_new_code_replaces_old_code() {
        let mut store = store();
        let now = Instant::now();
        let old = store.begin_pair(now).unwrap();
        let code = store.begin_pair(now).unwrap();
        assert!(
            store
                .pair(
                    PairRequest {
                        code: old,
                        name: "Phone".into()
                    },
                    now
                )
                .is_err()
        );
        assert!(
            store
                .pair(
                    PairRequest {
                        code,
                        name: "Phone".into()
                    },
                    now + PAIR_LIFETIME
                )
                .is_err()
        );
    }
    #[test]
    fn failed_auth_has_a_bounded_rate_limit() {
        let mut store = store();
        let now = Instant::now();
        for _ in 0..10 {
            assert!(!store.limited(now));
            store.authenticate("bad");
        }
        assert!(store.limited(now));
        assert!(!store.limited(now + Duration::from_secs(60)));
    }

    #[test]
    fn remote_pairing_bounds_names_devices_and_failed_persistence() {
        let mut store = store();
        let now = Instant::now();
        let code = store.begin_pair(now).unwrap();
        for name in [" ".to_string(), "x".repeat(81), "Phone\n".into()] {
            assert!(
                store
                    .pair(
                        PairRequest {
                            code: code.clone(),
                            name
                        },
                        now
                    )
                    .is_err()
            );
        }
        let path = store.path.clone();
        store.path = path.join("missing-parent/devices.json");
        assert!(
            store
                .pair(
                    PairRequest {
                        code: code.clone(),
                        name: "Phone".into()
                    },
                    now
                )
                .is_err()
        );
        assert!(store.devices.is_empty());
        store.path = path;
        let (_, token) = store
            .pair(
                PairRequest {
                    code: code.clone(),
                    name: "Phone".into(),
                },
                now,
            )
            .unwrap();
        assert!(store.authenticate(&token).is_some());
        store.devices = vec![store.devices[0].clone(); MAX_DEVICES];
        let code = store.begin_pair(now).unwrap();
        assert!(
            store
                .pair(
                    PairRequest {
                        code,
                        name: "Another phone".into()
                    },
                    now
                )
                .is_err()
        );
        std::fs::remove_file(store.path).unwrap();
    }
}
