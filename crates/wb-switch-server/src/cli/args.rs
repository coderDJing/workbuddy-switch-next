//! Command syntax and compatibility with the original webui entry point.
use super::{protocol::Failure, target::Target};
use std::collections::BTreeMap;
use wb_switch_core::modules::variant::WbVariant;

pub const HELP: &str = "workbuddy-switch-next [COMMAND] [OPTIONS]

The legacy workbuddy-switch command is also supported by the npm package.

Commands:
  serve                         Start webui (default when no command is given)
  status --target TARGET        Show selected client authentication status
  accounts [list]               List account metadata (never credentials)
  inspect [ACCOUNT_ID]           One call: accounts, CLI status, fresh credits and model limits
  switch ACCOUNT_ID             Switch selected client (default: CodeBuddy CLI)
  credits [ACCOUNT_ID | --all]   Query credit balances (default: all accounts)
  limits [ACCOUNT_ID]            Read observed model limits and recovery times
  token-stats                   Platform Token usage (unknown counters stay unavailable)
  credit-stats                  Platform credit statistics; --refresh queries official usage
  rewards ACCOUNT_ID|--all       Daily check-in / Qoder rewards (explicit selection required)
  automation [get|set|run]       Platform automation settings/history; run only enabled tasks
  login                         Wait for official authorization, save account, do not switch clients
                                 (minimax-desktop: prints a device user code, then polls until authorized)
  operations [OPERATION]        Discover complete WebUI operations and input contracts
  call OPERATION                Run a named operation in-process, without starting a server
  version                       Show version

One-shot options:
  --text                        Human-readable output instead of default JSON
  --json                        Explicit JSON output (kept for existing scripts)
  --ascii-json                  JSON with Unicode escapes for legacy pipes (implies --json)
  --summary                     Compact inspect/credits output; keeps raw balances and 2-decimal display strings
  --variant cn|ai|all            Filter accounts (default: all)
  --target TARGET               codebuddy-cli (default), workbuddy, codebuddy-ide, vscode, jetbrains,
                                qoder-cli, qoder-desktop, trae-desktop, minimax-desktop, doubao-desktop
  --dry-run                     Preview switch target without writing or closing clients
  --close-running               Explicitly allow closing a desktop/Qoder target
  --relaunch                    Desktop restart; requires --close-running (new desktop targets require both)
  --input-file PATH             call: JSON object file; - reads stdin (keeps secrets out of process arguments)
  --output-file PATH            call: new output file; mandatory for credential exports, never overwrites
  --settings-file PATH          automation set: JSON patch for --variant cn|ai; unknown fields rejected
  --refresh                     credit-stats: query selected region before reading snapshots
  --timeout SECONDS             Credit query timeout per account (default: 30)
  --concurrency N               Credit queries in flight (default: 4, maximum: 16)
  --model MODEL                 Filter observed limits by exact model name
  --wait SECONDS                Wait to read/switch auth state (default: 30; 0: fail fast)
  --quiet                       Suppress progress on stderr (stdout remains unchanged)
  -h, --help                    Show help

Serve options: --port PORT (default: 57890), --no-open
Legacy status without --target still shows WorkBuddy (optional --variant cn|ai).
Exit codes: 0 success; 1 operation failed; 2 invalid arguments/account; 3 partial credit results.
Exit code 130: interrupted; in-flight authentication updates finish before exit.
New one-shot commands default to JSON; help, version and legacy status default to text.
Start with inspect for IDs, balances and recovery times in one call.
Use inspect --summary for a compact AI overview without full account/resource details.
Credit *Display strings are rounded to 2 decimals for presentation; numeric fields keep original precision.
Progress goes to stderr; in JSON mode, progress is JSON Lines and stdout is one final object.
Streams use UTF-8 without BOM; --ascii-json escapes non-ASCII characters losslessly.
Qoder targets use an independent account store and stable IDs from accounts.
WorkBuddy/CodeBuddy token-stats and credit-stats cover the platform, not one client or region.
Use operations for account CRUD/import/export, sessions, automation configuration, hooks and WebDAV.
login emits official authorization instructions to stderr and waits in the same process.
Credential exports are written only to an explicit file; ordinary call results redact secrets.
Qoder status defaults to both regions; --variant cn|ai selects one.
Qoder switching refuses running clients unless --close-running is given.
Qoder limits returns UNSUPPORTED_CAPABILITY; inspect reports availabilityKnown=false.
Automation run does not start a daemon; scheduling requires the desktop app or serve.
No observed limit does not prove a model is available.";

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Command {
    Serve,
    Status,
    LegacyStatus,
    Accounts,
    Inspect,
    Switch,
    Credits,
    Limits,
    TokenStats,
    CreditStats,
    Rewards,
    Automation,
    Operations,
    Call,
    Login,
    Version,
    Help,
}

impl Command {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Serve => "serve",
            Self::Status | Self::LegacyStatus => "status",
            Self::Accounts => "accounts",
            Self::Inspect => "inspect",
            Self::Switch => "switch",
            Self::Credits => "credits",
            Self::Limits => "limits",
            Self::TokenStats => "token-stats",
            Self::CreditStats => "credit-stats",
            Self::Rewards => "rewards",
            Self::Automation => "automation",
            Self::Operations => "operations",
            Self::Call => "call",
            Self::Login => "login",
            Self::Version => "version",
            Self::Help => "help",
        }
    }
}

pub(super) fn json_by_default(command: &str, explicit_target: bool) -> bool {
    matches!(
        command,
        "accounts"
            | "operations"
            | "call"
            | "login"
            | "inspect"
            | "switch"
            | "credits"
            | "limits"
            | "token-stats"
            | "credit-stats"
            | "rewards"
            | "automation"
    ) || (command == "status" && explicit_target)
}

#[derive(Debug, Clone)]
pub struct Options {
    pub command: Command,
    pub target: Target,
    pub json: bool,
    pub ascii_json: bool,
    pub summary: bool,
    pub variant: Option<WbVariant>,
    pub account_id: Option<String>,
    pub dry_run: bool,
    pub timeout: u64,
    pub concurrency: usize,
    pub model: Option<String>,
    pub port: u16,
    pub no_open: bool,
    pub wait: u64,
    pub quiet: bool,
    pub action: String,
    pub settings_file: Option<String>,
    pub refresh: bool,
    pub close_running: bool,
    pub relaunch: bool,
    pub input_file: Option<String>,
    pub output_file: Option<String>,
}
pub fn parse(args: &[String]) -> Result<Options, Failure> {
    let mut options = Options {
        command: Command::Serve,
        target: Target::default(),
        json: false,
        ascii_json: false,
        summary: false,
        variant: None,
        account_id: None,
        dry_run: false,
        timeout: 30,
        concurrency: 4,
        model: None,
        port: 57890,
        no_open: false,
        wait: 30,
        quiet: false,
        action: "get".into(),
        settings_file: None,
        refresh: false,
        close_running: false,
        relaunch: false,
        input_file: None,
        output_file: None,
    };
    let mut positional = Vec::new();
    let mut flags = BTreeMap::new();
    let mut i = 0;
    while i < args.len() {
        let arg = &args[i];
        if arg == "--" {
            positional.extend(args[i + 1..].iter().cloned());
            break;
        }
        if !arg.starts_with('-') {
            positional.push(arg.clone());
            i += 1;
            continue;
        }
        let (key, inline) = arg
            .split_once('=')
            .map_or((arg.as_str(), None), |(k, v)| (k, Some(v)));
        let key = match key {
            "-h" => "--help",
            "-V" => "--version",
            other => other,
        };
        let takes_value = matches!(
            key,
            "--variant"
                | "--target"
                | "--timeout"
                | "--concurrency"
                | "--model"
                | "--port"
                | "--wait"
                | "--settings-file"
                | "--input-file"
                | "--output-file"
        );
        if !takes_value
            && !matches!(
                key,
                "--json"
                    | "--text"
                    | "--ascii-json"
                    | "--all"
                    | "--dry-run"
                    | "--no-open"
                    | "--help"
                    | "--version"
                    | "--quiet"
                    | "--summary"
                    | "--refresh"
                    | "--close-running"
                    | "--relaunch"
            )
        {
            return Err(Failure::usage(format!("Unknown option: {key}")));
        }
        let value = if takes_value {
            match inline {
                Some(value) if !value.is_empty() => value.to_string(),
                Some(_) => return Err(Failure::usage(format!("Missing value for {key}"))),
                None => {
                    i += 1;
                    args.get(i)
                        .filter(|v| !v.starts_with('-'))
                        .cloned()
                        .ok_or_else(|| Failure::usage(format!("Missing value for {key}")))?
                }
            }
        } else {
            if inline.is_some() {
                return Err(Failure::usage(format!("{key} does not take a value")));
            }
            String::new()
        };
        if flags.insert(key.to_string(), value).is_some() {
            return Err(Failure::usage(format!("Duplicate option: {key}")));
        }
        i += 1;
    }
    options.ascii_json = flags.contains_key("--ascii-json");
    options.json = flags.contains_key("--json") || options.ascii_json;
    if flags.contains_key("--text") && options.json {
        return Err(Failure::usage(
            "--text cannot be combined with --json or --ascii-json",
        ));
    }
    let cmd = positional.first().map(String::as_str).unwrap_or("serve");
    options.command = match cmd {
        "serve" => Command::Serve,
        "status" if flags.contains_key("--target") => Command::Status,
        "status" => Command::LegacyStatus,
        "accounts" => Command::Accounts,
        "inspect" => Command::Inspect,
        "switch" => Command::Switch,
        "credits" => Command::Credits,
        "limits" => Command::Limits,
        "token-stats" => Command::TokenStats,
        "credit-stats" => Command::CreditStats,
        "rewards" => Command::Rewards,
        "automation" => Command::Automation,
        "operations" => Command::Operations,
        "call" => Command::Call,
        "login" => Command::Login,
        "version" => Command::Version,
        "help" => Command::Help,
        _ => {
            return Err(Failure::usage(format!(
                "Unknown command: {cmd}. Use --help."
            )))
        }
    };
    if let Some(target) = flags.get("--target") {
        options.target = Target::parse(target).map_err(Failure::usage)?;
    }
    if flags.contains_key("--help") || options.command == Command::Help {
        options.command = Command::Help;
        return Ok(options);
    }
    if flags.contains_key("--version") {
        if !positional.is_empty() {
            return Err(Failure::usage(
                "--version cannot be combined with a command",
            ));
        }
        options.command = Command::Version;
    }
    options.json |= !flags.contains_key("--text")
        && json_by_default(options.command.as_str(), flags.contains_key("--target"));
    if options.command == Command::LegacyStatus && options.json {
        return Err(Failure::usage(
            "Use status --target codebuddy-cli for CLI authentication status",
        ));
    }
    // The complete named-operation interface has its own strict syntax. It
    // does not inherit the legacy target defaults or accept arbitrary URLs.
    if matches!(options.command, Command::Operations | Command::Call) {
        for key in flags.keys() {
            if !(matches!(
                key.as_str(),
                "--json" | "--ascii-json" | "--text" | "--quiet"
            ) || (options.command == Command::Call
                && matches!(key.as_str(), "--input-file" | "--output-file")))
            {
                return Err(Failure::usage(format!("{key} is not supported for {cmd}")));
            }
        }
        let rest = &positional[1..];
        if rest.len() > 1 || (options.command == Command::Call && rest.len() != 1) {
            return Err(Failure::usage("Use operations [OPERATION] or call OPERATION [--input-file PATH] [--output-file PATH]"));
        }
        options.action = rest.first().cloned().unwrap_or_default();
        options.input_file = flags.get("--input-file").cloned();
        options.output_file = flags.get("--output-file").cloned();
        options.quiet = flags.contains_key("--quiet");
        return Ok(options);
    }
    for key in flags.keys() {
        let allowed = match key.as_str() {
            "--json" | "--ascii-json" | "--text" => options.command != Command::Serve,
            "--variant" => {
                ((options.target.is_qoder()
                    || options.target.is_desktop()
                    || options.target.is_trae()
                    || options.target.is_minimax()
                    || options.target.is_doubao())
                    && options.command == Command::Status)
                    || matches!(
                        options.command,
                        Command::Accounts
                            | Command::Inspect
                            | Command::LegacyStatus
                            | Command::Switch
                            | Command::Credits
                            | Command::Limits
                            | Command::TokenStats
                            | Command::CreditStats
                            | Command::Rewards
                            | Command::Automation
                            | Command::Login
                    )
            }
            "--target" => matches!(
                options.command,
                Command::Status
                    | Command::Inspect
                    | Command::Accounts
                    | Command::Switch
                    | Command::Credits
                    | Command::Limits
                    | Command::TokenStats
                    | Command::CreditStats
                    | Command::Rewards
                    | Command::Automation
                    | Command::Login
            ),
            "--dry-run" => options.command == Command::Switch,
            "--close-running" | "--relaunch" => {
                options.command == Command::Switch
                    && (options.target.is_qoder()
                        || options.target.is_desktop()
                        || options.target.is_trae()
                        || options.target.is_minimax()
                        || options.target.is_doubao())
            }
            "--settings-file" => options.command == Command::Automation,
            "--refresh" => options.command == Command::CreditStats,
            "--all" => matches!(
                options.command,
                Command::Credits | Command::Inspect | Command::Rewards
            ),
            "--timeout" | "--concurrency" => {
                matches!(options.command, Command::Credits | Command::Inspect)
                    || (options.command == Command::CreditStats && flags.contains_key("--refresh"))
            }
            "--model" => matches!(options.command, Command::Limits | Command::Inspect),
            "--summary" => matches!(options.command, Command::Inspect | Command::Credits),
            "--wait" => matches!(
                options.command,
                Command::Accounts
                    | Command::Status
                    | Command::Switch
                    | Command::Credits
                    | Command::Limits
                    | Command::Inspect
                    | Command::Rewards
                    | Command::CreditStats
            ),
            "--quiet" => matches!(
                options.command,
                Command::Accounts
                    | Command::Status
                    | Command::Switch
                    | Command::Credits
                    | Command::Limits
                    | Command::TokenStats
                    | Command::CreditStats
                    | Command::Rewards
                    | Command::Automation
                    | Command::Inspect
            ),
            "--port" | "--no-open" => options.command == Command::Serve,
            "--version" => options.command == Command::Version,
            _ => false,
        };
        if !allowed {
            return Err(Failure::usage(format!("{key} is not supported for {cmd}")));
        }
    }
    if let Some(value) = flags.get("--variant") {
        options.variant = match value.to_ascii_lowercase().as_str() {
            "cn" => Some(WbVariant::Cn),
            "ai" => Some(WbVariant::Ai),
            "all" => None,
            _ => return Err(Failure::usage("--variant must be cn, ai, or all")),
        };
        if options.command == Command::LegacyStatus && options.variant.is_none() {
            return Err(Failure::usage(
                "Legacy status supports --variant cn|ai; use --target codebuddy-cli for CLI status",
            ));
        }
    } else if options.command == Command::LegacyStatus {
        options.variant = Some(WbVariant::Cn);
    }
    if let Some(value) = flags.get("--timeout") {
        options.timeout = positive_number(value, "--timeout", 3600)?;
    }
    if let Some(value) = flags.get("--concurrency") {
        options.concurrency = positive_number(value, "--concurrency", 16)? as usize;
    }
    if let Some(value) = flags.get("--port") {
        options.port = positive_number(value, "--port", u16::MAX as u64)? as u16;
    }
    options.model = flags.get("--model").cloned();
    options.dry_run = flags.contains_key("--dry-run");
    options.no_open = flags.contains_key("--no-open");
    options.quiet = flags.contains_key("--quiet");
    options.summary = flags.contains_key("--summary");
    if let Some(value) = flags.get("--wait") {
        options.wait = value
            .parse::<u64>()
            .ok()
            .filter(|v| *v <= 3600)
            .ok_or_else(|| Failure::usage("--wait must be an integer between 0 and 3600"))?;
    }
    options.settings_file = flags.get("--settings-file").cloned();
    options.refresh = flags.contains_key("--refresh");
    options.close_running = flags.contains_key("--close-running");
    options.relaunch = flags.contains_key("--relaunch");
    let mut rest = positional.into_iter().skip(1).collect::<Vec<_>>();
    if options.command == Command::Accounts && rest.first().is_some_and(|v| v == "list") {
        rest.remove(0);
    }
    if options.command == Command::Automation {
        if rest.len() > 1 {
            return Err(Failure::usage("automation accepts get, set, or run"));
        }
        options.action = rest.pop().unwrap_or_else(|| "get".into());
        if !matches!(options.action.as_str(), "get" | "set" | "run")
            || (options.action == "set") != options.settings_file.is_some()
        {
            return Err(Failure::usage(
                "Use automation get|run, or automation set --variant cn|ai --settings-file PATH",
            ));
        }
        if options.action == "set" && options.variant.is_none() && !options.target.is_doubao() {
            return Err(Failure::usage("automation set requires --variant cn or ai"));
        }
    }
    if matches!(options.command, Command::Automation)
        && !options.target.is_qoder()
        && !options.target.is_trae()
        && !options.target.is_minimax()
        && !options.target.is_doubao()
    {
        return Err(Failure::usage(
            "This command requires --target qoder-cli, qoder-desktop trae-desktop or minimax-desktop",
        ));
    }
    if options.relaunch
        && (!(options.target == Target::QoderDesktop
            || options.target.is_desktop()
            || options.target.is_trae()
            || options.target.is_minimax()
            || options.target.is_doubao())
            || !options.close_running)
    {
        return Err(Failure::usage(
            "--relaunch requires a desktop target and --close-running",
        ));
    }
    if options.target.is_desktop() && options.close_running != options.relaunch {
        return Err(Failure::usage("Desktop switching uses --close-running --relaunch together (the existing client switch transaction restarts the client)"));
    }
    if matches!(options.target, Target::Vscode | Target::Jetbrains)
        && options.variant == Some(WbVariant::Ai)
    {
        return Err(Failure::usage(
            "This plugin target only supports --variant cn",
        ));
    }
    if options.command == Command::Login
        && flags.contains_key("--variant")
        && options.variant.is_none()
    {
        return Err(Failure::usage(
            "This command requires one region: --variant cn or ai",
        ));
    }
    if matches!(options.command, Command::TokenStats | Command::CreditStats)
        && !options.target.is_qoder()
        && !options.target.is_trae()
        && !options.target.is_minimax()
        && !options.target.is_doubao()
        && flags.contains_key("--variant")
    {
        return Err(Failure::usage(
            "WorkBuddy/CodeBuddy statistics are platform-wide; do not pass --variant",
        ));
    }
    if options.command == Command::CreditStats
        && !options.target.is_qoder()
        && (flags.contains_key("--timeout") || flags.contains_key("--concurrency"))
    {
        return Err(Failure::usage(
            "Platform credit-stats does not support per-account --timeout or --concurrency",
        ));
    }
    if options.target.is_trae()
        && ["--model", "--concurrency", "--wait"]
            .iter()
            .any(|k| flags.contains_key(*k))
    {
        return Err(Failure::usage(
            "TRAE queries are sequential; --model, --concurrency and --wait are unsupported",
        ));
    }
    if options.target.is_minimax()
        && ["--model", "--concurrency", "--wait"]
            .iter()
            .any(|k| flags.contains_key(*k))
    {
        return Err(Failure::usage(
            "MiniMax queries are sequential; --model, --concurrency and --wait are unsupported",
        ));
    }
    if options.target.is_doubao()
        && (options.variant == Some(WbVariant::Ai)
            || ["--model", "--concurrency", "--wait"]
                .iter()
                .any(|k| flags.contains_key(*k)))
    {
        return Err(Failure::usage("Doubao supports the cn desktop only; --model, --concurrency and --wait are unsupported"));
    }
    if options.target.is_qoder() && flags.contains_key("--model") {
        return Err(Failure::usage(
            "Qoder model availability is unknown; --model is not supported",
        ));
    }
    match options.command {
        Command::Switch if rest.len() != 1 => {
            return Err(Failure::usage("switch requires exactly one ACCOUNT_ID"))
        }
        Command::Switch
        | Command::Credits
        | Command::Limits
        | Command::Inspect
        | Command::Rewards
            if rest.len() <= 1 =>
        {
            options.account_id = rest.pop()
        }
        _ if !rest.is_empty() => return Err(Failure::usage("Unexpected positional arguments")),
        _ => {}
    }
    if flags.contains_key("--all") && options.account_id.is_some() {
        return Err(Failure::usage("--all cannot be combined with ACCOUNT_ID"));
    }
    if options.command == Command::Rewards
        && options.account_id.is_none()
        && !flags.contains_key("--all")
    {
        return Err(Failure::usage(
            "rewards requires ACCOUNT_ID or explicit --all",
        ));
    }
    Ok(options)
}

fn positive_number(value: &str, flag: &str, max: u64) -> Result<u64, Failure> {
    value
        .parse::<u64>()
        .ok()
        .filter(|v| *v > 0 && *v <= max)
        .ok_or_else(|| Failure::usage(format!("{flag} must be an integer between 1 and {max}")))
}
