//! Keeps the device's private key in the OS credential store: Keychain on
//! macOS, Credential Manager on Windows, Secret Service on Linux.

use anyhow::Result;

#[cfg(feature = "os-keychain")]
const SERVICE: &str = "universal-clipboard";

#[cfg(feature = "os-keychain")]
fn entry(device_id: &str) -> Result<keyring::Entry> {
    Ok(keyring::Entry::new(SERVICE, device_id)?)
}

/// Stores `private_key` (hex) and reads it back to prove the store works.
#[cfg(feature = "os-keychain")]
pub fn store(device_id: &str, private_key: &str) -> Result<()> {
    // On platforms keyring has no real store for (e.g. Android) it falls back
    // to an in-memory mock; the key would be gone after a restart.
    let persistence = keyring::default::default_credential_builder().persistence();
    if !matches!(
        persistence,
        keyring::credential::CredentialPersistence::UntilDelete
    ) {
        anyhow::bail!("no persistent keychain on this platform");
    }
    let entry = entry(device_id)?;
    entry.set_password(private_key)?;
    if entry.get_password()? != private_key {
        anyhow::bail!("keychain returned a different key than was stored");
    }
    Ok(())
}

#[cfg(feature = "os-keychain")]
pub fn load(device_id: &str) -> Result<String> {
    // macOS asks before letting a rebuilt (differently signed) binary read the
    // key, and the read blocks until someone answers. Say so instead of hanging
    // silently.
    let (done, waiting) = std::sync::mpsc::channel::<()>();
    std::thread::spawn(move || {
        if waiting.recv_timeout(HINT_AFTER) == Err(std::sync::mpsc::RecvTimeoutError::Timeout) {
            eprintln!(
                "Waiting for access to this device's key in the OS keychain. \
                 If a dialog asks, enter your password and choose \"Always Allow\"."
            );
        }
    });
    let key = entry(device_id)?.get_password();
    let _ = done.send(());
    Ok(key?)
}

#[cfg(feature = "os-keychain")]
const HINT_AFTER: std::time::Duration = std::time::Duration::from_secs(2);

#[cfg(not(feature = "os-keychain"))]
pub fn store(_device_id: &str, _private_key: &str) -> Result<()> {
    anyhow::bail!("built without keychain support")
}

#[cfg(not(feature = "os-keychain"))]
pub fn load(_device_id: &str) -> Result<String> {
    anyhow::bail!("built without keychain support")
}
