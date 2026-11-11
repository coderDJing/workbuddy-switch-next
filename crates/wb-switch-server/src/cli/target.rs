//! Client-specific operations are dispatched here. Account storage and balances
//! remain shared services. Add a target only when its operations are implemented.
use serde_json::Value;
use wb_switch_core::modules::{codebuddy_cli, limits};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Target {
    #[default]
    CodebuddyCli,
    QoderCli,
    QoderDesktop,
    TraeDesktop,
    MinimaxDesktop,
    Workbuddy,
    CodebuddyIde,
    Vscode,
    Jetbrains,
}

impl Target {
    pub fn parse(value: &str) -> Result<Self, String> {
        match value {
            "codebuddy-cli" => Ok(Self::CodebuddyCli),
            "qoder-cli" => Ok(Self::QoderCli),
            "qoder-desktop" => Ok(Self::QoderDesktop),
            "trae-desktop" => Ok(Self::TraeDesktop),
            "minimax-desktop" => Ok(Self::MinimaxDesktop),
            "workbuddy" => Ok(Self::Workbuddy),
            "codebuddy-ide" => Ok(Self::CodebuddyIde),
            "vscode" => Ok(Self::Vscode),
            "jetbrains" => Ok(Self::Jetbrains),
            _ => Err("Unknown target; use help --json for supportedTargets".into()),
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::CodebuddyCli => "codebuddy-cli",
            Self::QoderCli => "qoder-cli",
            Self::QoderDesktop => "qoder-desktop",
            Self::TraeDesktop => "trae-desktop",
            Self::MinimaxDesktop => "minimax-desktop",
            Self::Workbuddy => "workbuddy",
            Self::CodebuddyIde => "codebuddy-ide",
            Self::Vscode => "vscode",
            Self::Jetbrains => "jetbrains",
        }
    }

    pub fn is_trae(self) -> bool {
        matches!(self, Self::TraeDesktop)
    }
    /// MiniMax Code：桌面端与 CLI 共用一个登录槽，所以只有一个目标。
    pub fn is_minimax(self) -> bool {
        matches!(self, Self::MinimaxDesktop)
    }
    pub fn is_qoder(self) -> bool {
        matches!(self, Self::QoderCli | Self::QoderDesktop)
    }
    pub fn is_desktop(self) -> bool {
        matches!(
            self,
            Self::Workbuddy | Self::CodebuddyIde | Self::Vscode | Self::Jetbrains
        )
    }

    pub fn status_for(self, variant: Option<wb_switch_core::modules::variant::WbVariant>) -> Value {
        use wb_switch_core::modules::{
            account, auth_file, codebuddy_cn_ide, codebuddy_ide, jetbrains, process,
            variant::WbVariant, vscode_ext,
        };
        if variant.is_none() && matches!(self, Self::Workbuddy | Self::CodebuddyIde) {
            return serde_json::json!({"clients":[self.status_for(Some(WbVariant::Cn)), self.status_for(Some(WbVariant::Ai))]});
        }
        let variant = variant.unwrap_or(WbVariant::Cn);
        let mut status = match self {
            Self::Workbuddy => {
                let auth = auth_file::read_auth_file(variant);
                let uid = auth.as_ref().and_then(|v| v["account"]["uid"].as_str());
                let current = account::load_accounts().into_iter().find(|a| {
                    account::variant_of(a) == variant && uid.is_some() && a["uid"].as_str() == uid
                });
                serde_json::json!({"running":process::is_workbuddy_running(variant),
                    "configured":auth.is_some(), "activeAccountId":current.as_ref().map(|a|&a["id"]),
                    "activeAccountName":current.as_ref().map(account::account_display_name), "variant":variant.as_str()})
            }
            Self::CodebuddyIde if variant == WbVariant::Ai => codebuddy_ide::status(),
            Self::CodebuddyIde => codebuddy_cn_ide::status(),
            Self::Vscode => vscode_ext::status(),
            Self::Jetbrains => jetbrains::status(),
            _ => self.status(),
        };
        if status.get("configured").is_none() {
            status["configured"] = status.get("loggedIn").cloned().unwrap_or(Value::Null);
        }
        if self.is_desktop() {
            status["variant"] = serde_json::json!(variant.as_str());
        }
        status
    }
    pub fn qoder_target(self, global: bool) -> wb_switch_core::modules::qoder::QoderTarget {
        use wb_switch_core::modules::qoder::QoderTarget as Q;
        match (self, global) {
            (Self::QoderDesktop, false) => Q::Desktop,
            (Self::QoderDesktop, true) => Q::GlobalDesktop,
            (_, false) => Q::Cli,
            (_, true) => Q::GlobalCli,
        }
    }

    /// Caller holds the shared authentication lock for this snapshot.
    pub fn status(self) -> Value {
        match self {
            Self::CodebuddyCli => codebuddy_cli::status(),
            _ => serde_json::Value::Null, // Qoder is routed before the shared account pipeline.
        }
    }

    /// Core owns the complete authentication transaction and process policy.
    pub fn switch(self, account_id: &str) -> Result<Value, String> {
        match self {
            Self::CodebuddyCli => codebuddy_cli::switch_active_account(account_id),
            _ => Err("Qoder requires its isolated account dispatcher".into()),
        }
    }

    /// A read-only OS process snapshot. It never claims that a detected CLI
    /// session is idle and never returns process command lines or history.
    pub fn switch_preview(self) -> Result<Value, String> {
        match self {
            Self::CodebuddyCli => codebuddy_cli::switch_process_preview(),
            _ => Err("Qoder requires its isolated process preview".into()),
        }
    }

    pub fn limits(self) -> Value {
        match self {
            Self::CodebuddyCli => limits::get_cli_rate_limits_snapshot(),
            _ => serde_json::Value::Null,
        }
    }

    pub fn limits_source(self) -> &'static str {
        match self {
            Self::CodebuddyCli => "codebuddy-cli-logs-and-shared-account-hook-ledger",
            _ => "unavailable",
        }
    }
}
