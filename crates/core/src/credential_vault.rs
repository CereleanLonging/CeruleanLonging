//! Current-Windows-user DPAPI envelope; only encrypted bytes are written.
use crate::host::ordinary;
use cerulean_domain::credentials::CredentialProvider;
use cerulean_domain::{execution::require, secret::SecretInput};
use std::{
    fs::{self, OpenOptions},
    io::{Read, Write},
    path::{Path, PathBuf},
    ptr,
};
use windows_sys::Win32::{
    Foundation::LocalFree,
    Security::Cryptography::{
        CryptProtectData, CryptUnprotectData, CRYPTPROTECT_UI_FORBIDDEN, CRYPT_INTEGER_BLOB,
    },
};
use zeroize::{Zeroize, Zeroizing};
const LIMIT: usize = 64 * 1024;
fn provider_location(root: &Path, provider: CredentialProvider) -> Result<PathBuf, String> {
    ordinary(root, true)?;
    let dir = root.join("credentials");
    if dir.exists() {
        ordinary(&dir, true)?;
    }
    Ok(dir.join(format!("{}.dpapi", provider.id())))
}
pub(crate) fn exists_for(root: &Path, provider: CredentialProvider) -> Result<bool, String> {
    let path = provider_location(root, provider)?;
    if !path.exists() {
        return Ok(false);
    }
    ordinary(&path, false)?;
    require(
        fs::metadata(path)
            .map_err(|_| "无法读取加密凭证文件")?
            .len()
            <= LIMIT as u64,
        "加密凭证文件过大",
    )?;
    Ok(true)
}
struct Allocation(CRYPT_INTEGER_BLOB);
impl Drop for Allocation {
    fn drop(&mut self) {
        unsafe {
            if !self.0.pbData.is_null() {
                std::slice::from_raw_parts_mut(self.0.pbData, self.0.cbData as usize).zeroize();
                LocalFree(self.0.pbData.cast());
            }
        }
    }
}
fn crypt(
    root: &Path,
    provider: CredentialProvider,
    bytes: &[u8],
    decrypt: bool,
) -> Result<Zeroizing<Vec<u8>>, String> {
    use sha2::{Digest, Sha256};
    require(bytes.len() <= LIMIT, "credential envelope exceeds limit")?;
    let canonical = fs::canonicalize(root).map_err(|_| "credential root unavailable")?;
    let entropy = Sha256::digest(
        format!(
            "CeruleanLonging/{}/v1/{}",
            provider.id(),
            canonical.to_string_lossy().to_lowercase()
        )
        .as_bytes(),
    );
    let input = CRYPT_INTEGER_BLOB {
        cbData: bytes.len() as u32,
        pbData: bytes.as_ptr().cast_mut(),
    };
    let extra = CRYPT_INTEGER_BLOB {
        cbData: entropy.len() as u32,
        pbData: entropy.as_ptr().cast_mut(),
    };
    let mut output = Allocation(CRYPT_INTEGER_BLOB {
        cbData: 0,
        pbData: ptr::null_mut(),
    });
    let ok = unsafe {
        if decrypt {
            CryptUnprotectData(
                &input,
                ptr::null_mut(),
                &extra,
                ptr::null(),
                ptr::null(),
                CRYPTPROTECT_UI_FORBIDDEN,
                &mut output.0,
            )
        } else {
            CryptProtectData(
                &input,
                ptr::null(),
                &extra,
                ptr::null(),
                ptr::null(),
                CRYPTPROTECT_UI_FORBIDDEN,
                &mut output.0,
            )
        }
    };
    require(
        ok != 0,
        "Windows 凭证加解密失败；请在当前用户和原工作台目录重新配置",
    )?;
    require(
        output.0.cbData as usize <= LIMIT && !output.0.pbData.is_null(),
        "invalid protected credential",
    )?;
    Ok(Zeroizing::new(
        unsafe { std::slice::from_raw_parts(output.0.pbData, output.0.cbData as usize) }.to_vec(),
    ))
}
pub(crate) fn save_for(
    root: &Path,
    provider: CredentialProvider,
    key: &SecretInput,
) -> Result<(), String> {
    // Credential slots have independent DPAPI envelopes and do not change the
    // project journal format. Reader guards apply when project records are written.
    key.validate()?;
    let dest = provider_location(root, provider)?;
    let folder = dest.parent().ok_or("credential folder missing")?;
    if !folder.exists() {
        fs::create_dir(folder).map_err(|_| "无法创建凭证目录")?;
    }
    ordinary(folder, true)?;
    if dest.exists() {
        ordinary(&dest, false)?;
    }
    let encrypted = crypt(root, provider, key.expose().as_bytes(), false)?;
    let temp = folder.join(format!(
        "{}-{}.tmp",
        provider.id(),
        chrono::Utc::now()
            .timestamp_nanos_opt()
            .ok_or("invalid clock")?
    ));
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&temp)
        .map_err(|_| "无法保存加密凭证")?;
    file.write_all(&encrypted)
        .and_then(|_| file.sync_all())
        .map_err(|_| "加密凭证写入未确认")?;
    drop(file);
    fs::rename(temp, dest).map_err(|_| "加密凭证替换未确认".into())
}
pub(crate) fn load_for(root: &Path, provider: CredentialProvider) -> Result<SecretInput, String> {
    let path = provider_location(root, provider)?;
    ordinary(&path, false).map_err(|_| "请先在本机配置对应供应商的 API Key")?;
    let mut bytes = Vec::new();
    fs::File::open(path)
        .map_err(|_| "无法读取加密凭证")?
        .take((LIMIT + 1) as u64)
        .read_to_end(&mut bytes)
        .map_err(|_| "无法读取加密凭证")?;
    let plain = crypt(root, provider, &bytes, true)?;
    let text = std::str::from_utf8(&plain).map_err(|_| "invalid credential encoding")?;
    SecretInput::new(text.into())
}
pub(crate) fn remove_for(root: &Path, provider: CredentialProvider) -> Result<(), String> {
    let path = provider_location(root, provider)?;
    if path.exists() {
        ordinary(&path, false)?;
        fs::remove_file(path).map_err(|_| "无法删除加密凭证")?;
    }
    Ok(())
}
// Preserve the legacy OpenAI filename and DPAPI entropy exactly.
pub(crate) fn exists(root: &Path) -> Result<bool, String> {
    exists_for(root, CredentialProvider::OpenAi)
}
pub(crate) fn save(root: &Path, key: &SecretInput) -> Result<(), String> {
    save_for(root, CredentialProvider::OpenAi, key)
}
pub(crate) fn load(root: &Path) -> Result<SecretInput, String> {
    load_for(root, CredentialProvider::OpenAi)
}
pub(crate) fn remove(root: &Path) -> Result<(), String> {
    remove_for(root, CredentialProvider::OpenAi)
}
#[cfg(test)]
fn location(root: &Path) -> Result<PathBuf, String> {
    provider_location(root, CredentialProvider::OpenAi)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn legacy_openai_entropy_remains_decryptable_after_provider_split() {
        use sha2::{Digest, Sha256};
        let root = tempfile::tempdir().unwrap();
        let marker = b"sk-legacy-openai-offline-compatibility";
        // Encode using the original literal namespace, independently of crypt().
        let canonical = fs::canonicalize(root.path()).unwrap();
        let entropy = Sha256::digest(
            format!(
                "CeruleanLonging/openai/v1/{}",
                canonical.to_string_lossy().to_lowercase()
            )
            .as_bytes(),
        );
        let input = CRYPT_INTEGER_BLOB {
            cbData: marker.len() as u32,
            pbData: marker.as_ptr().cast_mut(),
        };
        let extra = CRYPT_INTEGER_BLOB {
            cbData: entropy.len() as u32,
            pbData: entropy.as_ptr().cast_mut(),
        };
        let mut output = Allocation(CRYPT_INTEGER_BLOB {
            cbData: 0,
            pbData: ptr::null_mut(),
        });
        assert_ne!(
            unsafe {
                CryptProtectData(
                    &input,
                    ptr::null(),
                    &extra,
                    ptr::null(),
                    ptr::null(),
                    CRYPTPROTECT_UI_FORBIDDEN,
                    &mut output.0,
                )
            },
            0
        );
        fs::create_dir(root.path().join("credentials")).unwrap();
        fs::write(root.path().join("credentials/openai.dpapi"), unsafe {
            std::slice::from_raw_parts(output.0.pbData, output.0.cbData as usize)
        })
        .unwrap();
        assert_eq!(load(root.path()).unwrap().expose().as_bytes(), marker);
        assert_eq!(
            load_for(root.path(), CredentialProvider::OpenAi)
                .unwrap()
                .expose()
                .as_bytes(),
            marker
        );
        assert!(!exists_for(root.path(), CredentialProvider::DeepSeek).unwrap());
    }
    #[test]
    fn provider_slots_are_separate_and_never_enter_product_history() {
        use cerulean_domain::{Command, Request, PROTOCOL_VERSION};
        let root = tempfile::tempdir().unwrap();
        let openai = SecretInput::new("sk-offline-openai-provider-marker".into()).unwrap();
        let deepseek = SecretInput::new("sk-offline-deepseek-provider-marker".into()).unwrap();
        let mut host = crate::host::RuntimeHost::open(root.path()).unwrap();
        for (provider, key) in [
            (CredentialProvider::OpenAi, openai.clone()),
            (CredentialProvider::DeepSeek, deepseek.clone()),
        ] {
            let command = Command::SetProviderKey { provider, key };
            assert!(!format!("{command:?}").contains("provider-marker"));
            let snapshot = host
                .handle(Request {
                    id: provider.id().into(),
                    protocol_version: PROTOCOL_VERSION,
                    command,
                })
                .result
                .unwrap();
            assert!(
                snapshot
                    .provider_credentials
                    .iter()
                    .find(|s| s.provider == provider)
                    .unwrap()
                    .key_configured
            );
            assert!(snapshot.events.is_empty());
            assert!(snapshot.workflows.is_empty());
            assert!(!serde_json::to_string(&snapshot)
                .unwrap()
                .contains("provider-marker"));
        }
        assert_eq!(
            load_for(root.path(), CredentialProvider::OpenAi)
                .unwrap()
                .expose(),
            openai.expose()
        );
        assert_eq!(
            load_for(root.path(), CredentialProvider::DeepSeek)
                .unwrap()
                .expose(),
            deepseek.expose()
        );
        let invalid:Command=serde_json::from_value(serde_json::json!({"type":"setProviderKey","provider":"deepseek","key":"invalid whitespace value"})).unwrap();
        let result = host.handle(Request {
            id: "invalid".into(),
            protocol_version: PROTOCOL_VERSION,
            command: invalid,
        });
        assert!(result.error.is_some());
        assert!(!serde_json::to_string(&result)
            .unwrap()
            .contains("invalid whitespace"));
        assert_eq!(
            load_for(root.path(), CredentialProvider::DeepSeek)
                .unwrap()
                .expose(),
            deepseek.expose()
        );
        drop(host);
        let mut host = crate::host::RuntimeHost::open(root.path()).unwrap();
        assert!(host
            .snapshot()
            .provider_credentials
            .iter()
            .all(|s| s.key_configured
                == matches!(
                    s.provider,
                    CredentialProvider::OpenAi | CredentialProvider::DeepSeek
                )));
        host.handle(Request {
            id: "remove-deepseek".into(),
            protocol_version: PROTOCOL_VERSION,
            command: Command::RemoveProviderKey {
                provider: CredentialProvider::DeepSeek,
            },
        })
        .result
        .unwrap();
        assert!(!exists_for(root.path(), CredentialProvider::DeepSeek).unwrap());
        assert_eq!(load(root.path()).unwrap().expose(), openai.expose());
        drop(host);
        let host = crate::host::RuntimeHost::open(root.path()).unwrap();
        assert!(
            !host
                .snapshot()
                .provider_credentials
                .iter()
                .find(|s| s.provider == CredentialProvider::DeepSeek)
                .unwrap()
                .key_configured
        );
        assert!(host.snapshot().openai_connection.key_configured);
        for provider in ["../../outside", "unregistered-provider", "DEEPSEEK"] {
            assert!(serde_json::from_value::<Command>(serde_json::json!({"type":"setProviderKey","provider":provider,"key":"sk-fixture-should-never-be-used"})).is_err());
        }
    }
    #[test]
    fn deepseek_envelope_cannot_be_swapped_into_openai_or_another_workspace() {
        let root = tempfile::tempdir().unwrap();
        let other = tempfile::tempdir().unwrap();
        let key = SecretInput::new("sk-deepseek-envelope-offline-marker".into()).unwrap();
        save_for(root.path(), CredentialProvider::DeepSeek, &key).unwrap();
        let ciphertext =
            fs::read(provider_location(root.path(), CredentialProvider::DeepSeek).unwrap())
                .unwrap();
        assert!(!ciphertext
            .windows(key.expose().len())
            .any(|w| w == key.expose().as_bytes()));
        fs::write(
            provider_location(root.path(), CredentialProvider::OpenAi).unwrap(),
            &ciphertext,
        )
        .unwrap();
        assert!(load(root.path()).is_err());
        fs::create_dir(other.path().join("credentials")).unwrap();
        fs::write(
            provider_location(other.path(), CredentialProvider::DeepSeek).unwrap(),
            &ciphertext,
        )
        .unwrap();
        assert!(load_for(other.path(), CredentialProvider::DeepSeek).is_err());
        assert_eq!(
            load_for(root.path(), CredentialProvider::DeepSeek)
                .unwrap()
                .expose(),
            key.expose()
        );
        let replacement = SecretInput::new("sk-deepseek-replacement-offline".into()).unwrap();
        save_for(root.path(), CredentialProvider::DeepSeek, &replacement).unwrap();
        assert_eq!(
            load_for(root.path(), CredentialProvider::DeepSeek)
                .unwrap()
                .expose(),
            replacement.expose()
        );
    }
    #[test]
    fn host_key_commands_never_write_plaintext_to_journal_or_snapshot() {
        use crate::host::RuntimeHost;
        use cerulean_domain::{Command, Request, PROTOCOL_VERSION};
        let root = tempfile::tempdir().unwrap();
        let mut host = RuntimeHost::open(root.path()).unwrap();
        let marker = "sk-host-synthetic-never-real-credential";
        let command = Command::SetOpenAiKey {
            key: SecretInput::new(marker.into()).unwrap(),
        };
        assert!(!format!("{command:?}").contains(marker));
        let response = host.handle(Request {
            id: "key".into(),
            protocol_version: PROTOCOL_VERSION,
            command,
        });
        let snapshot = response.result.unwrap();
        assert!(snapshot.openai_connection.key_configured);
        assert!(snapshot.events.is_empty());
        assert!(!serde_json::to_string(&snapshot).unwrap().contains(marker));
        drop(host);
        let mut host = RuntimeHost::open(root.path()).unwrap();
        assert_eq!(host.snapshot().openai_connection.state, "unchecked");
        let invalid: Command = serde_json::from_value(
            serde_json::json!({"type":"setOpenAiKey","key":" whitespace secret "}),
        )
        .unwrap();
        let error = host
            .handle(Request {
                id: "bad".into(),
                protocol_version: PROTOCOL_VERSION,
                command: invalid,
            })
            .error
            .unwrap();
        assert!(!error.message.contains("whitespace"));
        assert_eq!(load(root.path()).unwrap().expose(), marker);
        assert!(host
            .handle(Request {
                id: "remove".into(),
                protocol_version: PROTOCOL_VERSION,
                command: Command::RemoveOpenAiKey
            })
            .result
            .is_some());
        assert!(!exists(root.path()).unwrap());
        assert!(!host.snapshot().openai_connection.key_configured);
    }
    #[test]
    fn dpapi_roundtrip_replace_tamper_and_workspace_binding() {
        let root = tempfile::tempdir().unwrap();
        let other = tempfile::tempdir().unwrap();
        let key = SecretInput::new("sk-synthetic-vault-test-not-real".into()).unwrap();
        save(root.path(), &key).unwrap();
        let encrypted = fs::read(location(root.path()).unwrap()).unwrap();
        assert!(!encrypted
            .windows(key.expose().len())
            .any(|s| s == key.expose().as_bytes()));
        assert_eq!(load(root.path()).unwrap().expose(), key.expose());
        let second = SecretInput::new("sk-replacement-synthetic-not-real".into()).unwrap();
        save(root.path(), &second).unwrap();
        assert_eq!(load(root.path()).unwrap().expose(), second.expose());
        fs::create_dir(other.path().join("credentials")).unwrap();
        fs::write(location(other.path()).unwrap(), &encrypted).unwrap();
        assert!(load(other.path()).is_err());
        let mut damaged = encrypted;
        damaged[32] ^= 1;
        fs::write(location(root.path()).unwrap(), damaged).unwrap();
        assert!(load(root.path()).is_err());
        remove(root.path()).unwrap();
        assert!(!exists(root.path()).unwrap());
    }
}
