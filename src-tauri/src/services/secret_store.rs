//! Encrypts saved credentials at rest with Windows DPAPI.
//!
//! Tokens were stored in state.json as plain text, readable by anything that
//! can read the file (including a copy of the portable folder). DPAPI ties the
//! ciphertext to the Windows user account, so a copied folder is useless
//! elsewhere. The trade-off: moving the portable folder to another PC or user
//! means signing in again, which the loader reports rather than failing
//! silently.

use anyhow::{anyhow, Result};
use base64::Engine;

const PREFIX: &str = "dpapi:";

/// Encrypts `plain` for storage. Empty stays empty. Falls back to plain text
/// if DPAPI is unavailable, so a failure can never lose a credential.
pub fn protect(plain: &str) -> String {
    if plain.is_empty() || plain.starts_with(PREFIX) {
        return plain.to_string();
    }
    match platform::protect(plain.as_bytes()) {
        Ok(cipher) => format!(
            "{PREFIX}{}",
            base64::engine::general_purpose::STANDARD.encode(cipher)
        ),
        Err(_) => plain.to_string(),
    }
}

/// Decrypts a stored value. Values without the prefix are legacy plain text
/// and are returned unchanged (they get encrypted on the next save).
pub fn unprotect(stored: &str) -> Result<String> {
    let Some(encoded) = stored.strip_prefix(PREFIX) else {
        return Ok(stored.to_string());
    };
    let cipher = base64::engine::general_purpose::STANDARD
        .decode(encoded)
        .map_err(|_| anyhow!("stored credential is not valid base64"))?;
    let plain = platform::unprotect(&cipher)?;
    String::from_utf8(plain).map_err(|_| anyhow!("stored credential is not valid text"))
}

#[cfg(windows)]
mod platform {
    use anyhow::{anyhow, Result};
    use windows::{
        core::PCWSTR,
        Win32::{
            Foundation::{LocalFree, HLOCAL},
            Security::Cryptography::{
                CryptProtectData, CryptUnprotectData, CRYPTPROTECT_UI_FORBIDDEN,
                CRYPT_INTEGER_BLOB,
            },
        },
    };

    pub fn protect(plain: &[u8]) -> Result<Vec<u8>> {
        let input = CRYPT_INTEGER_BLOB {
            cbData: plain.len() as u32,
            pbData: plain.as_ptr() as *mut u8,
        };
        let mut output = CRYPT_INTEGER_BLOB::default();
        unsafe {
            CryptProtectData(
                &input,
                PCWSTR::null(),
                None,
                None,
                None,
                CRYPTPROTECT_UI_FORBIDDEN,
                &mut output,
            )
            .map_err(|error| anyhow!("DPAPI protect failed: {error}"))?;
            Ok(take_blob(output))
        }
    }

    pub fn unprotect(cipher: &[u8]) -> Result<Vec<u8>> {
        let input = CRYPT_INTEGER_BLOB {
            cbData: cipher.len() as u32,
            pbData: cipher.as_ptr() as *mut u8,
        };
        let mut output = CRYPT_INTEGER_BLOB::default();
        unsafe {
            CryptUnprotectData(
                &input,
                None,
                None,
                None,
                None,
                CRYPTPROTECT_UI_FORBIDDEN,
                &mut output,
            )
            .map_err(|error| anyhow!("DPAPI unprotect failed: {error}"))?;
            Ok(take_blob(output))
        }
    }

    /// Copies a DPAPI-allocated blob into Rust memory and frees the original.
    unsafe fn take_blob(blob: CRYPT_INTEGER_BLOB) -> Vec<u8> {
        if blob.pbData.is_null() {
            return Vec::new();
        }
        let bytes = unsafe { std::slice::from_raw_parts(blob.pbData, blob.cbData as usize) }.to_vec();
        unsafe {
            let _ = LocalFree(Some(HLOCAL(blob.pbData as *mut core::ffi::c_void)));
        }
        bytes
    }
}

#[cfg(not(windows))]
mod platform {
    use anyhow::{anyhow, Result};

    pub fn protect(_plain: &[u8]) -> Result<Vec<u8>> {
        Err(anyhow!("credential encryption is only available on Windows"))
    }

    pub fn unprotect(_cipher: &[u8]) -> Result<Vec<u8>> {
        Err(anyhow!("credential encryption is only available on Windows"))
    }
}

#[cfg(all(test, windows))]
mod tests {
    use super::*;

    #[test]
    fn round_trips_and_hides_the_token() {
        let stored = protect("oauth:abc123secret");
        assert!(stored.starts_with(PREFIX));
        assert!(!stored.contains("abc123secret"));
        assert_eq!(unprotect(&stored).unwrap(), "oauth:abc123secret");
    }

    #[test]
    fn legacy_plain_text_passes_through() {
        assert_eq!(unprotect("oauth:legacy").unwrap(), "oauth:legacy");
        assert_eq!(protect(""), "");
    }

    #[test]
    fn corrupted_ciphertext_is_an_error_not_a_panic() {
        assert!(unprotect("dpapi:bm90LWEtcmVhbC1ibG9i").is_err());
        assert!(unprotect("dpapi:***").is_err());
    }
}
