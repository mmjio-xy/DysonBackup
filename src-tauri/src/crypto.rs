//! 加密、解密与哈希工具
//! 使用 AES-256-GCM + PBKDF2-HMAC-SHA256 密钥派生

use aes_gcm::aead::{Aead, KeyInit};
use aes_gcm::{Aes256Gcm, Nonce};
use anyhow::{anyhow, Context, Result};
use base64::engine::general_purpose::STANDARD as BASE64;
use base64::Engine;
use pbkdf2::pbkdf2_hmac;
use rand::{rng, Rng};
use sha2::{Digest, Sha256};

use crate::types::EncryptionMeta;

/// PBKDF2 迭代次数（OWASP 2023 推荐最低值）
const PBKDF2_ITERS: u32 = 600_000;

/// 计算数据的 SHA-256 哈希，返回十六进制字符串
pub fn sha256_hex(data: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(data);
    hex::encode(hasher.finalize())
}

/// 使用 AES-256-GCM 加密数据
/// - 随机生成 16 字节 salt 和 12 字节 nonce
/// - 通过 PBKDF2 从密码派生 32 字节密钥
///
/// 返回 (密文, 加密元数据)
#[allow(dead_code)]
pub fn encrypt_aes_gcm(password: &str, plain: &[u8]) -> Result<(Vec<u8>, EncryptionMeta)> {
    let mut salt = [0u8; 16];
    let mut nonce_bytes = [0u8; 12];
    rng().fill(&mut salt);
    rng().fill(&mut nonce_bytes);

    let mut key = [0u8; 32];
    pbkdf2_hmac::<Sha256>(password.as_bytes(), &salt, PBKDF2_ITERS, &mut key);

    let cipher = Aes256Gcm::new_from_slice(&key).context("Invalid AES key")?;
    let nonce = Nonce::from_slice(&nonce_bytes);
    let ciphertext = cipher.encrypt(nonce, plain).map_err(|_| anyhow!("Encryption failed"))?;

    Ok((ciphertext, EncryptionMeta {
        algorithm: "AES-256-GCM".to_string(),
        kdf: "PBKDF2-HMAC-SHA256".to_string(),
        iterations: PBKDF2_ITERS,
        salt_b64: BASE64.encode(salt),
        nonce_b64: BASE64.encode(nonce_bytes),
        stream_encryption: None,
        segment_size: None,
    }))
}

/// 分段加密的默认段大小
pub const STREAM_SEGMENT_SIZE: usize = 64 * 1024;
/// GCM tag 大小
const GCM_TAG_SIZE: usize = 16;

/// 从 base_nonce XOR segment_index 派生每段的 nonce，防止重排序攻击
fn derive_segment_nonce(base_nonce: &[u8; 12], index: u64) -> [u8; 12] {
    let mut nonce = *base_nonce;
    let idx_bytes = index.to_le_bytes();
    for i in 0..8 {
        nonce[i] ^= idx_bytes[i];
    }
    nonce
}

/// 分段流式 AES-256-GCM 加密
/// 将 plaintext 按 STREAM_SEGMENT_SIZE 分段，每段独立加密
/// 返回 (密文, 加密元数据)，元数据中 stream_encryption=true
pub fn encrypt_aes_gcm_stream(password: &str, plaintext: &[u8]) -> Result<(Vec<u8>, EncryptionMeta)> {
    let mut salt = [0u8; 16];
    let mut base_nonce = [0u8; 12];
    rng().fill(&mut salt);
    rng().fill(&mut base_nonce);

    let mut key = [0u8; 32];
    pbkdf2_hmac::<Sha256>(password.as_bytes(), &salt, PBKDF2_ITERS, &mut key);
    let cipher = Aes256Gcm::new_from_slice(&key).context("Invalid AES key")?;

    // 预分配：每段增加 GCM_TAG_SIZE 字节
    let num_segments = plaintext.len().div_ceil(STREAM_SEGMENT_SIZE);
    let mut output = Vec::with_capacity(plaintext.len() + num_segments * GCM_TAG_SIZE);

    for (idx, chunk) in plaintext.chunks(STREAM_SEGMENT_SIZE).enumerate() {
        let nonce_bytes = derive_segment_nonce(&base_nonce, idx as u64);
        let nonce = Nonce::from_slice(&nonce_bytes);
        let encrypted = cipher.encrypt(nonce, chunk)
            .map_err(|_| anyhow!("Stream encryption failed at segment {}", idx))?;
        output.extend_from_slice(&encrypted);
    }

    Ok((output, EncryptionMeta {
        algorithm: "AES-256-GCM".to_string(),
        kdf: "PBKDF2-HMAC-SHA256".to_string(),
        iterations: PBKDF2_ITERS,
        salt_b64: BASE64.encode(salt),
        nonce_b64: BASE64.encode(base_nonce),
        stream_encryption: Some(true),
        segment_size: Some(STREAM_SEGMENT_SIZE as u64),
    }))
}

/// 分段流式 AES-256-GCM 解密
/// 每段密文大小 = segment_size + GCM_TAG_SIZE
pub fn decrypt_aes_gcm_stream(password: &str, ciphertext: &[u8], meta: &EncryptionMeta) -> Result<Vec<u8>> {
    let salt = BASE64.decode(&meta.salt_b64).context("Invalid encryption salt")?;
    let base_nonce_vec = BASE64.decode(&meta.nonce_b64).context("Invalid encryption nonce")?;
    if base_nonce_vec.len() != 12 {
        return Err(anyhow!("Invalid nonce length"));
    }
    let base_nonce: [u8; 12] = base_nonce_vec.try_into().unwrap();

    let seg_size = meta.segment_size.unwrap_or(STREAM_SEGMENT_SIZE as u64) as usize;
    let cipher_seg_size = seg_size + GCM_TAG_SIZE;

    let mut key = [0u8; 32];
    pbkdf2_hmac::<Sha256>(password.as_bytes(), &salt, meta.iterations, &mut key);
    let cipher = Aes256Gcm::new_from_slice(&key).context("Invalid AES key")?;

    let mut output = Vec::with_capacity(ciphertext.len());

    for (idx, chunk) in ciphertext.chunks(cipher_seg_size).enumerate() {
        let nonce_bytes = derive_segment_nonce(&base_nonce, idx as u64);
        let nonce = Nonce::from_slice(&nonce_bytes);
        let decrypted = cipher.decrypt(nonce, chunk)
            .map_err(|_| anyhow!("Stream decryption failed at segment {} (password may be wrong)", idx))?;
        output.extend_from_slice(&decrypted);
    }

    Ok(output)
}

/// 使用 AES-256-GCM 解密数据，元数据来自 manifest
pub fn decrypt_aes_gcm(password: &str, cipher_text: &[u8], meta: &EncryptionMeta) -> Result<Vec<u8>> {
    let salt = BASE64.decode(&meta.salt_b64).context("Invalid encryption salt")?;
    let nonce_bytes = BASE64.decode(&meta.nonce_b64).context("Invalid encryption nonce")?;
    if nonce_bytes.len() != 12 {
        return Err(anyhow!("Invalid nonce length"));
    }
    let mut key = [0u8; 32];
    pbkdf2_hmac::<Sha256>(password.as_bytes(), &salt, meta.iterations, &mut key);
    let cipher = Aes256Gcm::new_from_slice(&key).context("Invalid AES key")?;
    let nonce = Nonce::from_slice(&nonce_bytes);
    cipher.decrypt(nonce, cipher_text).map_err(|_| anyhow!("Failed to decrypt (password may be wrong)"))
}
