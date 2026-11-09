use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Region {
    Cn,
    Ai,
}
impl Region {
    pub fn text(self) -> &'static str {
        match self {
            Self::Cn => "cn",
            Self::Ai => "ai",
        }
    }
}
#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Client {
    Desktop,
}
impl Client {
    pub fn text(self) -> &'static str {
        match self {
            Self::Desktop => "desktop",
        }
    }
    pub fn app_name(self, region: Region) -> &'static str {
        match (self, region) {
            (Self::Desktop, Region::Cn) => "Trae CN",
            (Self::Desktop, Region::Ai) => "Trae",
        }
    }
}
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
        let store = home.join(".wb-switch/trae");
        Self {
            home,
            app_data,
            store,
        }
    }
    pub fn storage(&self, region: Region, client: Client) -> PathBuf {
        self.app_data
            .join(client.app_name(region))
            .join("User/globalStorage/storage.json")
    }
    pub fn executable(&self, region: Region, client: Client) -> Option<PathBuf> {
        let name = client.app_name(region);
        if cfg!(target_os = "macos") {
            let p = PathBuf::from("/Applications").join(format!("{name}.app"));
            return p.is_dir().then_some(p);
        }
        let mut candidates = Vec::new();
        for drive in ['D', 'C'] {
            candidates.push(
                PathBuf::from(format!("{drive}:\\Program Files"))
                    .join(name)
                    .join(format!("{name}.exe")),
            );
        }
        candidates.push(
            self.home
                .join("AppData/Local/Programs")
                .join(name)
                .join(format!("{name}.exe")),
        );
        candidates.into_iter().find(|p| p.is_file())
    }
}
