use base64::engine::general_purpose::STANDARD as B64;
use base64::Engine;
use ssh_key::private::{Ed25519Keypair, Ed25519PrivateKey, KeypairData, PrivateKey};
use ssh_key::public::Ed25519PublicKey;
use ssh_key::LineEnding;

use crate::error::{Error, Result};

/// Convert any supported private key format to OpenSSH PEM.
///
/// Converter chain, tried in order:
/// 1. Passthrough — already OpenSSH format
/// 2. 1Password Ed25519 — PKCS#8 v2 (OneAsymmetricKey, RFC 8410)
/// 3. Standard Ed25519 PKCS#8 v1
pub fn to_openssh(pem: &str) -> Result<String> {
    let stripped = pem.trim();

    if stripped.contains("BEGIN OPENSSH PRIVATE KEY") {
        return Ok(format!("{stripped}\n"));
    }

    let converters: &[fn(&str) -> Result<String>] =
        &[convert_1password_ed25519, convert_standard_ed25519_pkcs8];

    for converter in converters {
        if let Ok(key) = converter(stripped) {
            return Ok(key);
        }
    }

    Err(Error::Runtime(
        "Unsupported key format. Expected: OpenSSH, PKCS#8 PEM (Ed25519), \
         or 1Password Ed25519 export."
            .into(),
    ))
}

/// Decode PEM body to DER bytes.
fn pem_to_der(raw: &str) -> Result<Vec<u8>> {
    let b64_content: String = raw.lines().filter(|l| !l.starts_with("-----")).collect();
    B64.decode(&b64_content)
        .map_err(|e| Error::Runtime(format!("base64 decode: {e}")))
}

/// Check if DER contains Ed25519 OID (1.3.101.112 → bytes 2b 65 70).
fn is_ed25519_der(der: &[u8]) -> bool {
    der.get(..20)
        .is_some_and(|h| h.windows(3).any(|w| w == [0x2b, 0x65, 0x70]))
}

/// Extract the 32-byte Ed25519 private seed from DER.
///
/// Works for both PKCS#8 v1 and v2. The seed is wrapped as:
///   OCTET STRING (0x04, 0x22) { OCTET STRING (0x04, 0x20) { 32 bytes } }
fn extract_ed25519_seed(der: &[u8]) -> Result<[u8; 32]> {
    let marker = [0x04, 0x22, 0x04, 0x20];
    let idx = der
        .windows(4)
        .position(|w| w == marker)
        .ok_or_else(|| Error::Runtime("Could not find Ed25519 seed in DER".into()))?;
    let slice = der
        .get(idx + 4..idx + 4 + 32)
        .ok_or_else(|| Error::Runtime("Could not extract 32-byte Ed25519 seed".into()))?;
    let mut seed = [0u8; 32];
    seed.copy_from_slice(slice);
    Ok(seed)
}

/// Build an OpenSSH PEM string from a 32-byte Ed25519 seed.
fn ed25519_seed_to_openssh(seed: [u8; 32]) -> Result<String> {
    let signing_key = ed25519_dalek::SigningKey::from_bytes(&seed);
    let verifying_key = signing_key.verifying_key();

    let keypair = Ed25519Keypair {
        public: Ed25519PublicKey(verifying_key.to_bytes()),
        private: Ed25519PrivateKey::from_bytes(&seed),
    };
    let private_key = PrivateKey::new(KeypairData::Ed25519(keypair), "")
        .map_err(|e| Error::Runtime(format!("constructing SSH key: {e}")))?;
    let openssh = private_key
        .to_openssh(LineEnding::LF)
        .map_err(|e| Error::Runtime(format!("serializing to OpenSSH: {e}")))?;
    Ok(openssh.to_string())
}

/// 1Password Ed25519 PKCS#8 v2 (OneAsymmetricKey, RFC 8410).
///
/// 1Password exports Ed25519 keys as PKCS#8 v2 which includes an optional
/// public key field. Most crypto libraries can't parse this variant, so we
/// extract the 32-byte private seed directly from the DER and reconstruct.
fn convert_1password_ed25519(raw: &str) -> Result<String> {
    if !raw
        .lines()
        .next()
        .is_some_and(|l| l.starts_with("-----BEGIN PRIVATE KEY"))
    {
        return Err(Error::Runtime("Not a PKCS#8 PEM".into()));
    }

    let der = pem_to_der(raw)?;

    if !is_ed25519_der(&der) {
        return Err(Error::Runtime("Not an Ed25519 key".into()));
    }

    let seed = extract_ed25519_seed(&der)?;
    ed25519_seed_to_openssh(seed)
}

/// Standard Ed25519 PKCS#8 v1 PEM (from OpenSSL, cloud KMS, etc).
fn convert_standard_ed25519_pkcs8(raw: &str) -> Result<String> {
    let first_line = raw
        .lines()
        .next()
        .ok_or_else(|| Error::Runtime("empty key".into()))?;

    if !first_line.starts_with("-----BEGIN") {
        return Err(Error::Runtime("Not a PEM key".into()));
    }

    let der = pem_to_der(raw)?;

    if !is_ed25519_der(&der) {
        return Err(Error::Runtime("Not an Ed25519 key".into()));
    }

    let seed = extract_ed25519_seed(&der)?;
    ed25519_seed_to_openssh(seed)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn generate_openssh_key() -> String {
        let seed = [42u8; 32];
        ed25519_seed_to_openssh(seed).unwrap()
    }

    fn generate_pkcs8_v1_pem() -> String {
        let seed = [42u8; 32];

        // PKCS#8 v1 DER for Ed25519: SEQUENCE { SEQUENCE { OID }, OCTET STRING { OCTET STRING { seed } } }
        let mut der = Vec::new();
        // Inner: algorithm identifier SEQUENCE { OID 1.3.101.112 }
        let oid_seq = [0x30, 0x05, 0x06, 0x03, 0x2b, 0x65, 0x70];
        // Private key: OCTET STRING wrapping OCTET STRING wrapping 32-byte seed
        let mut priv_key = vec![0x04, 0x22, 0x04, 0x20];
        priv_key.extend_from_slice(&seed);
        // Outer SEQUENCE
        // version INTEGER 0
        let version = [0x02, 0x01, 0x00];
        let inner_len = version.len() + oid_seq.len() + priv_key.len();
        der.push(0x30);
        der.push(inner_len as u8);
        der.extend_from_slice(&version);
        der.extend_from_slice(&oid_seq);
        der.extend_from_slice(&priv_key);

        let b64 = B64.encode(&der);
        let mut lines = vec!["-----BEGIN PRIVATE KEY-----".to_string()];
        for chunk in b64.as_bytes().chunks(64) {
            lines.push(String::from_utf8_lossy(chunk).to_string());
        }
        lines.push("-----END PRIVATE KEY-----".to_string());
        lines.join("\n")
    }

    fn generate_pkcs8_v2_pem() -> String {
        let seed = [42u8; 32];
        let signing_key = ed25519_dalek::SigningKey::from_bytes(&seed);
        let pub_bytes = signing_key.verifying_key().to_bytes();

        // PKCS#8 v2 — same as v1 but with explicit public key context tag [1]
        let mut der = Vec::new();
        let oid_seq = [0x30, 0x05, 0x06, 0x03, 0x2b, 0x65, 0x70];
        let mut priv_key = vec![0x04, 0x22, 0x04, 0x20];
        priv_key.extend_from_slice(&seed);
        // version INTEGER 1 (v2)
        let version = [0x02, 0x01, 0x01];
        // public key context [1] EXPLICIT BIT STRING
        let mut pub_ctx = vec![0xa1, 0x23, 0x03, 0x21, 0x00];
        pub_ctx.extend_from_slice(&pub_bytes);

        let inner_len = version.len() + oid_seq.len() + priv_key.len() + pub_ctx.len();
        der.push(0x30);
        der.push(inner_len as u8);
        der.extend_from_slice(&version);
        der.extend_from_slice(&oid_seq);
        der.extend_from_slice(&priv_key);
        der.extend_from_slice(&pub_ctx);

        let b64 = B64.encode(&der);
        let mut lines = vec!["-----BEGIN PRIVATE KEY-----".to_string()];
        for chunk in b64.as_bytes().chunks(64) {
            lines.push(String::from_utf8_lossy(chunk).to_string());
        }
        lines.push("-----END PRIVATE KEY-----".to_string());
        lines.join("\n")
    }

    #[test]
    fn test_openssh_passthrough() {
        let key = generate_openssh_key();
        let result = to_openssh(&key).unwrap();
        assert!(result.contains("BEGIN OPENSSH PRIVATE KEY"));
        assert!(result.ends_with('\n'));
    }

    #[test]
    fn test_openssh_passthrough_with_whitespace() {
        let key = format!("  \n{}\n  ", generate_openssh_key());
        let result = to_openssh(&key).unwrap();
        assert!(result.contains("BEGIN OPENSSH PRIVATE KEY"));
    }

    #[test]
    fn test_pkcs8_v1_conversion() {
        let pem = generate_pkcs8_v1_pem();
        let result = to_openssh(&pem).unwrap();
        assert!(result.contains("BEGIN OPENSSH PRIVATE KEY"));
    }

    #[test]
    fn test_pkcs8_v2_1password_conversion() {
        let pem = generate_pkcs8_v2_pem();
        let result = to_openssh(&pem).unwrap();
        assert!(result.contains("BEGIN OPENSSH PRIVATE KEY"));
    }

    #[test]
    fn test_same_seed_produces_same_key() {
        let v1 = to_openssh(&generate_pkcs8_v1_pem()).unwrap();
        let v2 = to_openssh(&generate_pkcs8_v2_pem()).unwrap();
        // Both use seed [42; 32], so the resulting OpenSSH keys should be identical
        assert_eq!(v1, v2);
    }

    #[test]
    fn test_unsupported_format() {
        let result = to_openssh("not a key at all");
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("Unsupported"));
    }

    #[test]
    fn test_rsa_key_rejected() {
        // RSA PEM header but not Ed25519
        let fake = "-----BEGIN RSA PRIVATE KEY-----\nMIIB=\n-----END RSA PRIVATE KEY-----";
        let result = to_openssh(fake);
        assert!(result.is_err());
    }

    #[test]
    fn test_ed25519_seed_roundtrip() {
        let seed = [7u8; 32];
        let openssh = ed25519_seed_to_openssh(seed).unwrap();
        assert!(openssh.contains("BEGIN OPENSSH PRIVATE KEY"));

        // Parse it back and verify the public key matches
        let signing_key = ed25519_dalek::SigningKey::from_bytes(&seed);
        let expected_pub = signing_key.verifying_key().to_bytes();
        let parsed = PrivateKey::from_openssh(&openssh).unwrap();
        if let KeypairData::Ed25519(kp) = parsed.key_data() {
            assert_eq!(kp.public.0, expected_pub);
        } else {
            panic!("Expected Ed25519 key data");
        }
    }

    #[test]
    fn test_ssh_agent_integration() {
        // Only run if ssh-agent is available
        let agent = std::process::Command::new("ssh-agent").arg("-s").output();
        let agent = match agent {
            Ok(out) if out.status.success() => out,
            _ => return, // skip if no ssh-agent
        };

        let stdout = String::from_utf8_lossy(&agent.stdout);
        let mut env: std::collections::HashMap<String, String> = std::collections::HashMap::new();
        for line in stdout.lines() {
            if let Some((var_eq, _)) = line.split_once(';') {
                if let Some((var, val)) = var_eq.split_once('=') {
                    env.insert(var.trim().to_string(), val.to_string());
                }
            }
        }

        let agent_pid = env.get("SSH_AGENT_PID").cloned();

        // Generate a key and add it to the agent
        let key = generate_openssh_key();
        let add = std::process::Command::new("ssh-add")
            .arg("-")
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .envs(&env)
            .spawn()
            .and_then(|mut child| {
                if let Some(ref mut stdin) = child.stdin {
                    std::io::Write::write_all(stdin, key.as_bytes()).unwrap();
                }
                child.wait_with_output()
            })
            .unwrap();

        assert!(
            add.status.success(),
            "ssh-add failed: {}",
            String::from_utf8_lossy(&add.stderr)
        );

        // Verify the key is listed
        let list = std::process::Command::new("ssh-add")
            .arg("-l")
            .envs(&env)
            .output()
            .unwrap();
        assert!(list.status.success());
        let list_out = String::from_utf8_lossy(&list.stdout);
        assert!(
            list_out.contains("256"),
            "Expected ed25519 key in agent listing"
        );

        // Cleanup
        if let Some(pid) = agent_pid {
            let _ = std::process::Command::new("kill").arg(&pid).status();
        }
    }
}
