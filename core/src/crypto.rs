use aes_gcm::{Aes256Gcm, Key, Nonce, KeyInit};
use aes_gcm::aead::Aead;
use sha2::{Sha256, Digest};
use anyhow::{Result, anyhow};

/// Prefijo mágico que identifica un archivo config cifrado por Agente AIR.
/// Si el archivo empieza con este prefijo, el contenido posterior es base64(nonce + ciphertext).
pub const MAGIC_PREFIX: &str = "PAGENT_ENC:";

/// Secreto interno embebido en el binario. NO es visible para el usuario final.
/// Combinado con SHA-256 produce la clave AES-256 de 32 bytes.
const INTERNAL_SECRET: &[u8] = b"PrintAgentRS::Config::v1::s3cr3t_k3y_2026";

/// Deriva la clave AES-256 a partir del secreto interno usando SHA-256.
fn derive_key() -> Key<Aes256Gcm> {
    let hash = Sha256::digest(INTERNAL_SECRET);
    *Key::<Aes256Gcm>::from_slice(&hash)
}

/// Cifra un texto plano (el contenido TOML del config) y devuelve
/// el string con el prefijo mágico + base64(nonce || ciphertext).
pub fn cifrar(texto_plano: &str) -> Result<String> {
    use aes_gcm::aead::OsRng;
    use aes_gcm::AeadCore;

    let key = derive_key();
    let cipher = Aes256Gcm::new(&key);
    let nonce = Aes256Gcm::generate_nonce(&mut OsRng);

    let ciphertext = cipher.encrypt(&nonce, texto_plano.as_bytes())
        .map_err(|e| anyhow!("Error cifrando config: {}", e))?;

    // Concatenar nonce (12 bytes) + ciphertext y codificar en base64
    let mut combined = nonce.to_vec();
    combined.extend_from_slice(&ciphertext);

    let encoded = base64::Engine::encode(&base64::engine::general_purpose::STANDARD, &combined);
    Ok(format!("{}{}", MAGIC_PREFIX, encoded))
}

/// Descifra un string que empieza con el prefijo mágico.
/// Devuelve el texto TOML original.
pub fn descifrar(contenido_cifrado: &str) -> Result<String> {
    let b64 = contenido_cifrado
        .strip_prefix(MAGIC_PREFIX)
        .ok_or_else(|| anyhow!("El archivo no tiene el prefijo de cifrado esperado"))?;

    let combined = base64::Engine::decode(&base64::engine::general_purpose::STANDARD, b64)
        .map_err(|e| anyhow!("Error decodificando base64 del config: {}", e))?;

    if combined.len() < 12 {
        return Err(anyhow!("Datos cifrados corruptos (muy cortos)"));
    }

    let (nonce_bytes, ciphertext) = combined.split_at(12);
    let nonce = Nonce::from_slice(nonce_bytes);

    let key = derive_key();
    let cipher = Aes256Gcm::new(&key);

    let plaintext = cipher.decrypt(nonce, ciphertext)
        .map_err(|_| anyhow!("⚠️ config.toml fue alterado o está corrupto. El descifrado falló. Contacte soporte técnico."))?;

    String::from_utf8(plaintext)
        .map_err(|e| anyhow!("El config descifrado no es texto válido: {}", e))
}

/// Detecta si un contenido está cifrado (empieza con el prefijo mágico).
pub fn esta_cifrado(contenido: &str) -> bool {
    contenido.starts_with(MAGIC_PREFIX)
}
