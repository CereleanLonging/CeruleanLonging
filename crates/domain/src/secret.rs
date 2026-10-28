//! Wire-only input. Never place this type in snapshots, journal events or errors.
use serde::{Deserialize, Serialize};
use ts_rs::TS;
use zeroize::Zeroize;

#[derive(Clone, Serialize, Deserialize, TS)]
#[ts(type = "string")]
pub struct SecretInput(String);
impl std::fmt::Debug for SecretInput {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("SecretInput([REDACTED])")
    }
}
impl Drop for SecretInput {
    fn drop(&mut self) {
        self.0.zeroize();
    }
}
impl SecretInput {
    pub fn new(value: String) -> Result<Self, String> {
        let secret = Self(value);
        secret.validate()?;
        Ok(secret)
    }
    pub fn validate(&self) -> Result<(), String> {
        if !(16..=512).contains(&self.0.len()) || !self.0.bytes().all(|b| b.is_ascii_graphic()) {
            return Err("密钥须为 16–512 个可见 ASCII 字符，不能带空格或换行".into());
        }
        Ok(())
    }
    pub fn expose(&self) -> &str {
        &self.0
    }
}
