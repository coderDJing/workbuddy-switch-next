use std::path::PathBuf;

#[derive(Clone)]
pub(super) struct Paths {
    pub data: PathBuf,
    pub store: PathBuf,
    pub home: PathBuf,
    pub isolated: bool,
}
impl Paths {
    pub fn real() -> Self {
        let home = crate::modules::config::home_dir();
        let isolated = std::env::var_os(crate::modules::config::HOME_ENV_VAR).is_some();
        let data = if cfg!(target_os = "macos") {
            home.join("Library/Application Support/Doubao")
        } else if isolated {
            home.join("AppData/Local/Doubao/User Data")
        } else {
            dirs::data_local_dir().unwrap_or_else(|| home.join("AppData/Local")).join("Doubao/User Data")
        };
        Self { data, store: crate::modules::config::store_dir().join("doubao"), home, isolated }
    }
    pub fn executable(&self) -> Option<PathBuf> {
        if self.isolated { return None; }
        let candidates = if cfg!(target_os = "macos") {
            vec![PathBuf::from("/Applications/Doubao.app"), PathBuf::from("/Applications/豆包.app"), self.home.join("Applications/Doubao.app")]
        } else {
            vec![PathBuf::from("D:/Program Files/Doubao/Doubao.exe"), PathBuf::from("C:/Program Files/Doubao/Doubao.exe"), self.home.join("AppData/Local/Doubao/Application/Doubao.exe")]
        };
        candidates.into_iter().find(|p| p.exists())
    }
}
