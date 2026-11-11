//! MiniMax Code 的凭据、身份与客户端路径。
//!
//! 桌面端与 CLI 共用同一个数据根 `~/.minimax`：每个区域只有一个登录槽
//! `auth/prod/{cn|en}/mcode-public/{auth.json,auth-state.json,auth.lock}`。
//! 布局来自官方开源实现 `MiniMax-AI/minimax-code` 的 `packages/oauth-core/src/namespace.ts`
//! 与本机 3.1.13 桌面端的实际写入结果。
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

/// 区域：国内版使用 `account.minimax.cn` / `agent.minimax.cn`，国际版使用 `.io`。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Region {
    Cn,
    En,
}

impl Region {
    pub fn text(self) -> &'static str {
        match self {
            Self::Cn => "cn",
            Self::En => "en",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Cn => "MiniMax Code 国内版",
            Self::En => "MiniMax Code 国际版",
        }
    }

    /// 业务接口（额度 / 会员 / 签到）所在的主机。
    pub fn origin(self) -> &'static str {
        match self {
            Self::Cn => "https://agent.minimax.cn",
            Self::En => "https://agent.minimax.io",
        }
    }

    /// OAuth 令牌端点所在的主机。
    pub fn account_origin(self) -> &'static str {
        match self {
            Self::Cn => "https://account.minimax.cn",
            Self::En => "https://account.minimax.io",
        }
    }

    /// 区域在凭据命名空间里的固定部分。
    pub fn namespace_service(self) -> String {
        format!("com.minimax.mcode.oauth.prod.{}", self.text())
    }

    /// 桌面端保存本机用户资料的配置文件名。国内版已在本机核实；国际版的名称
    /// 只是候选，读不到时按「身份未知」处理，不影响切换。
    fn config_file_names(self) -> &'static [&'static str] {
        match self {
            Self::Cn => &[
                "minimax-agent-cn-config.json",
                "minimax-agent-config.json",
                "config.json",
            ],
            Self::En => &[
                "minimax-agent-en-config.json",
                "minimax-agent-global-config.json",
                "minimax-agent-config.json",
                "config.json",
            ],
        }
    }
}

/// 可注入的根目录：生产取真实目录，测试指向临时沙箱。
#[derive(Clone)]
pub struct Paths {
    pub home: PathBuf,
    pub app_data: PathBuf,
    pub store: PathBuf,
}

impl Paths {
    pub fn real() -> Self {
        let home = crate::modules::config::home_dir();
        let isolated = std::env::var_os(crate::modules::config::HOME_ENV_VAR).is_some();
        let app_data = if cfg!(target_os = "macos") {
            home.join("Library/Application Support")
        } else if isolated {
            home.join("AppData/Roaming")
        } else {
            dirs::config_dir().unwrap_or_else(|| home.join(".config"))
        };
        Self {
            home,
            app_data,
            store: crate::modules::config::store_dir().join("minimax"),
        }
    }

    /// 桌面端与 CLI 的数据根；`MINIMAX_DATA_DIR` 可覆盖，但切换必须写默认位置，
    /// 否则改到的不是客户端真正读取的那份。
    pub fn data_dir(&self) -> PathBuf {
        self.home.join(".minimax")
    }

    pub fn auth_dir(&self, region: Region) -> PathBuf {
        self.data_dir()
            .join("auth")
            .join("prod")
            .join(region.text())
            .join("mcode-public")
    }

    pub fn auth_file(&self, region: Region) -> PathBuf {
        self.auth_dir(region).join("auth.json")
    }

    /// 凭据记录键里的账号片段。官方 `createAuthNamespace` 取
    /// `base64url(sha256("{dataDir}/auth\0mcode-public"))`，路径分隔符随平台；
    /// 工具内网页登录新建账号包时必须用同一个键，否则客户端读不到这份登录。
    pub fn credential_account(&self) -> String {
        use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
        use sha2::{Digest, Sha256};
        let auth_home = self.data_dir().join("auth");
        let seed = format!("{}\u{0}mcode-public", auth_home.display());
        URL_SAFE_NO_PAD.encode(Sha256::digest(seed.as_bytes()))
    }

    /// `auth.json` 里该区域的完整记录键：`{service}\0{account}`。
    pub fn credential_key(&self, region: Region) -> String {
        format!(
            "{}\u{0}{}",
            region.namespace_service(),
            self.credential_account()
        )
    }

    pub fn state_file(&self, region: Region) -> PathBuf {
        self.auth_dir(region).join("auth-state.json")
    }

    /// 桌面端的本地用户资料候选路径，按可信度排序。
    pub fn identity_candidates(&self, region: Region) -> Vec<PathBuf> {
        region
            .config_file_names()
            .iter()
            .map(|name| self.app_data.join("MiniMax").join(name))
            .collect()
    }

    /// 桌面端可执行文件候选位置；运行中的实例优先用真实路径。
    pub fn executable(&self, region: Region) -> Option<PathBuf> {
        let name = "MiniMax Code";
        if cfg!(target_os = "macos") {
            let bundled = PathBuf::from("/Applications").join(format!("{name}.app"));
            if bundled.is_dir() {
                return Some(bundled);
            }
            let local = self.home.join("Applications").join(format!("{name}.app"));
            return local.is_dir().then_some(local);
        }
        let mut candidates = Vec::new();
        for drive in ['D', 'C'] {
            candidates.push(
                PathBuf::from(format!("{drive}:\\Program Files"))
                    .join(name)
                    .join(format!("{name}.exe")),
            );
            candidates.push(
                PathBuf::from(format!("{drive}:\\Program Files"))
                    .join(name.to_lowercase())
                    .join(format!("{name}.exe")),
            );
        }
        candidates.push(
            self.home
                .join("AppData/Local/Programs")
                .join(name)
                .join(format!("{name}.exe")),
        );
        let _ = region;
        candidates.into_iter().find(|p| p.is_file())
    }

    /// 是否检测到该区域的客户端（安装过或登录过）。
    pub fn installed(&self, region: Region) -> bool {
        self.auth_dir(region).is_dir()
            || self.executable(region).is_some()
            || self.app_data.join("MiniMax").is_dir()
    }
}

/// 桌面端进程映像名（Windows 带 .exe）。
pub fn desktop_image() -> &'static str {
    if cfg!(target_os = "windows") {
        "MiniMax Code.exe"
    } else {
        "MiniMax Code"
    }
}

/// macOS 的进程路径标记。
#[cfg(target_os = "macos")]
pub fn macos_process_marker() -> &'static str {
    "MiniMax Code.app/Contents/MacOS"
}
