//! Device fingerprint (Brief Section 8.1).
//!
//! Two different jobs are deliberately kept apart here, because conflating
//! them is what makes naive copy-protection break honest users:
//!
//! * **`device_id`** — the one value a licence is bound to, and half of the
//!   key material for everything encrypted at rest. It comes from the Windows
//!   `MachineGuid` alone. That identifier survives a new disk, a new CPU, more
//!   RAM, even a new motherboard; it changes only when Windows itself is
//!   reinstalled, at which point the database is gone anyway. See D24.
//!
//! * **The component profile** — `MachineGuid` plus the CPU, baseboard, BIOS
//!   and system-volume identifiers, each hashed separately. This is never key
//!   material. It is recorded at activation so the app can *notice* that the
//!   machine has changed and say so, without ever locking anyone out for it.
//!
//! Everything is read from the registry. No WMI, no COM, and nothing is
//! spawned — `wmic` is deprecated on Windows 11 and shelling out from a
//! hardened app is a liability we do not need.

use sha2::{Digest, Sha256};

use crate::crypto::APP_SALT;
use crate::error::{CoreError, Result};

/// One fingerprint input, stored only as a hash.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Component {
    /// Stable key, e.g. `"cpu"`. Used to line components up across launches.
    pub name: String,
    /// Hex SHA-256 of the salted value. The raw value never leaves this module.
    pub hash: String,
}

/// What we could read about this machine.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct DeviceProfile {
    /// Present on a healthy Windows install; absent means we cannot bind.
    pub machine_guid: Option<String>,
    pub components: Vec<Component>,
}

fn hash_component(name: &str, value: &str) -> Component {
    // Normalise first: case and surrounding space differ between the registry
    // and WMI for the same machine, and we must not be sensitive to that.
    let normalised = value.trim().to_ascii_lowercase();
    let mut hasher = Sha256::new();
    hasher.update(APP_SALT);
    hasher.update(b"|component|");
    hasher.update(name.as_bytes());
    hasher.update(b"|");
    hasher.update(normalised.as_bytes());
    Component {
        name: name.to_string(),
        hash: hex(&hasher.finalize()),
    }
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

impl DeviceProfile {
    /// The value a licence binds to, and half the key material.
    ///
    /// Fails when `MachineGuid` could not be read, which is the one case where
    /// we genuinely cannot identify the machine. Refusing is right: deriving a
    /// key from a guessed identity would silently lock the student out of
    /// their own data later.
    pub fn device_id(&self) -> Result<[u8; 32]> {
        let guid = self.machine_guid.as_deref().ok_or_else(|| {
            CoreError::Invalid(
                "StudyBites could not read this computer's Windows machine identifier, so it \
                 cannot be activated here. This usually means the registry is restricted."
                    .into(),
            )
        })?;

        let mut hasher = Sha256::new();
        hasher.update(APP_SALT);
        hasher.update(b"|device-id|v1|");
        hasher.update(guid.trim().to_ascii_lowercase().as_bytes());
        Ok(hasher.finalize().into())
    }

    /// First 8 bytes of `device_id`: what an activation code carries, so the
    /// code stays short enough to read out over a phone.
    pub fn device_tag(&self) -> Result<[u8; 8]> {
        let id = self.device_id()?;
        let mut tag = [0u8; 8];
        tag.copy_from_slice(&id[..8]);
        Ok(tag)
    }
}

/// How far the machine has drifted from the one that was activated.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Drift {
    pub matched: usize,
    pub total: usize,
    /// Component names that changed, for an honest message in Settings.
    pub changed: Vec<String>,
}

impl Drift {
    /// Advisory only. Nothing in the app refuses to run on this.
    pub fn is_same_machine(&self) -> bool {
        self.changed.is_empty()
    }
}

/// Compare the profile recorded at activation with the machine running now.
///
/// Components missing from either side are not counted as changes: an unread
/// registry key is our problem, not evidence of a different computer.
pub fn drift(activated: &[Component], current: &[Component]) -> Drift {
    let mut matched = 0;
    let mut total = 0;
    let mut changed = Vec::new();

    for was in activated {
        let Some(now) = current.iter().find(|c| c.name == was.name) else {
            continue;
        };
        total += 1;
        if now.hash == was.hash {
            matched += 1;
        } else {
            changed.push(was.name.clone());
        }
    }
    Drift {
        matched,
        total,
        changed,
    }
}

// ---- collection ---------------------------------------------------------

#[cfg(windows)]
pub fn collect() -> DeviceProfile {
    use winreg::enums::{HKEY_LOCAL_MACHINE, KEY_READ, KEY_WOW64_64KEY};
    use winreg::RegKey;

    let hklm = RegKey::predef(HKEY_LOCAL_MACHINE);

    // Always the 64-bit view: under WOW64 the 32-bit view of Cryptography has
    // a *different* MachineGuid, which would change the device id if the app
    // were ever built for another target.
    let read = |path: &str, name: &str| -> Option<String> {
        hklm.open_subkey_with_flags(path, KEY_READ | KEY_WOW64_64KEY)
            .ok()?
            .get_value::<String, _>(name)
            .ok()
            .filter(|v| !v.trim().is_empty())
    };

    let machine_guid = read(r"SOFTWARE\Microsoft\Cryptography", "MachineGuid");

    let mut components = Vec::new();
    let mut push = |name: &str, value: Option<String>| {
        if let Some(value) = value {
            components.push(hash_component(name, &value));
        }
    };

    push("machine_guid", machine_guid.clone());
    push(
        "cpu",
        read(
            r"HARDWARE\DESCRIPTION\System\CentralProcessor\0",
            "ProcessorNameString",
        ),
    );
    push(
        "baseboard",
        read(r"HARDWARE\DESCRIPTION\System\BIOS", "BaseBoardProduct"),
    );
    push(
        "bios",
        read(r"HARDWARE\DESCRIPTION\System\BIOS", "BIOSVersion"),
    );
    push(
        "system_product",
        read(r"HARDWARE\DESCRIPTION\System\BIOS", "SystemProductName"),
    );
    push("system_volume", system_volume_serial());

    DeviceProfile {
        machine_guid,
        components,
    }
}

/// Serial of the volume Windows is installed on. This is the one component
/// that a reformat changes, which is exactly why it is advisory and not part
/// of `device_id`.
#[cfg(windows)]
fn system_volume_serial() -> Option<String> {
    use windows_sys::Win32::Storage::FileSystem::GetVolumeInformationW;

    let drive = std::env::var("SystemDrive").unwrap_or_else(|_| "C:".into());
    let root: Vec<u16> = format!("{}\\", drive.trim_end_matches('\\'))
        .encode_utf16()
        .chain(std::iter::once(0))
        .collect();

    let mut serial: u32 = 0;
    // SAFETY: `root` is a NUL-terminated UTF-16 buffer that outlives the call,
    // every optional out-parameter is null with a matching zero length, and
    // `serial` is a live u32 the call only writes.
    let ok = unsafe {
        GetVolumeInformationW(
            root.as_ptr(),
            std::ptr::null_mut(),
            0,
            &mut serial,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            0,
        )
    };
    (ok != 0).then(|| format!("{serial:08X}"))
}

/// Development stand-in for Linux and macOS, so the app and its tests run in
/// Codespaces and CI. Shipping builds are Windows-only, so this is never the
/// code a student runs.
#[cfg(not(windows))]
pub fn collect() -> DeviceProfile {
    let host = std::fs::read_to_string("/etc/hostname")
        .ok()
        .map(|h| h.trim().to_string())
        .filter(|h| !h.is_empty())
        .unwrap_or_else(|| "studybites-dev-host".to_string());

    let guid = format!("dev-{host}");
    DeviceProfile {
        components: vec![
            hash_component("machine_guid", &guid),
            hash_component("cpu", "development"),
        ],
        machine_guid: Some(guid),
    }
}
