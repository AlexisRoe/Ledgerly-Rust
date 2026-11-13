//! The encodings Ledgerly-Java stores data in (port spec §4), written out here so that the fixtures do not
//! depend on the code they test.

use sha2::{Digest, Sha256};

const STANDARD: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
const URL_SAFE: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";

fn base64(bytes: &[u8], alphabet: &[u8; 64]) -> String {
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let b = [chunk[0], *chunk.get(1).unwrap_or(&0), *chunk.get(2).unwrap_or(&0)];
        let sextets = [
            b[0] >> 2,
            (b[0] & 0x03) << 4 | b[1] >> 4,
            (b[1] & 0x0f) << 2 | b[2] >> 6,
            b[2] & 0x3f,
        ];
        for sextet in &sextets[..=chunk.len()] {
            out.push(char::from(alphabet[usize::from(*sextet)]));
        }
    }
    out
}

/// Standard Base64 without padding: the salt and hash of an Argon2 PHC string.
#[must_use]
pub fn base64_unpadded(bytes: &[u8]) -> String {
    base64(bytes, STANDARD)
}

/// Base64url without padding: opaque tokens.
#[must_use]
pub fn base64url_unpadded(bytes: &[u8]) -> String {
    base64(bytes, URL_SAFE)
}

/// Lower-case hex.
#[must_use]
pub fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

/// Lower-case hex SHA-256: how Java stores opaque tokens (of the token's UTF-8 text) and receipt checksums.
#[must_use]
pub fn sha256_hex(bytes: &[u8]) -> String {
    hex(&Sha256::digest(bytes))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// RFC 4648 §10, without the padding.
    #[test]
    fn base64_matches_the_rfc_vectors() {
        for (input, expected) in [
            ("", ""),
            ("f", "Zg"),
            ("fo", "Zm8"),
            ("foo", "Zm9v"),
            ("foob", "Zm9vYg"),
            ("fooba", "Zm9vYmE"),
            ("foobar", "Zm9vYmFy"),
        ] {
            assert_eq!(base64_unpadded(input.as_bytes()), expected);
        }
    }

    #[test]
    fn the_two_alphabets_differ_in_the_last_two_characters() {
        assert_eq!(base64_unpadded(&[0xfb, 0xff]), "+/8");
        assert_eq!(base64url_unpadded(&[0xfb, 0xff]), "-_8");
    }

    #[test]
    fn sha256_of_abc_is_the_fips_vector() {
        assert_eq!(
            sha256_hex(b"abc"),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }
}
