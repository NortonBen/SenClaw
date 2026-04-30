use aes_gcm::{
    aead::{Aead, KeyInit, Payload},
    Aes256Gcm, Nonce,
};
use rand::{rngs::OsRng, RngCore};
use base64::{engine::general_purpose::STANDARD, Engine};
use anyhow::{Result, anyhow};

pub struct Crypto {
    key: [u8; 32],
}

impl Crypto {
    pub fn new(key: [u8; 32]) -> Self {
        Self { key }
    }

    /// Encrypts data using AES-256-GCM.
    /// Returns (nonce, ciphertext, tag)
    pub fn encrypt(&self, data: &[u8]) -> Result<(Vec<u8>, Vec<u8>, Vec<u8>)> {
        let cipher = Aes256Gcm::new_from_slice(&self.key)
            .map_err(|e| anyhow!("Invalid key: {}", e))?;
        
        let mut nonce_bytes = [0u8; 12];
        OsRng.fill_bytes(&mut nonce_bytes);
        let nonce = Nonce::from_slice(&nonce_bytes);

        let ciphertext_with_tag = cipher
            .encrypt(nonce, Payload { msg: data, aad: &[] })
            .map_err(|e| anyhow!("Encryption failed: {}", e))?;

        // AES-GCM-256 tag is usually the last 16 bytes
        let (ciphertext, tag) = ciphertext_with_tag.split_at(ciphertext_with_tag.len() - 16);

        Ok((nonce_bytes.to_vec(), ciphertext.to_vec(), tag.to_vec()))
    }

    /// Decrypts data using AES-256-GCM.
    pub fn decrypt(&self, nonce_bytes: &[u8], ciphertext: &[u8], tag: &[u8]) -> Result<Vec<u8>> {
        let cipher = Aes256Gcm::new_from_slice(&self.key)
            .map_err(|e| anyhow!("Invalid key: {}", e))?;
        
        let nonce = Nonce::from_slice(nonce_bytes);
        
        let mut full_payload = ciphertext.to_vec();
        full_payload.extend_from_slice(tag);

        let plaintext = cipher
            .decrypt(nonce, Payload { msg: &full_payload, aad: &[] })
            .map_err(|e| anyhow!("Decryption failed: {}", e))?;

        Ok(plaintext)
    }

    /// Generates a random 32-byte key and returns as base64.
    pub fn generate_key() -> ( [u8; 32], String) {
        let mut key = [0u8; 32];
        OsRng.fill_bytes(&mut key);
        let b64 = STANDARD.encode(key);
        (key, b64)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_encrypt_decrypt() {
        let (key, _) = Crypto::generate_key();
        let crypto = Crypto::new(key);
        let data = b"hello semaclaw";
        
        let (nonce, ciphertext, tag) = crypto.encrypt(data).unwrap();
        let decrypted = crypto.decrypt(&nonce, &ciphertext, &tag).unwrap();
        
        assert_eq!(data, decrypted.as_slice());
    }
}
