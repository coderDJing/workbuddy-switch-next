//! 独立的本机凭据保护：状态读取不访问钥匙串，只有加解密账号包时才碰系统密钥。
use base64::{engine::general_purpose::STANDARD, Engine};

pub trait Vault: Send + Sync {
    fn seal(&self, plain: &[u8]) -> Result<String, String>;
    fn open(&self, sealed: &str) -> Result<Vec<u8>, String>;
}
pub struct SystemVault;

#[cfg(windows)]
fn crypt(bytes: &[u8], encrypt: bool) -> Result<Vec<u8>, String> {
    use windows::{
        core::PCWSTR,
        Win32::{
            Foundation::{LocalFree, HLOCAL},
            Security::Cryptography::{CryptProtectData, CryptUnprotectData, CRYPT_INTEGER_BLOB},
        },
    };
    let source = CRYPT_INTEGER_BLOB {
        cbData: u32::try_from(bytes.len()).map_err(|_| "MiniMax 账号数据过大")?,
        pbData: bytes.as_ptr() as *mut u8,
    };
    let mut out = CRYPT_INTEGER_BLOB::default();
    unsafe {
        if encrypt {
            CryptProtectData(&source, PCWSTR::null(), None, None, None, 1, &mut out)
        } else {
            CryptUnprotectData(&source, None, None, None, None, 1, &mut out)
        }
        .map_err(|_| {
            if encrypt {
                "无法加密 MiniMax 账号"
            } else {
                "无法解密 MiniMax 账号，请使用保存账号时的系统用户"
            }
        })?;
        let result = std::slice::from_raw_parts(out.pbData, out.cbData as usize).to_vec();
        let _ = LocalFree(HLOCAL(out.pbData as _));
        Ok(result)
    }
}

#[cfg(target_os = "macos")]
fn key(create: bool) -> Result<Vec<u8>, String> {
    use security_framework::os::macos::keychain::SecKeychain;
    const SERVICE: &str = "io.github.coderdjing.workbuddyswitchnext.minimax";
    let keychain = SecKeychain::default().map_err(|_| "无法访问 MiniMax 钥匙串")?;
    match keychain.find_generic_password(SERVICE, "vault-key") {
        Ok((password, _)) => {
            let bytes = password.to_vec();
            if bytes.len() != 32 {
                return Err("MiniMax 本机密钥损坏".into());
            }
            return Ok(bytes);
        }
        Err(error) if create && error.code() == -25300 => {} // errSecItemNotFound
        Err(_) => return Err("无法访问 MiniMax 钥匙串密钥，请允许系统授权".into()),
    }
    let mut bytes = uuid::Uuid::new_v4().as_bytes().to_vec();
    bytes.extend_from_slice(uuid::Uuid::new_v4().as_bytes());
    // 只新增、不更新：并发创建或授权被拒时不能覆盖已有账号的密钥。
    match keychain.add_generic_password(SERVICE, "vault-key", &bytes) {
        Ok(()) => Ok(bytes),
        Err(error) if error.code() == -25299 => key(false), // errSecDuplicateItem
        Err(_) => Err("无法创建 MiniMax 钥匙串密钥".into()),
    }
}

impl Vault for SystemVault {
    fn seal(&self, plain: &[u8]) -> Result<String, String> {
        #[cfg(windows)]
        {
            crypt(plain, true).map(|v| STANDARD.encode(v))
        }
        #[cfg(target_os = "macos")]
        {
            use aes_gcm::{aead::Aead, Aes256Gcm, KeyInit};
            let key = key(true)?;
            let cipher = Aes256Gcm::new_from_slice(&key).map_err(|_| "密钥无效")?;
            let random = uuid::Uuid::new_v4();
            let nonce = &random.as_bytes()[..12];
            let mut bytes = nonce.to_vec();
            bytes.extend(
                cipher
                    .encrypt(nonce.into(), plain)
                    .map_err(|_| "无法加密 MiniMax 账号")?,
            );
            Ok(STANDARD.encode(bytes))
        }
        #[cfg(not(any(windows, target_os = "macos")))]
        {
            let _ = plain;
            Err("MiniMax 接入仅支持 Windows x64 与 macOS Apple Silicon".into())
        }
    }
    fn open(&self, sealed: &str) -> Result<Vec<u8>, String> {
        let bytes = STANDARD
            .decode(sealed)
            .map_err(|_| "MiniMax 账号密文损坏")?;
        #[cfg(windows)]
        {
            crypt(&bytes, false)
        }
        #[cfg(target_os = "macos")]
        {
            use aes_gcm::{aead::Aead, Aes256Gcm, KeyInit};
            if bytes.len() < 28 {
                return Err("MiniMax 账号密文损坏".into());
            }
            let key = key(false)?;
            let cipher = Aes256Gcm::new_from_slice(&key).map_err(|_| "密钥无效")?;
            cipher
                .decrypt((&bytes[..12]).into(), &bytes[12..])
                .map_err(|_| "无法解密 MiniMax 账号".into())
        }
        #[cfg(not(any(windows, target_os = "macos")))]
        {
            let _ = bytes;
            Err("MiniMax 接入仅支持 Windows x64 与 macOS Apple Silicon".into())
        }
    }
}
