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

/// Decodes Base64url, with or without padding. `None` if the text is not Base64url.
#[must_use]
pub fn base64url_decode(text: &str) -> Option<Vec<u8>> {
    let text = text.trim_end_matches('=');
    let mut out = Vec::with_capacity(text.len() * 3 / 4);
    let (mut buffer, mut bits) = (0_u32, 0_u32);
    for byte in text.bytes() {
        let sextet = URL_SAFE.iter().position(|candidate| *candidate == byte)?;
        buffer = (buffer << 6) | u32::try_from(sextet).ok()?;
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push(u8::try_from((buffer >> bits) & 0xff).ok()?);
        }
    }
    Some(out)
}

/// HMAC-SHA256 (RFC 2104), for forging the HS256 token of the algorithm-confusion attack.
#[must_use]
pub fn hmac_sha256(key: &[u8], message: &[u8]) -> [u8; 32] {
    const BLOCK: usize = 64;
    let mut block = [0_u8; BLOCK];
    if key.len() > BLOCK {
        block[..32].copy_from_slice(&Sha256::digest(key));
    } else {
        block[..key.len()].copy_from_slice(key);
    }
    let inner = Sha256::new()
        .chain_update(block.map(|byte| byte ^ 0x36))
        .chain_update(message)
        .finalize();
    Sha256::new()
        .chain_update(block.map(|byte| byte ^ 0x5c))
        .chain_update(inner)
        .finalize()
        .into()
}

/// The value of the query parameter `name` of `url`, percent-decoded (`+` is a space).
#[must_use]
pub fn query_parameter(url: &str, name: &str) -> Option<String> {
    let query = url.split_once('?')?.1;
    query.split('&').find_map(|pair| {
        let (key, value) = pair.split_once('=')?;
        (key == name).then(|| percent_decode(value))
    })
}

fn percent_decode(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%'
            && let Some(byte) = text.get(i + 1..i + 3).and_then(|hex| u8::from_str_radix(hex, 16).ok())
        {
            out.push(byte);
            i += 3;
            continue;
        }
        out.push(if bytes[i] == b'+' { b' ' } else { bytes[i] });
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
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
    fn base64url_decoding_reverses_encoding() {
        for bytes in [&b""[..], b"f", b"fo", b"foo", b"foob", &[0xfb, 0xff, 0x00, 0x10]] {
            assert_eq!(base64url_decode(&base64url_unpadded(bytes)).as_deref(), Some(bytes));
        }
        assert_eq!(base64url_decode("Zm9v="), Some(b"foo".to_vec()), "padding is accepted");
        assert_eq!(base64url_decode("a+b"), None, "the standard alphabet is not Base64url");
    }

    /// RFC 4231, test cases 2 and 6 (a key longer than a block).
    #[test]
    fn hmac_sha256_matches_the_rfc_vectors() {
        assert_eq!(
            hex(&hmac_sha256(b"Jefe", b"what do ya want for nothing?")),
            "5bdcc146bf60754e6a042426089575c75a003f089d2739839dec58b964ec3843"
        );
        assert_eq!(
            hex(&hmac_sha256(
                &[0xaa; 131],
                b"Test Using Larger Than Block-Size Key - Hash Key First"
            )),
            "60e431591ee0b67f0d8a26aacbf5b77f8e0bc6213728c5140546040f0ee37f54"
        );
    }

    #[test]
    fn query_parameters_are_percent_decoded() {
        let url =
            "http://s3/b/k?response-content-disposition=attachment%3B%20filename%3D%22passwd.pdf%22&X-Amz-Expires=300";
        assert_eq!(
            query_parameter(url, "response-content-disposition").as_deref(),
            Some("attachment; filename=\"passwd.pdf\"")
        );
        assert_eq!(query_parameter(url, "X-Amz-Expires").as_deref(), Some("300"));
        assert_eq!(query_parameter(url, "missing"), None);
        assert_eq!(query_parameter("http://x/?a=1+2%2", "a").as_deref(), Some("1 2%2"));
    }

    #[test]
    fn sha256_of_abc_is_the_fips_vector() {
        assert_eq!(
            sha256_hex(b"abc"),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }
}
