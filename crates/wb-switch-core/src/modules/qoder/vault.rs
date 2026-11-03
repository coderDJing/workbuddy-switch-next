//! Windows prototype: keep authorization independent of any Qoder installation.
//! The ciphertext is bound to this Windows user by DPAPI.

use serde::{Deserialize, Serialize};
use windows::{
    core::PCWSTR,
    Win32::{
        Foundation::{LocalFree, HLOCAL},
        Security::Cryptography::{CryptProtectData, CryptUnprotectData, CRYPT_INTEGER_BLOB},
    },
};

use super::authorized::AuthorizedAccount;

pub(super) const FILE: &str = "authorized.dpapi";

#[derive(Serialize, Deserialize)]
struct Vault {
    account: AuthorizedAccount,
    machine: String,
}

fn crypt(input: &[u8], protect: bool) -> Result<Vec<u8>, String> {
    let size = u32::try_from(input.len()).map_err(|_| "Qoder 账号数据过大")?;
    let source = CRYPT_INTEGER_BLOB {
        cbData: size,
        pbData: input.as_ptr() as *mut u8,
    };
    let mut output = CRYPT_INTEGER_BLOB::default();
    unsafe {
        if protect {
            CryptProtectData(&source, PCWSTR::null(), None, None, None, 1, &mut output)
                .map_err(|_| "无法加密 Qoder 账号")?;
        } else {
            CryptUnprotectData(&source, None, None, None, None, 1, &mut output)
                .map_err(|_| "无法解密 Qoder 账号，请重新添加")?;
        }
        let result = std::slice::from_raw_parts(output.pbData, output.cbData as usize).to_vec();
        let _ = LocalFree(HLOCAL(output.pbData as _));
        Ok(result)
    }
}

pub(super) fn protect_bytes(input: &[u8]) -> Result<Vec<u8>, String> {
    crypt(input, true)
}

pub(super) fn seal(account: &AuthorizedAccount, machine: &str) -> Result<Vec<u8>, String> {
    let data = serde_json::to_vec(&Vault {
        account: AuthorizedAccount {
            access_token: account.access_token.clone(),
            refresh_token: account.refresh_token.clone(),
            expires_at: account.expires_at.clone(),
            refresh_expires_at: account.refresh_expires_at.clone(),
            user: account.user.clone(),
        },
        machine: machine.to_string(),
    })
    .map_err(|_| "无法编码 Qoder 账号")?;
    crypt(&data, true)
}

pub(super) fn open(bytes: &[u8]) -> Result<(AuthorizedAccount, String), String> {
    let plain = crypt(bytes, false)?;
    let vault: Vault = serde_json::from_slice(&plain).map_err(|_| "Qoder 账号数据损坏")?;
    Ok((vault.account, vault.machine))
}
