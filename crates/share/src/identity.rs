//! This computer's key for shared projects.

use std::path::Path;

use iroh::SecretKey;

use crate::{Result, err};

/// A member's key pair. The secret half never leaves the computer; the public half
/// ([`Identity::id`]) is how other members know this one.
#[derive(Clone)]
pub struct Identity {
    secret: SecretKey,
}

impl std::fmt::Debug for Identity {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Identity({})", self.id())
    }
}

impl Identity {
    /// A new random identity (kept in memory only).
    pub fn generate() -> Self {
        Self {
            secret: SecretKey::generate(),
        }
    }

    /// The identity kept at `path`, made on first use (32 bytes, readable only by the user).
    pub fn load_or_create(path: &Path) -> Result<Self> {
        if let Ok(bytes) = std::fs::read(path) {
            let key: [u8; 32] = bytes
                .as_slice()
                .try_into()
                .map_err(|_| err(format!("{} isn't a PencilPilot key", path.display())))?;
            return Ok(Self {
                secret: SecretKey::from_bytes(&key),
            });
        }
        let me = Self::generate();
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir).map_err(err)?;
        }
        write_private(path, &me.secret.to_bytes()).map_err(err)?;
        Ok(me)
    }

    /// The member id other apps see (the public key, as text).
    pub fn id(&self) -> String {
        self.secret.public().to_string()
    }

    pub(crate) fn secret(&self) -> SecretKey {
        self.secret.clone()
    }
}

#[cfg(unix)]
fn write_private(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    use std::io::Write;
    use std::os::unix::fs::OpenOptionsExt;
    let mut f = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(path)?;
    f.write_all(bytes)
}

#[cfg(not(unix))]
fn write_private(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    std::fs::write(path, bytes)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn made_once_then_the_same_key() {
        let dir = std::env::temp_dir().join(format!("pp-share-id-{}", std::process::id()));
        let path = dir.join("key");
        let a = Identity::load_or_create(&path).unwrap();
        let b = Identity::load_or_create(&path).unwrap();
        assert_eq!(a.id(), b.id());
        assert_ne!(a.id(), Identity::generate().id());
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(&path).unwrap().permissions().mode();
            assert_eq!(mode & 0o777, 0o600, "only the user can read the key");
        }
        std::fs::write(&path, b"short").unwrap();
        assert!(Identity::load_or_create(&path).is_err());
        let _ = std::fs::remove_dir_all(dir);
    }
}
