//! Decode compressed HTTP response bodies when `Content-Encoding` is still present.
//!
//! reqwest strips that header after it inflates. Sniffing gzip magic on the
//! result inflates a second time when the payload itself starts with gzip bytes.

use std::io::Read;

/// Inflate only when `Content-Encoding` is still present.
pub fn finalize_response_body(
    bytes: &[u8],
    headers: &[(String, String)],
    log: &mut Vec<String>,
) -> String {
    if content_encoding(headers).is_some() {
        decode_response_body(bytes, headers, log)
    } else {
        String::from_utf8_lossy(bytes).into_owned()
    }
}

/// Decode `bytes` using `Content-Encoding` and return UTF-8 text.
/// Logs `encoding: decompressed {codec}` when decompression runs.
pub fn decode_response_body(
    bytes: &[u8],
    headers: &[(String, String)],
    log: &mut Vec<String>,
) -> String {
    let encoding = content_encoding(headers);
    let (codec, decoded) = match encoding.as_deref() {
        Some("gzip") => ("gzip", try_gzip(bytes)),
        Some("deflate") => ("deflate", try_deflate(bytes)),
        Some("br") => ("br", try_brotli(bytes)),
        Some(other) if other.contains("gzip") => ("gzip", try_gzip(bytes)),
        _ => {
            return String::from_utf8_lossy(bytes).into_owned();
        }
    };

    match decoded {
        Some(plain) => {
            log.push(format!("encoding: decompressed {codec}"));
            String::from_utf8_lossy(&plain).into_owned()
        }
        None => String::from_utf8_lossy(bytes).into_owned(),
    }
}

fn content_encoding(headers: &[(String, String)]) -> Option<String> {
    headers
        .iter()
        .find(|(k, _)| k.eq_ignore_ascii_case("content-encoding"))
        .map(|(_, v)| v.trim().to_ascii_lowercase())
        .filter(|v| !v.is_empty())
}

#[cfg(test)]
fn looks_like_gzip(bytes: &[u8]) -> bool {
    bytes.len() >= 2 && bytes[0] == 0x1f && bytes[1] == 0x8b
}

const MAX_DECOMPRESSED_BYTES: u64 = 64 * 1024 * 1024;

fn try_gzip(bytes: &[u8]) -> Option<Vec<u8>> {
    let decoder = flate2::read::GzDecoder::new(bytes);
    let mut out = Vec::new();
    decoder.take(MAX_DECOMPRESSED_BYTES).read_to_end(&mut out).ok()?;
    if out.is_empty() && !bytes.is_empty() {
        // Empty payload is valid gzip; keep it.
    }
    Some(out)
}

fn try_deflate(bytes: &[u8]) -> Option<Vec<u8>> {
    // zlib-wrapped (common) then raw deflate
    {
        let decoder = flate2::read::ZlibDecoder::new(bytes);
        let mut out = Vec::new();
        if decoder.take(MAX_DECOMPRESSED_BYTES).read_to_end(&mut out).is_ok() {
            return Some(out);
        }
    }
    let decoder = flate2::read::DeflateDecoder::new(bytes);
    let mut out = Vec::new();
    decoder.take(MAX_DECOMPRESSED_BYTES).read_to_end(&mut out).ok()?;
    Some(out)
}

fn try_brotli(bytes: &[u8]) -> Option<Vec<u8>> {
    let mut out = Vec::new();
    let cursor = std::io::Cursor::new(bytes);
    let reader = brotli::Decompressor::new(cursor, 4096);
    reader.take(MAX_DECOMPRESSED_BYTES).read_to_end(&mut out).ok()?;
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use flate2::write::GzEncoder;
    use flate2::Compression;
    use std::io::Write;

    fn gzip_bytes(plain: &[u8]) -> Vec<u8> {
        let mut enc = GzEncoder::new(Vec::new(), Compression::default());
        enc.write_all(plain).unwrap();
        enc.finish().unwrap()
    }

    #[test]
    fn decompresses_gzip_when_content_encoding_set() {
        let plain = br#"{"ok":true,"msg":"snb"}"#;
        let compressed = gzip_bytes(plain);
        assert!(looks_like_gzip(&compressed));
        // Compressed bytes are not valid UTF-8 JSON as-is (contain binary).
        assert!(serde_json::from_slice::<serde_json::Value>(&compressed).is_err());

        let headers = vec![("content-encoding".into(), "gzip".into())];
        let mut log = vec![];
        let body = decode_response_body(&compressed, &headers, &mut log);

        assert_eq!(body, String::from_utf8_lossy(plain));
        assert!(log.iter().any(|l| l == "encoding: decompressed gzip"));
        let v: serde_json::Value = serde_json::from_str(&body).unwrap();
        assert_eq!(v["ok"], true);
    }

    #[test]
    fn gzip_magic_without_content_encoding_is_not_inflated_again() {
        let plain = b"hello gzip";
        let compressed = gzip_bytes(plain);
        let mut log = vec![];
        let body = finalize_response_body(&compressed, &[], &mut log);
        assert_eq!(body, String::from_utf8_lossy(&compressed));
        assert_ne!(body, String::from_utf8_lossy(plain));
        assert!(log.iter().all(|l| !l.contains("decompressed")));
    }

    #[test]
    fn gzip_decoded_once_and_nested_gzip_is_left_intact() {
        let plain = "z".repeat(64 * 1024);
        let inner = gzip_bytes(plain.as_bytes());
        let outer = gzip_bytes(&inner);
        let headers = vec![("content-encoding".into(), "gzip".into())];

        let mut log = vec![];
        let once = finalize_response_body(&outer, &headers, &mut log);
        assert_eq!(once, String::from_utf8_lossy(&inner));
        assert_ne!(once, plain);
        assert_eq!(log.iter().filter(|l| l.contains("decompressed")).count(), 1);

        let mut skipped = vec![];
        let still = finalize_response_body(&inner, &[], &mut skipped);
        assert_eq!(still, String::from_utf8_lossy(&inner));
        assert_ne!(still, plain);
        assert!(skipped.iter().all(|l| !l.contains("decompressed")));

        let mut plain_log = vec![];
        let same = finalize_response_body(plain.as_bytes(), &[], &mut plain_log);
        assert_eq!(same, plain);
        assert!(plain_log.is_empty());
    }

    #[test]
    fn leaves_plain_json_untouched() {
        let plain = br#"{"a":1}"#;
        let mut log = vec![];
        let body = decode_response_body(plain, &[], &mut log);
        assert_eq!(body, r#"{"a":1}"#);
        assert!(log.iter().all(|l| !l.contains("decompressed")));
    }
}
