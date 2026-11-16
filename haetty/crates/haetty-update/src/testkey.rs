//! TEST ONLY (feature `test-support`, never in a shipped build): a fixed ed25519 key that signs the tests' manifests
//! in minisign's format (prehashed "ED"), so the tests can make good, bad and foreign signatures. Its public half is
//! `tests/TEST-ONLY-minisign.pub`; a build with this feature trusts it (`sign::trusted`).

use base64::Engine as _;
use blake2::Digest as _;
use ed25519_dalek::{Signer, SigningKey};

/// The test key's seed: public on purpose; it signs nothing but test data.
pub const SEED: [u8; 32] = *b"TEST-ONLY haetty update key seed";
/// A second key, for "signed by another key".
pub const OTHER_SEED: [u8; 32] = *b"TEST-ONLY some other key seed..!";
const KEYNUM: [u8; 8] = *b"TESTONLY";
const OTHER_KEYNUM: [u8; 8] = *b"OTHERKEY";

fn b64(b: &[u8]) -> String {
    base64::engine::general_purpose::STANDARD.encode(b)
}

/// The public key line (`minisign.pub`'s second line) of a seed.
pub fn public_line(seed: &[u8; 32], other: bool) -> String {
    let pk = SigningKey::from_bytes(seed).verifying_key();
    let mut v = b"Ed".to_vec();
    v.extend(if other { OTHER_KEYNUM } else { KEYNUM });
    v.extend(pk.as_bytes());
    b64(&v)
}

/// A `.minisig` of `data` (prehashed), with `trusted` as the trusted comment.
pub fn sign_with(seed: &[u8; 32], other: bool, data: &[u8], trusted: &str) -> String {
    let sk = SigningKey::from_bytes(seed);
    let hash = blake2::Blake2b512::digest(data);
    let sig = sk.sign(&hash);
    let mut first = b"ED".to_vec();
    first.extend(if other { OTHER_KEYNUM } else { KEYNUM });
    first.extend(sig.to_bytes());
    let mut global = sig.to_bytes().to_vec();
    global.extend(trusted.as_bytes());
    let gsig = sk.sign(&global);
    format!(
        "untrusted comment: TEST-ONLY signature\n{}\ntrusted comment: {trusted}\n{}\n",
        b64(&first),
        b64(&gsig.to_bytes())
    )
}

/// Signed by the test key.
pub fn sign(data: &[u8]) -> String {
    sign_with(&SEED, false, data, "timestamp:0\tfile:manifest.json")
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The committed public file is this seed's key; good, changed and foreign signatures behave.
    #[test]
    fn test_key_signs_and_verifies() {
        let file = include_str!("../tests/TEST-ONLY-minisign.pub");
        assert_eq!(crate::sign::keys_in(file), [public_line(&SEED, false)], "regenerate tests/TEST-ONLY-minisign.pub");
        let keys = crate::sign::trusted();
        let data = b"{\"format\":1}";
        assert_eq!(crate::sign::verify(data, &sign(data), &keys), Ok(()));
        assert!(crate::sign::verify(b"{\"format\":2}", &sign(data), &keys).is_err(), "changed data");
        let foreign = sign_with(&OTHER_SEED, true, data, "x");
        assert!(crate::sign::verify(data, &foreign, &keys).is_err(), "another key");
        assert!(crate::sign::verify(data, "", &keys).is_err(), "no signature");
        // The legacy unhashed form is refused.
        let legacy = sign(data).replacen("\nRURU", "\nRWRU", 1);
        assert_ne!(legacy, sign(data));
        assert!(crate::sign::verify(data, &legacy, &keys).is_err());
    }
}
