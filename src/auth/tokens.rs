use sha2::{Digest, Sha256};

/// Hash tokenu z URL (w DB tylko hash).
pub fn hash_token(raw: &str) -> String {
    let digest = Sha256::digest(raw.as_bytes());
    digest.iter().map(|b| format!("{b:02x}")).collect()
}

pub fn new_token() -> String {
    uuid::Uuid::new_v4().to_string() + &uuid::Uuid::new_v4().to_string()
}
