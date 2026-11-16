//! The manifest's signature (DESIGN.md §3.17): minisign's format, ed25519 over the BLAKE2b-512 of the file ("ED",
//! prehashed; the legacy unhashed "Ed" signatures are refused), checked with the pure-Rust `minisign-verify`. haetty
//! never signs: the person signs with minisign or rsign2 and their own secret key (`tools/release.sh`).
//!
//! The public keys are built in: `assets/update/haetty.pub` (one or more minisign public key files, concatenated;
//! empty until the person makes the key — such a build checks for updates but installs none). A build with the
//! `test-support` feature (the tests' builds only) also trusts the test key in `tests/TEST-ONLY-*`.

use minisign_verify::{PublicKey, Signature};

/// The release key(s), as minisign writes `minisign.pub` (comment lines are skipped).
const RELEASE_KEYS: &str = include_str!("../../../assets/update/haetty.pub");

/// The tests' key: never in a shipped build (`tests/update.rs` checks the dist binary for it).
#[cfg(feature = "test-support")]
pub const TEST_KEY: &str = include_str!("../tests/TEST-ONLY-minisign.pub");

/// The public keys in a `minisign.pub`-style text (base64 lines; `untrusted comment:` lines skipped).
pub fn keys_in(text: &str) -> Vec<String> {
    text.lines()
        .map(str::trim)
        .filter(|l| !l.is_empty() && !l.starts_with("untrusted comment:") && !l.starts_with('#'))
        .map(str::to_string)
        .collect()
}

/// The keys this build trusts.
pub fn trusted() -> Vec<String> {
    #[allow(unused_mut)]
    let mut k = keys_in(RELEASE_KEYS);
    #[cfg(feature = "test-support")]
    k.extend(keys_in(TEST_KEY));
    k
}

/// Whether a release key is built in (without one, updates are found but not installed).
pub fn have_release_key() -> bool {
    !trusted().is_empty()
}

/// Checks `sig_text` (a `.minisig` file) over `data` against `keys`: one key must match it.
pub fn verify(data: &[u8], sig_text: &str, keys: &[String]) -> Result<(), String> {
    if keys.is_empty() {
        return Err("no release key is built in: updates cannot be installed by this build".into());
    }
    if sig_text.len() > 4096 {
        return Err("signature file too large".into());
    }
    let sig = Signature::decode(sig_text).map_err(|e| format!("signature unreadable: {e}"))?;
    let mut last = String::from("no key");
    for k in keys {
        let pk = match PublicKey::from_base64(k) {
            Ok(pk) => pk,
            Err(e) => {
                last = format!("a built-in key is unreadable: {e}");
                continue;
            }
        };
        match pk.verify(data, &sig, false) {
            Ok(()) => return Ok(()),
            Err(e) => last = e.to_string(),
        }
    }
    Err(format!("signature does not match a release key ({last})"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keys_are_read_and_the_release_key_slot_is_empty_for_now() {
        assert_eq!(keys_in("untrusted comment: x\nRWQabc\n\n# note\nRWQdef\n"), ["RWQabc", "RWQdef"]);
        assert!(keys_in(RELEASE_KEYS).is_empty(), "the person's key is not made yet (SELF-HOSTING.md, Releases)");
        assert!(verify(b"x", "y", &[]).unwrap_err().contains("no release key"));
    }
}
