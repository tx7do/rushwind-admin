//! Credential crypto primitives:
//! the password transfer layer — the `rsa-v1` hybrid envelope
//! (RSA-2048 OAEP(SHA-256) wrapping an AES-256-GCM session key, the
//! P1-12 sync) with the legacy `base64(AES-128-CBC(key, iv=key,
//! PKCS#7))` form accepted through the transition — plus bcrypt
//! hashing (DEFAULT_COST = 12, the P3 bcrypt12 posture), SHA-256 (access-key secrets),
//! AES-256-GCM (MFA secret at-rest, `enc:` prefix), and RFC 6238 TOTP.

use std::sync::OnceLock;

use aes::cipher::{block_padding::Pkcs7, BlockDecryptMut, KeyIvInit};
use rsa::pkcs8::DecodePrivateKey;
use sha2::{Digest, Sha256};

type Aes128CbcDec = cbc::Decryptor<aes::Aes128>;

/// The default AES key — 16 bytes, also reused as the IV.
pub const DEFAULT_AES_KEY: &[u8; 16] = b"f51d66a73d8a0927";

/// The AES-256-GCM key for `enc:` secrets (MFA factors): derived from
/// env `RUSHWIND_CRYPTO_KEY`; unset ⇒ factors are stored in plaintext.
pub fn crypto_key() -> Option<[u8; 32]> {
    let raw = std::env::var("RUSHWIND_CRYPTO_KEY").ok()?;
    let mut key = [0u8; 32];
    let bytes = raw.as_bytes();
    key[..bytes.len().min(32)].copy_from_slice(&bytes[..bytes.len().min(32)]);
    Some(key)
}

/// Envelope format version prefix: `rsa-v1:<b64(wrappedKey)>:<b64(nonce)>:<b64(cipher)>`.
pub const ENVELOPE_PREFIX: &str = "rsa-v1:";

/// Overrides the built-in development private key (PEM; a single-line
/// form with `\n` escapes is accepted).
pub const ENV_PASSWORD_RSA_PRIVATE_KEY: &str = "RUSHWIND_PASSWORD_RSA_PRIVATE_KEY";

/// The development default RSA-2048 keypair (public in-repo; the same
/// public half is baked into the three frontend snapshots as
/// `VITE_PASSWORD_RSA_PUB`). Production must override the private half
/// via [`ENV_PASSWORD_RSA_PRIVATE_KEY`] — anyone holding this repo can
/// otherwise decrypt password envelopes.
const DEV_PRIVATE_PEM: &str = "-----BEGIN PRIVATE KEY-----
MIIEuwIBADANBgkqhkiG9w0BAQEFAASCBKUwggShAgEAAoIBAQCiQ/+NZXAkXN3F
SVvq+eg07sxK1nKzLjTrsaEZ6Lbl8mLIGQkJmXAfouVy0FvIaWrC9yDmSD4t+xDH
127h51wyq29IGJ6omOmOsgE2qDcMZ21Z0O3hI+1DWWSeLl3HAHVMfjBotvxOdWOk
na/lwGTKZR2IJ/cZGrtTk8hatg79vvH4fSf5rsnIpD8TO8ptkLjXtiA9/ETJwLNY
gTmSPgRFYw3cVfIhxowS3XA84X4GgLedqfDswNbQrPiJVchjkUvzIipz1C0Gd9DW
DHB15QkjsAeR5fOEexJSfWOIwMHl6fjasGoMp+D7x8jEpyjHyz+wrKhg45mzzz7D
InLLSQgLAgMBAAECgf90rAImKNv5SV8HsqVShfCC+lV8VWxIIJXIB0vhHmCtGojs
V10V5U/wStTUpteg5wbKcebaiPK+09lz16oZey8GrYaxq5Ro7q5uRyGYPX/pois1
DfKPJjOqAopon+4W8ZjTeZ8whz1Q44TUM7IPJTTBLc6yvFLdFZJSvLj3W8vZXTkH
mtXiAx6MEphT1uRMubo+tvOH7FJheH+c9L8NHVxbt5k5rQ7oGlxurEOw35+T5Cka
ALDuNnz+A+Zm7354tsJJnwsSxOoEDVXhb8gzXEE+NDw3Tzdj9TgL6RgBwCIgRTsO
dxnV8sf3yG/waydx296A7V8LXScEh3ZZrcxKr9UCgYEA44wgOBwPfN+jAxQWaU2v
P1ae5E0ZLv8PlS/QFp9ZCwcF81Kxn16jgQIs7BZW0JiYomxu8cMxJnKxmw7TcbIb
EzJBsxKI+ltgtGwfbWkDRStOmya15mLmffG+/OLgclUMJklH3Tl3PgVtA0YpK2pR
M3RUq+U8ynaSi9FbCS44lf8CgYEAto4uCYM8Wrw7vypZ0vlF3qx7yqZCKPBuh3FG
C2UhZUOeoQZtydsNaX0fNKtEZ3ZUh3GTGhO9mQjxFZJhgIufOgrz39BrGoeSzD2K
JAoAbJsOzXxpKqe+2ITsGd+cXaUqnPdbarrh5Va5kx4iRK7RhwNOuRIGGei+dxxM
hV7MhfUCgYBVIYzAUUc+GdE8Cpzj4suI79hcY7fjTEvBLfU1MYK3zHXedfa8QET9
tBmobicmvaCFS/op+zP9RXXLJD3ZUKAui2PnIi0bNWq1taoAXgR6MVykUVRGkIPZ
ZMNmi18bh1QXUt1PF/vTuWXyjqJCevFMRd6TvR0ERl5HbxkXEzLacwKBgGsNtbGe
ZCZRfDf+fe5af761J2MPXHzlgeurYYgNJihcHwoO2xMkghj6ZvJb/SEvIyLKV5lT
OjwkIjykOslHF3jSM3Fsp2JHXq6m5/t6YgVOaL/X1VROSnxudoY1u1yvHu4toE5W
iWIUGX9MjIv6eKTUK7Yb+7f15Lu0vKr5jLaBAoGBAL1tmIMq94ggsqcJpI4/LcSs
F85pT2iSzOmoLYzg2x4QNH7tNuqn8yjsX/4voNqXkrT1g7t/Tpm64Kps+13s77gG
T8jcwfRVNn0lhIFj7g2mLFagec8G7Q5mlzumDTzXRj+Sw3SPOVip6a9p7wn24TG9
imwTPhL0crfiqbxJJAu6
-----END PRIVATE KEY-----";

/// The dev private half as a parsed key (None on parse failure —
/// decrypt then answers "private key not loaded" per the reference).
fn parse_private_pem(pem: &str) -> Option<rsa::RsaPrivateKey> {
    rsa::RsaPrivateKey::from_pkcs8_pem(pem.trim()).ok()
}

/// The password-envelope private key: env override wins, the dev pair
/// is the fallback. Parsed once per process.
fn password_rsa_private() -> &'static Option<rsa::RsaPrivateKey> {
    static KEY: OnceLock<Option<rsa::RsaPrivateKey>> = OnceLock::new();
    KEY.get_or_init(|| {
        let raw = std::env::var(ENV_PASSWORD_RSA_PRIVATE_KEY).unwrap_or_default();
        let raw = raw.trim();
        let pem: &'static str = if raw.is_empty() {
            DEV_PRIVATE_PEM
        } else {
            // env single-line form carries `\n` escapes
            Box::leak(raw.replace("\\n", "\n").into_boxed_str())
        };
        parse_private_pem(pem)
    })
}

/// Whether the running process still decrypts envelopes with the
/// in-repo development private key (cryptographic equality). Startup
/// warns on this: production must override via env, or anyone holding
/// the source can decrypt password envelopes.
pub fn using_dev_password_private_key() -> bool {
    let Some(key) = password_rsa_private() else {
        return false;
    };
    let Some(dev) = parse_private_pem(DEV_PRIVATE_PEM) else {
        return false;
    };
    use rsa::pkcs8::EncodePrivateKey;
    match (key.to_pkcs8_der(), dev.to_pkcs8_der()) {
        (Ok(a), Ok(b)) => a.as_bytes() == b.as_bytes(),
        _ => false,
    }
}

/// The unified password-transfer decrypt entry (login / change
/// password / create user / reset): an `rsa-v1:` prefix dispatches to
/// the hybrid envelope; anything else is the legacy
/// `base64(AES-128-CBC, key=IV=DefaultAESKey)` form, accepted through
/// the frontend rollout transition. Format/decrypt failures answer
/// Err — callers surface them as 400.
pub fn decrypt_transfer_password(v: &str) -> Result<String, String> {
    if let Some(rest) = v.strip_prefix(ENVELOPE_PREFIX) {
        return decrypt_password_envelope(rest);
    }
    decrypt_legacy(v)
}

/// The hybrid envelope: split into wrappedKey/nonce/cipher, unwrap the
/// AES-256-GCM session key with RSA-OAEP(SHA-256), decrypt the
/// password. The frontend (WebCrypto) emits exactly this shape.
fn decrypt_password_envelope(rest: &str) -> Result<String, String> {
    use base64::Engine as _;
    let Some(key) = password_rsa_private() else {
        return Err("passwordcrypto: private key not loaded".into());
    };
    let parts: Vec<&str> = rest.split(':').collect();
    let [wrapped_b64, nonce_b64, cipher_b64] = parts.as_slice() else {
        return Err("passwordcrypto: malformed envelope".into());
    };
    let wrapped = base64::engine::general_purpose::STANDARD
        .decode(wrapped_b64)
        .map_err(|_| "passwordcrypto: invalid envelope base64")?;
    let nonce = base64::engine::general_purpose::STANDARD
        .decode(nonce_b64)
        .map_err(|_| "passwordcrypto: invalid envelope base64")?;
    let cipher = base64::engine::general_purpose::STANDARD
        .decode(cipher_b64)
        .map_err(|_| "passwordcrypto: invalid envelope base64")?;

    use rsa::Oaep;
    let session = key
        .decrypt(Oaep::new::<Sha256>(), &wrapped)
        .map_err(|_| "passwordcrypto: envelope unwrap".to_string())?;
    if session.len() != 32 {
        return Err("passwordcrypto: session key size".into());
    }
    if nonce.len() != 12 {
        return Err("passwordcrypto: nonce size".into());
    }
    use aes_gcm::aead::generic_array::GenericArray;
    use aes_gcm::aead::Aead;
    use aes_gcm::{Aes256Gcm, KeyInit};
    let gcm = Aes256Gcm::new_from_slice(&session)
        .map_err(|_| "passwordcrypto: session key size".to_string())?;
    let plain = gcm
        .decrypt(GenericArray::from_slice(&nonce), cipher.as_ref())
        .map_err(|_| "passwordcrypto: envelope decrypt".to_string())?;
    String::from_utf8(plain).map_err(|_| "passwordcrypto: envelope utf8".into())
}

/// The legacy frontend form: base64(AES-128-CBC(key, iv=key, PKCS7)).
fn decrypt_legacy(v: &str) -> Result<String, String> {
    use base64::Engine as _;
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(v.trim())
        .map_err(|_| "passwordcrypto: invalid legacy base64".to_string())?;
    decrypt_aes_cbc(&bytes).ok_or_else(|| "passwordcrypto: legacy decrypt".into())
}

/// AES-128-CBC decrypt with the DefaultAESKey pair (bytes in). The
/// cipher build lacks the `alloc` pairing helper, so the buffer form runs
/// explicitly.
pub fn decrypt_aes_cbc(ciphertext: &[u8]) -> Option<String> {
    let mut out = vec![0u8; ciphertext.len()];
    let len = Aes128CbcDec::new(DEFAULT_AES_KEY.into(), DEFAULT_AES_KEY.into())
        .decrypt_padded_b2b_mut::<Pkcs7>(ciphertext, &mut out)
        .ok()?
        .len();
    out.truncate(len);
    String::from_utf8(out).ok()
}

/// bcrypt hash at default cost.
pub fn hash_password(password: &str) -> Result<String, String> {
    bcrypt::hash(password, bcrypt::DEFAULT_COST).map_err(|e| format!("bcrypt: {e}"))
}

/// bcrypt verify — the raw `$2a$…` stored string.
pub fn verify_password(password: &str, hash: &str) -> bool {
    bcrypt::verify(password, hash).unwrap_or(false)
}

/// The timing-equalizer dummy hash verifies on the
/// user-not-found paths .
pub const DUMMY_PASSWORD_HASH: &str =
    "$2a$10$1sbpKmhQDpXLHnDnEQ1nLe3oOnYyP2bUJyqHcX2T0Fq1qfyoXOrPm";

pub fn dummy_verify() {
    let _ = bcrypt::verify("definitely-not-the-password", DUMMY_PASSWORD_HASH);
}

/// SHA-256 hex — the access-key secret storage form.
pub fn sha256_hex(data: &[u8]) -> String {
    hex::encode(Sha256::digest(data))
}

/// AES-256-GCM encrypt with the `enc:` envelope (MFA secret at-rest).
/// Random 12-byte nonce prepended to the ciphertext, then base64.
pub fn encrypt_if_needed(plain: &str) -> String {
    match crypto_key() {
        None => plain.to_string(),
        Some(key) => {
            use aes_gcm::aead::Aead;
            use aes_gcm::Aes256Gcm;
            use aes_gcm::KeyInit;
            let cipher = Aes256Gcm::new((&key).into());
            let nonce_bytes = rand::random::<[u8; 12]>();
            let ct = cipher
                .encrypt((&nonce_bytes).into(), plain.as_bytes())
                .expect("aes-gcm encrypt");
            let mut packed = nonce_bytes.to_vec();
            packed.extend(ct);
            use base64::Engine as _;
            format!(
                "enc:{}",
                base64::engine::general_purpose::STANDARD.encode(packed)
            )
        }
    }
}

/// Reverses [`encrypt_if_needed`].
pub fn decrypt_if_needed(stored: &str) -> Result<String, String> {
    let Some(stripped) = stored.strip_prefix("enc:") else {
        return Ok(stored.to_string());
    };
    let key = crypto_key().ok_or("RUSHWIND_CRYPTO_KEY unset but secret is encrypted")?;
    use base64::Engine as _;
    let packed = base64::engine::general_purpose::STANDARD
        .decode(stripped)
        .map_err(|e| format!("secret base64: {e}"))?;
    if packed.len() < 13 {
        return Err("secret too short".into());
    }
    use aes_gcm::aead::Aead;
    use aes_gcm::Aes256Gcm;
    use aes_gcm::KeyInit;
    let cipher = Aes256Gcm::new((&key).into());
    let plain = cipher
        .decrypt((&packed[..12]).into(), &packed[12..])
        .map_err(|_| "secret decrypt".to_string())?;
    String::from_utf8(plain).map_err(|_| "secret utf8".to_string())
}

/// TOTP verify (RFC 6238, SHA-1, 6 digits, 30 s step, ±1 window) — the
/// `ValidateCustom`-shaped validation parameters.
pub fn totp_verify(secret_base32: &str, code: &str) -> bool {
    if code.len() != 6 || !code.bytes().all(|b| b.is_ascii_digit()) {
        return false;
    }
    let secret = match base32_decode(secret_base32) {
        Some(s) => s,
        None => return false,
    };
    let now = chrono::Utc::now().timestamp();
    for drift in [-1i64, 0, 1] {
        let counter = (now / 30 + drift) as u64;
        if totp_at(&secret, counter) == code {
            return true;
        }
    }
    false
}

fn totp_at(secret: &[u8], counter: u64) -> String {
    use hmac::Mac;
    let mut mac = hmac::Hmac::<Sha1>::new_from_slice(secret).expect("hmac sha1");
    mac.update(&counter.to_be_bytes());
    let digest = mac.finalize().into_bytes();
    let offset = (digest[digest.len() - 1] & 0x0f) as usize;
    let binary = ((digest[offset] as u32 & 0x7f) << 24)
        | ((digest[offset + 1] as u32) << 16)
        | ((digest[offset + 2] as u32) << 8)
        | digest[offset + 3] as u32;
    format!("{:06}", binary % 1_000_000)
}

use sha1::Sha1;

/// RFC 4648 base32 (no padding), the otpauth secret alphabet.
pub fn base32_decode(input: &str) -> Option<Vec<u8>> {
    const ALPHABET: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZ234567";
    let mut bits = 0u32;
    let mut acc = 0u32;
    let mut out = Vec::new();
    for ch in input.trim_end_matches('=').bytes() {
        let upper = ch.to_ascii_uppercase();
        let value = ALPHABET.iter().position(|&a| a == upper)? as u32;
        acc = (acc << 5) | value;
        bits += 5;
        if bits >= 8 {
            bits -= 8;
            out.push((acc >> bits) as u8);
        }
    }
    Some(out)
}

pub fn base32_encode(data: &[u8]) -> String {
    const ALPHABET: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZ234567";
    let mut out = String::new();
    let mut bits = 0u32;
    let mut acc = 0u32;
    for &byte in data {
        acc = (acc << 8) | byte as u32;
        bits += 8;
        while bits >= 5 {
            bits -= 5;
            out.push(ALPHABET[(acc >> bits) as usize & 31] as char);
        }
    }
    if bits > 0 {
        out.push(ALPHABET[(acc << (5 - bits)) as usize & 31] as char);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::{
        decrypt_aes_cbc, decrypt_transfer_password, hash_password, parse_private_pem, sha256_hex,
        using_dev_password_private_key, verify_password, DEFAULT_AES_KEY, DUMMY_PASSWORD_HASH,
        ENVELOPE_PREFIX,
    };

    #[test]
    fn bcrypt_hash_and_verify_roundtrip() {
        let h = hash_password("s3cret!x").unwrap();
        assert!(verify_password("s3cret!x", &h));
        assert!(!verify_password("wrong", &h));
    }

    #[test]
    fn dummy_hash_never_verifies_the_canary() {
        assert!(!verify_password(
            "definitely-not-the-password",
            DUMMY_PASSWORD_HASH
        ));
    }

    #[test]
    fn sha256_known_vector() {
        assert_eq!(
            sha256_hex(b"abc"),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }

    #[test]
    fn aes_cbc_roundtrip_through_default_key() {
        use aes::cipher::{block_padding::Pkcs7, BlockEncryptMut, KeyIvInit};
        type Aes128CbcEnc = cbc::Encryptor<aes::Aes128>;
        let plaintext = b"login-credential-payload";
        let mut buf = [0u8; 48];
        let ciphertext = Aes128CbcEnc::new(DEFAULT_AES_KEY.into(), DEFAULT_AES_KEY.into())
            .encrypt_padded_b2b_mut::<Pkcs7>(plaintext, &mut buf)
            .unwrap()
            .to_vec();
        assert_eq!(
            decrypt_aes_cbc(&ciphertext).unwrap(),
            "login-credential-payload"
        );
    }

    /// Golden vectors produced by the reference Go implementation
    /// (EncryptPasswordEnvelope with the in-repo dev keypair): the
    /// Rust decrypt must accept what the real frontend/Go emits,
    /// including a CJK plaintext and the one-char edge.
    #[test]
    fn envelope_golden_vectors_from_reference_impl() {
        for (envelope, plain) in [
            (
                "rsa-v1:US/2KKULhGiS3/kaO/F4vEXnGTIlhfZ0QPHdgZjPkKa4T9LKQQUWYxn3m3S6RYeF5Wpvwq5A4S7sPYgJ7a3UNXrVM6WJ2UESqhk1nPw29WW3zzbeSKXatu7GwQ7K76m7iVW2dSSfqIrm2K/cVA0o56HkZqW6RL9FtCCWELIH2dT7QpYhtsvZEA4rtxdKjM2ESDBw5SJOX1H9oCcPTUiwG+6mlMHFwx5GvUDDQMtaLVIaR0ayAT+7HRufwNNTtS0BUzdceFpu2DWQZf0ZlDqjKGLmkB7dICpNJbMSDdyYjA4C2GpqBxfh5dOdCLOHp3Y3j6DC2bJhsUktem+qr+uG2Q==:+3/Y+63MmMoCkLk8:rg1AnT4Dw9mAytPuhSl1XvGpkRF0eCuWVA==",
                "Abcd@1234",
            ),
            (
                "rsa-v1:aJGTxXZFP/0QgIPweAYwn4hLNP7w25tjtR8QNjbB9LFCuL8e2v48BeCDmkQyZZaUbWbhzU4fTljkp1boZJlVbKS8kBQtf9FMZMGs1yNDNX/UPd5XEqVksq0B7jvsmnSxlTwJXzExN6lkmpBuMx8h+gG1+uMdOBGs9WWR5ZqNCi9+YguGgKUVB865ELinbYJCyLOzHgbv4BoTIeZ+vfnUWsjW+cKMmwNg2cg3GM0P2OQe66IdYFNXrFa8bfWcMlVYc1EQE3bT4NqPHZyU8lpBpah1ABATlVd+R02GssTUKe441anPPFLY8RRimqav1+V34REtRP5bU5YTIHQ1YvLcWQ==:nEt0bsKXHUQw7Rtv:L+5MhZVINKbJXXq0wF8T0gELQ0BCfGUQRtmMaOTyRnhLOzZ3ciI=",
                "中文密码+符号!@#",
            ),
            (
                "rsa-v1:Q/RnaGGSZ9kRfiP0CZVNZ/YUy5pP4ajiUytDq8IAPDulPOctYgI6nDFKzSFi5gltWyIMKv9ld2zTRQWbyHUeba1h58URHchyUKCogjdZ6K08iaxi9Xmni63lmwEhSm5B20jDzoIsFMzfXTXObXOZiLGnoLfErs6T1zavMK4JVuGg1MgqLQQ8aiVjyoIMHdTydGnaWpHT/IhMnR/X82mICAS4yEjspDDEZv+rsugoVtUWYPrmR/U056cQzpwvBmdb1rZj6LV8XB842LJoy4VH0a18QtkkvzZZdSXu/k8k+tw4o7vJ8AVs00YS0S52r1wMjXE8xMznCvGfaQJuSB6cAQ==:JOyF37j5UJ+S0sTq:hdWxNHwA8qWv559De/D61Lc=",
                "x",
            ),
        ] {
            assert_eq!(decrypt_transfer_password(envelope).unwrap(), plain);
        }
    }

    /// The legacy form (base64 AES-128-CBC, key=IV) still decrypts
    /// through the unified entry — the frontend rollout transition.
    #[test]
    fn legacy_form_accepted_through_unified_entry() {
        use aes::cipher::{block_padding::Pkcs7, BlockEncryptMut, KeyIvInit};
        use base64::Engine as _;
        type Aes128CbcEnc = cbc::Encryptor<aes::Aes128>;
        let mut buf = [0u8; 32];
        let ciphertext = Aes128CbcEnc::new(DEFAULT_AES_KEY.into(), DEFAULT_AES_KEY.into())
            .encrypt_padded_b2b_mut::<Pkcs7>(b"LegacyP@ss9", &mut buf)
            .unwrap();
        let legacy = base64::engine::general_purpose::STANDARD.encode(ciphertext);
        assert_eq!(decrypt_transfer_password(&legacy).unwrap(), "LegacyP@ss9");
    }

    /// Garbage never panics and never decrypts (mirrors the reference
    /// TestDecryptGarbageRejected).
    #[test]
    fn garbage_inputs_rejected() {
        for bad in [
            format!("{ENVELOPE_PREFIX}!!!:???:**:"),
            "not-base64!!".to_string(),
            String::new(),
            // structurally right but cryptographically wrong
            format!("{ENVELOPE_PREFIX}AAAA:BBBB:CCCC"),
        ] {
            assert!(decrypt_transfer_password(&bad).is_err(), "{bad:?}");
        }
    }

    #[test]
    fn base32_roundtrip_otp_alphabet() {
        use super::{base32_decode, base32_encode};
        // RFC 4648 无填充字母表：任意字节串编码→解码恒等
        for sample in [
            b"hello world".to_vec(),
            vec![0u8, 1, 2, 250, 251, 255],
            b"K".to_vec(),
            b"KK".to_vec(),
            b"KKK".to_vec(),
            b"KKKK".to_vec(),
            b"KKKKK".to_vec(),
        ] {
            let encoded = base32_encode(&sample);
            let decoded = base32_decode(&encoded).unwrap();
            assert_eq!(decoded, sample, "{encoded}");
            // 全大写字母表 + padding 兼容解码
            assert!(encoded
                .bytes()
                .all(|b| b.is_ascii_uppercase() || (b'2'..=b'7').contains(&b)));
        }
        // 小写输入解码容错（otpauth 密钥常以小写出现）
        let upper = base32_encode(b"secret!");
        assert_eq!(base32_decode(&upper.to_lowercase()).unwrap(), b"secret!");
        assert!(base32_decode("!!!!").is_none());
    }

    /// The dev pair is in use when env is unset — the startup warning
    /// predicate — and a `\\n`-escaped single-line PEM parses to the
    /// same key (the env override form).
    #[test]
    fn dev_key_detected_and_escaped_pem_parses() {
        // NOTE: holds while RUSHWIND_PASSWORD_RSA_PRIVATE_KEY is unset
        // or set to the dev pair; the test process does not override it.
        assert!(using_dev_password_private_key());

        let escaped = super::DEV_PRIVATE_PEM.replace('\n', "\\n");
        // the env wire form is unescaped before parsing (the loader's job)
        let unescaped = escaped.replace("\\n", "\n");
        let parsed = parse_private_pem(&unescaped).expect("escaped pem parses");
        let direct = parse_private_pem(super::DEV_PRIVATE_PEM).unwrap();
        use rsa::pkcs8::EncodePrivateKey;
        assert_eq!(
            parsed.to_pkcs8_der().unwrap().as_bytes(),
            direct.to_pkcs8_der().unwrap().as_bytes()
        );
    }
}
