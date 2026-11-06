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
}

impl Target {
    pub fn parse(value: &str) -> Result<Self, String> {
        match value {
            "codebuddy-cli" => Ok(Self::CodebuddyCli),
            "qoder-cli" => Ok(Self::QoderCli),
            "qoder-desktop" => Ok(Self::QoderDesktop),
            _ => Err("--target must be codebuddy-cli, qoder-cli, or qoder-desktop".into()),
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::CodebuddyCli => "codebuddy-cli",
            Self::QoderCli => "qoder-cli",
            Self::QoderDesktop => "qoder-desktop",
        }
    }

    pub fn is_qoder(self) -> bool {
        matches!(self, Self::QoderCli | Self::QoderDesktop)
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
