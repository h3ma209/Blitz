//! PKCS#12 (.p12/.pfx) → PEM conversion for reqwest rustls Identity.
//! Best-effort: uses OpenSSL CLI when available; otherwise clear error.

use std::path::Path;
use std::process::Command;

/// Load client identity bytes suitable for `reqwest::Identity::from_pem`.
/// Accepts PEM directly, or PKCS#12 via `openssl pkcs12` conversion.
pub fn load_client_identity_pem(path: &str, passphrase: &str) -> Result<Vec<u8>, String> {
    let path = path.trim();
    if path.is_empty() {
        return Err("empty cert path".into());
    }
    let lower = path.to_ascii_lowercase();
    let raw = std::fs::read(path).map_err(|e| format!("read {path}: {e}"))?;

    if lower.ends_with(".p12") || lower.ends_with(".pfx") {
        return pkcs12_to_pem(&raw, passphrase, path);
    }

    // PEM (or unknown) — pass through
    if raw.windows(11).any(|w| w == b"BEGIN CERTI") || raw.windows(10).any(|w| w == b"BEGIN PRIV")
    {
        return Ok(raw);
    }

    // Try as PKCS#12 anyway
    match pkcs12_to_pem(&raw, passphrase, path) {
        Ok(pem) => Ok(pem),
        Err(_) => Ok(raw), // let Identity::from_pem report
    }
}

fn pkcs12_to_pem(der: &[u8], passphrase: &str, src: &str) -> Result<Vec<u8>, String> {
    let tmp_dir = std::env::temp_dir().join("evilpostman-pkcs12");
    std::fs::create_dir_all(&tmp_dir).map_err(|e| e.to_string())?;
    let p12_path = tmp_dir.join(format!("{}.p12", uuid::Uuid::new_v4()));
    let pem_path = tmp_dir.join(format!("{}.pem", uuid::Uuid::new_v4()));
    std::fs::write(&p12_path, der).map_err(|e| e.to_string())?;

    let pass = if passphrase.is_empty() { "pass:" } else { passphrase };
    let pass_arg = if passphrase.is_empty() {
        "pass:".to_string()
    } else {
        format!("pass:{passphrase}")
    };
    let _ = pass;

    let out = Command::new("openssl")
        .args([
            "pkcs12",
            "-in",
            p12_path.to_str().unwrap_or("in.p12"),
            "-out",
            pem_path.to_str().unwrap_or("out.pem"),
            "-nodes",
            "-passin",
            &pass_arg,
        ])
        .output();

    let _ = std::fs::remove_file(&p12_path);

    match out {
        Ok(o) if o.status.success() => {
            let pem = std::fs::read(&pem_path).map_err(|e| e.to_string())?;
            let _ = std::fs::remove_file(&pem_path);
            if pem.is_empty() {
                Err(format!(
                    "PKCS#12 {src}: openssl produced empty PEM"
                ))
            } else {
                Ok(pem)
            }
        }
        Ok(o) => {
            let err = String::from_utf8_lossy(&o.stderr);
            Err(format!(
                "PKCS#12 {src}: openssl failed — {err}. Install openssl or convert to PEM (cert+key)."
            ))
        }
        Err(e) => Err(format!(
            "PKCS#12 {src}: openssl not available ({e}). Convert with: openssl pkcs12 -in cert.p12 -out cert.pem -nodes"
        )),
    }
}

/// True when path looks like PKCS#12.
pub fn looks_like_pkcs12(path: &str) -> bool {
    let p = Path::new(path);
    p.extension()
        .and_then(|e| e.to_str())
        .map(|e| e.eq_ignore_ascii_case("p12") || e.eq_ignore_ascii_case("pfx"))
        .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detect_p12() {
        assert!(looks_like_pkcs12("a.p12"));
        assert!(looks_like_pkcs12("b.PFX"));
        assert!(!looks_like_pkcs12("c.pem"));
    }
}
