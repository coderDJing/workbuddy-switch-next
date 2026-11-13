import operationRoutes from "./operations.json";
import { invoke } from "@tauri-apps/api/core";
import type {
  AccountMeta,
  AccountRecord,
  AppNotification,
  AppStatus,
  AutoRotateConfig,
  CodeBuddyCliInstallResult,
  CodeBuddyCliStatus,
  CodeBuddyCliSwitchResult,
  CodeBuddyCnIdeStatus,
  CodeBuddyCnIdeSwitchResult,
  CheckinConfig,
  CheckinLog,
  CheckinResult,
  CreditExpiry,
  CreditStatistics,
  DisplayField,
  TokenStatistics,
  ErrorLogKind,
  GithubConfig,
  ImportPreviewAccount,
  ImportResult,
  OAuthPollResult,
  OAuthStartResult,
  RateLimitConfig,
  RateLimitHookStatus,
  RateLimitsPayload,
  RotateLog,
  RotateStatus,
  Session,
  SessionCopyReport,
  SessionLinksPreview,
  SessionSyncReport,
  SessionSyncSelection,
  SessionSyncMode,
  SessionGroupClient,
  SessionGroupList,
  SessionGroupDetail,
  SessionGroupPairPreview,
  SessionGroupActionReport,
  SwitchResult,
  TravelConfig,
  TravelStatus,
  UpdateInfo,
  UpdateSnapshot,
  VscodeExtStatus,
  VscodeExtSwitchResult,
  JetbrainsStatus,
  JetbrainsSwitchResult,
  QoderBundle,
  QoderCredit,
  QoderStatus,
  QoderSwitchResult,
  QoderTarget,
  VscodeSessionList,
  VscodeSessionRef,
  WbVariant,
  WebdavConfig,
  WebdavConfigInput,
  WebdavRemoteInfo,
  WebdavActionResult,
  WebdavRestoreResult,
  WebdavRestorePreview,
} from "./types";
import { DEMO_UNAVAILABLE_MESSAGE, demoModeEnabled } from "./demo-mode";
import { screenshotDemoResponse } from "./screenshot-demo";
import { RC_DIAGNOSTICS_ENABLED } from "./update";
import { UpdateCheckTimeoutError, waitForUpdateCheck } from "./update-check";

/**
 * 双通道适配层：
 * - 桌面 App（Tauri）：`invoke` 调用 Rust commands
 * - webui（浏览器）：HTTP fetch 调用同源服务（服务端口 / SSH 转发端口均可）
 */

const DEMO_READ_COMMANDS = new Set([
  "get_doubao_status", "get_minimax_status", "get_trae_status", "get_status", "get_accounts", "get_codebuddy_cli_status", "get_codebuddy_cn_ide_status", "get_codebuddy_ide_status", "get_vscode_ext_status", "get_jetbrains_status", "get_qoder_status", "get_qoder_credits", "list_vscode_sessions", "list_codebuddy_ide_sessions", "list_codebuddy_intl_ide_sessions", "vscode_session_links_preview", "codebuddy_ide_session_links_preview", "codebuddy_intl_ide_session_links_preview", "list_account_sessions", "session_links_preview_cross", "list_session_groups", "get_session_group", "preview_session_group_pair", "get_checkin_status",
  "get_credit_expiry", "get_credit_statistics", "get_auto_checkin_config", "get_claim_ledger",
  "get_token_statistics", "get_qoder_statistics", "get_qoder_credit_statistics",
  "get_qoder_automation", "get_qoder_companion",
  "get_checkin_logs", "get_auto_rotate_config", "rotate_status", "get_rotate_logs",
  "get_github_config", "check_update", "get_launch_at_login_enabled", "switch_progress",
  "get_travel_status", "get_auto_travel_config", "get_rate_limits",
  "get_rate_limit_hook_status", "get_rate_limit_config",
  "get_webdav_config", "webdav_info",
]);

export function isDemoMode(): boolean {
  return demoModeEnabled;
}

export function isWebui(): boolean {
  return typeof window !== "undefined" && !("__TAURI_INTERNALS__" in window);
}

/** Tauri mobile 也注入内部 API；用现有平台 UA 约定把桌面宿主与移动宿主区分开。 */
function isMobilePlatform(): boolean {
  if (typeof navigator === "undefined") return false;
  const ua = navigator.userAgent;
  return (
    /Android|iPhone|iPad|iPod/i.test(ua) ||
    (ua.includes("Macintosh") && navigator.maxTouchPoints > 1)
  );
}

/** 是否为提供桌面专属能力的 Tauri 宿主。 */
export function isDesktop(): boolean {
  return !isWebui() && !isMobilePlatform();
}

/** Agent Companion 只由桌面宿主管理，不能经 WebUI 或演示模式访问。 */
function requireCompanionDesktop(): void {
  if (demoModeEnabled) throw new Error(DEMO_UNAVAILABLE_MESSAGE);
  if (!isDesktop()) throw new Error("Agent Companion 仅在桌面版中可用");
}

type Route = { method: "GET" | "POST"; path: string };

/** Tauri command → HTTP 路由映射（webui 模式）。 */
const ROUTES = operationRoutes as Record<string, Route>;

/**
 * 档位参数只在国际版时下发：缺省（国内版）保持改造前的请求体逐字一致，
 * Tauri 走 `invoke(cmd, undefined)`，HTTP 走无 query 的路径。
 */
function variantArgs(variant?: WbVariant): Record<string, unknown> | undefined {
  return variant === "ai" ? { variant } : undefined;
}

function queryString(args?: Record<string, unknown>): string {
  if (!args) return "";
  const params = new URLSearchParams();
  for (const [key, value] of Object.entries(args)) {
    if (value === undefined || value === null) continue;
    params.set(key, String(value));
  }
  const text = params.toString();
  return text ? `?${text}` : "";
}

/** 页面当前来源，仅用于错误提示；缺失时返回空串，调用方据此省略括号。 */
function currentOrigin(): string {
  if (typeof window === "undefined") return "";
  const origin = window.location?.origin;
  return typeof origin === "string" ? origin : "";
}

async function httpCall<T>(cmd: string, args?: Record<string, unknown>): Promise<T> {
  const route = ROUTES[cmd];
  if (!route) throw new Error(`webui 模式暂不支持该操作: ${cmd}`);
  let res: Response;
  try {
    // 相对路径以页面自身来源为基准（服务端口 / SSH 转发端口 / localhost 与
    // 127.0.0.1 混用都自动跟随），不能写死 127.0.0.1:57890。
    const url =
      route.method === "GET"
        ? `${route.path}${queryString(args)}`
        : route.path;
    res = await fetch(url, {
      method: route.method,
      headers: { "Content-Type": "application/json" },
      body: route.method === "POST" ? JSON.stringify(args ?? {}) : undefined,
    });
  } catch {
    const origin = currentOrigin();
    throw new Error(
      origin
        ? `无法连接 WorkBuddy Switch Next 服务（${origin}），请先运行 \`workbuddy-switch-next\``
        : `无法连接 WorkBuddy Switch Next 服务，请先运行 \`workbuddy-switch-next\``,
    );
  }
  const data = await res.json().catch(() => ({}));
  if (!res.ok) {
    throw new Error(data.message || data.error || `请求失败 (${res.status})`);
  }
  return data as T;
}

async function call<T>(cmd: string, args?: Record<string, unknown>): Promise<T> {
  if (demoModeEnabled) {
    if (cmd === "doubao_action") {
      if (["statistics", "get_settings"].includes(String(args?.action)) && !(args?.input as { refresh?: boolean } | undefined)?.refresh) return screenshotDemoResponse(cmd, args) as T;
      throw new Error(DEMO_UNAVAILABLE_MESSAGE);
    }
    if (cmd === "minimax_action") {
      if (["statistics", "token_stats", "get_settings"].includes(String(args?.action))) return screenshotDemoResponse(cmd, args) as T;
      throw new Error(DEMO_UNAVAILABLE_MESSAGE);
    }
    if (cmd === "trae_action") {
      if (args?.action === "statistics" || args?.action === "get_settings") return screenshotDemoResponse(cmd, args) as T;
      throw new Error(DEMO_UNAVAILABLE_MESSAGE);
    }
    if ((cmd === "get_credit_statistics" || cmd === "get_qoder_credit_statistics") && args?.refresh === true) {
      throw new Error(DEMO_UNAVAILABLE_MESSAGE);
    }
    if (!DEMO_READ_COMMANDS.has(cmd)) throw new Error(DEMO_UNAVAILABLE_MESSAGE);
    return screenshotDemoResponse(cmd, args) as T;
  }
  if (!isWebui()) return invoke<T>(cmd, args);
  return httpCall<T>(cmd, args);
}

// ---------------------------------------------------------------------------
// 状态 / 账号
// ---------------------------------------------------------------------------

/** 运行状态 / 当前账号 / 应用路径；`variant` 缺省为国内版。 */
export function getStatus(variant?: WbVariant): Promise<AppStatus> {
  return call("get_status", variantArgs(variant));
}

/** 返回全部档位的账号，由调用方按 `variant` 过滤。 */
export function getAccounts(): Promise<{ accounts: AccountMeta[] }> {
  return call("get_accounts");
}

export function getCodebuddyCliStatus(): Promise<CodeBuddyCliStatus> {
  return call("get_codebuddy_cli_status");
}

export function installCodebuddyCliHelper(): Promise<CodeBuddyCliInstallResult> {
  return call("install_codebuddy_cli_helper");
}

/**
 * 切换 CodeBuddy CLI 默认账号。
 *
 * @param closeRunningCli 已废弃：后端一律先关闭正在运行的 CLI 再写状态，该入参被忽略。
 *   仅为兼容既有调用方保留（HTTP 路径仍会原样发送）。
 */
export function switchCodebuddyCliAccount(
  accountId: string,
  closeRunningCli = false,
): Promise<CodeBuddyCliSwitchResult> {
  if (demoModeEnabled) {
    return new Promise((resolve, reject) => {
      window.setTimeout(() => {
        try {
          resolve(
            screenshotDemoResponse("switch_codebuddy_cli_account", {
              accountId,
              closeRunningCli,
            }) as CodeBuddyCliSwitchResult,
          );
        } catch (error) {
          reject(error);
        }
      }, 1200);
    });
  }
  return call("switch_codebuddy_cli_account", { accountId, closeRunningCli });
}

export function getCodebuddyCnIdeStatus(): Promise<CodeBuddyCnIdeStatus> {
  return call("get_codebuddy_cn_ide_status");
}

/**
 * 切换 CodeBuddy IDE 账号（可同时复制 / 同步会话）。
 *
 * `restart` 默认 true：IDE 运行时由后端先关闭、写入后再重新打开。
 * `copySessions` 非空时切换前把勾选会话复制到目标账号（默认沿用会话 id，冲突才重随机）；
 * `syncSelections` 与 VS Code 侧同形；两者都不传时行为与纯切换逐字一致。
 */
export function switchCodebuddyCnIdeAccount(
  accountId: string,
  restart = true,
  copySessions?: VscodeSessionRef[],
  syncSelections?: SessionSyncSelection[],
): Promise<CodeBuddyCnIdeSwitchResult> {
  return call("switch_codebuddy_cn_ide_account", { accountId, restart, copySessions, syncSelections });
}

/** 列出当前 CodeBuddy IDE 账号可复制的会话（未登录/未安装时返回空列表）。 */
export function listCodebuddyIdeSessions(): Promise<VscodeSessionList> {
  return call("list_codebuddy_ide_sessions");
}

/**
 * 预览「当前 CodeBuddy IDE 账号 → 目标账号」可同步的关联会话。
 *
 * 只读：`defaultChecked` 与 `availableModes` 是勾选权限的唯一来源，前端不得自行扩大。
 */
export function codebuddyIdeSessionLinksPreview(
  targetAccountId: string,
): Promise<SessionLinksPreview> {
  return call("codebuddy_ide_session_links_preview", { targetAccountId });
}

export function detectCodebuddyCnIdeAccount(): Promise<{
  ok: boolean;
  found: boolean;
  matched?: boolean;
  accountId?: string;
  message?: string;
}> {
  return call("detect_codebuddy_cn_ide_account");
}

export function getVscodeExtStatus(): Promise<VscodeExtStatus> {
  return call("get_vscode_ext_status");
}

/** 列出当前 VS Code 扩展账号可复制的会话（未安装/未登录时返回空列表）。 */
export function listVscodeSessions(): Promise<VscodeSessionList> {
  return call("list_vscode_sessions");
}

/**
 * 预览「当前 VS Code CodeBuddy 插件账号 → 目标账号」可同步的关联会话。
 *
 * 只读：`defaultChecked` 与 `availableModes` 是勾选权限的唯一来源，前端不得自行扩大。
 */
export function vscodeSessionLinksPreview(targetAccountId: string): Promise<SessionLinksPreview> {
  return call("vscode_session_links_preview", { targetAccountId });
}

/**
 * 切换 VS Code CodeBuddy 扩展账号。
 *
 * `restart` 默认 true：VS Code 运行时由后端先优雅退出、写入后再重新打开；
 * 传 false 退回「请先完全退出 VS Code」的手动模式（不在编辑器中自动操作）。
 * `syncSelections` 与 WorkBuddy 侧同形；只传它（不传 `copySessions`）也能执行同步。
 */
export function switchVscodeExtAccount(
  accountId: string,
  restart = true,
  copySessions?: VscodeSessionRef[],
  syncSelections?: SessionSyncSelection[],
): Promise<VscodeExtSwitchResult> {
  return call("switch_vscode_ext_account", { accountId, restart, copySessions, syncSelections });
}

export function detectVscodeExtAccount(): Promise<{
  ok: boolean;
  found: boolean;
  matched?: boolean;
  accountId?: string;
  message?: string;
}> {
  return call("detect_vscode_ext_account");
}

export function getJetbrainsStatus(): Promise<JetbrainsStatus> {
  return call("get_jetbrains_status");
}

/**
 * 切换 JetBrains IDE（全家桶）/ Android Studio 的 CodeBuddy 插件账号。
 *
 * `restart` 默认 true：IDE 运行时由后端先优雅退出、写入后再重新打开；
 * 传 false 退回「请先完全退出 IDE」的手动模式（不在 IDE 中自动操作）。
 * `configDirs` 可选：目标配置目录名列表（如 ["PyCharm2026.2"]），缺省 / 空
 * = 全部装了插件的 IDE；非空时只写所选目录、只关闭/重开这些目录的运行实例。
 */
export function switchJetbrainsAccount(
  accountId: string,
  restart = true,
  configDirs?: string[],
): Promise<JetbrainsSwitchResult> {
  return call("switch_jetbrains_account", { accountId, restart, configDirs });
}

export function detectJetbrainsAccount(): Promise<{
  ok: boolean;
  found: boolean;
  matched?: boolean;
  accountId?: string;
  message?: string;
}> {
  return call("detect_jetbrains_account");
}

export function getQoderStatus(): Promise<QoderStatus> {
  return call("get_qoder_status");
}

export interface QoderRegionSettings {
  dailyEnabled: boolean; dailyTime: string; alertsEnabled: boolean; expiryDays: number;
  lowBalance: number; rotateEnabled: boolean; rotateBelow: number; cooldownMinutes: number;
}
export type QoderSettings = Record<"cn" | "ai", QoderRegionSettings>;
export interface QoderAutomationStatus {
  settings: QoderSettings;
  logs: { at: number; region: "cn" | "ai"; action: string; outcome: string }[];
}
export function getQoderAutomation(): Promise<QoderAutomationStatus> { return call("get_qoder_automation"); }
export function saveQoderAutomation(settings: QoderSettings): Promise<QoderSettings> { return call("save_qoder_automation", { settings }); }
export function runQoderAutomation(): Promise<{ ok?: boolean; skipped?: boolean }> { return call("run_qoder_automation"); }

export interface QoderCompanionStatus { installed: boolean; template: { id: string } & Record<string, unknown> }
export function getQoderCompanion(global: boolean): Promise<QoderCompanionStatus> { return call("get_qoder_companion", { global }); }
export async function setQoderCompanion(global: boolean, enabled: boolean): Promise<QoderCompanionStatus> {
  if (demoModeEnabled) throw new Error(DEMO_UNAVAILABLE_MESSAGE);
  if (!enabled) {
    const status = await call<QoderCompanionStatus>("set_qoder_companion", { global, enabled });
    if (isDesktop()) {
      // Removing hooks must still work when the collector is offline.
      await invoke("plugin:agent-studio|collector_request", { command: "custom_integrations_set", payload: { action: "disable", id: status.template.id } }).catch(() => {});
    }
    return status;
  }
  if (!isDesktop()) throw new Error("请在桌面版接入 Qoder 会话悬浮窗");
  await setCompanionEnabled(true);
  const status = await getQoderCompanion(global);
  const request = <T>(command: string, payload: Record<string, unknown> = {}) => invoke<T>("plugin:agent-studio|collector_request", { command, payload });
  let registry: { templates: { id: string; enabled: boolean }[] } | undefined;
  for (let attempt = 0; attempt < 10; attempt++) {
    try { registry = await request("custom_integrations_get"); break; }
    catch { await new Promise(resolve => setTimeout(resolve, 300)); }
  }
  if (!registry) throw new Error("悬浮窗服务尚未启动，请稍后重试");
  const existing = registry.templates.find(t => t.id === status.template.id);
  if (!existing) await request("custom_integrations_set", { action: "import", template: status.template });
  else if (!existing.enabled) await request("custom_integrations_set", { action: "enable", id: status.template.id });
  return call("set_qoder_companion", { global, enabled });
}
export interface QoderCloudResult {
  ok?: boolean; count?: number; imported?: number; skipped?: number; warnings?: string[];
  snapshotId?: string; updatedAt?: number;
  accounts?: { index: number; label: string; global: boolean; targets: QoderTarget[] }[];
}
export function qoderCloud(action: "backup" | "sync" | "preview" | "restore", passphrase: string, indexes?: number[], snapshotId?: string): Promise<QoderCloudResult> {
  return call("qoder_cloud", { action, passphrase, indexes, snapshotId });
}

export function exportAllAccounts(path?: string): Promise<unknown> {
  return call("export_all_accounts", { path: path ?? null });
}
export function previewAllAccounts(fileText: string): Promise<{workbuddy: number; qoder: number}> {
  return call("preview_all_accounts", { fileText });
}
export function importAllAccounts(fileText: string): Promise<{workbuddy: {imported: number; skipped: number}; qoder: {imported: number; skipped: number; warnings: string[]}}> {
  return call("import_all_accounts", { fileText });
}

export function getQoderCredits(bundleIds: string[], liveTarget?: QoderTarget): Promise<QoderCredit> {
  return call("get_qoder_credits", { bundleIds, liveTarget: liveTarget ?? null });
}

export function getQoderStatistics(): Promise<import("./types").QoderStatistics> {
  return call("get_qoder_statistics");
}

export function getQoderCreditStatistics(refresh = false): Promise<import("./types").QoderCreditStatistics> {
  return call("get_qoder_credit_statistics", { refresh });
}

export function claimQoderRewards(bundleIds: string[], liveTarget?: QoderTarget): Promise<{ok: boolean; message: string}> {
  return call("claim_qoder_rewards", { bundleIds, liveTarget: liveTarget ?? null });
}

export function startQoderLogin(target: QoderTarget): Promise<{loginId: string; verificationUri: string; expiresIn: number}> {
  return call("qoder_oauth_start", { target });
}

export function pollQoderLogin(loginId: string): Promise<{done: boolean; result?: { label: string; clients: QoderTarget[]; warnings: string[] }; error?: string}> {
  return call("qoder_oauth_poll", { loginId });
}

export function cancelQoderLogin(loginId: string): Promise<{ok: boolean}> {
  return call("qoder_oauth_cancel", { loginId });
}

/** 把 Qoder 现场登录态认领为账号包；相同凭据再次认领会刷新原包。 */
export function captureQoderAccount(target: QoderTarget, label?: string): Promise<QoderBundle> {
  return call("capture_qoder_account", { target, label });
}

export function autoCaptureQoderAccounts(): Promise<{added: number; failedTargets: QoderTarget[]; authorizationRequired: QoderTarget[]; deferred?: boolean}> {
  return call("auto_capture_qoder_accounts");
}

export interface QoderBackupPreview { index: number; label: string; global: boolean; targets: QoderTarget[] }
export function exportQoderAccounts(bundleIds: string[]): Promise<unknown> {
  return call("export_qoder_accounts", { bundleIds });
}
export function exportQoderAccountsToPath(bundleIds: string[], path: string): Promise<{path: string}> {
  return call("export_qoder_accounts_to_path", { bundleIds, path });
}
export function previewQoderBackup(fileText: string): Promise<{accounts: QoderBackupPreview[]}> {
  return call("preview_qoder_backup", { fileText });
}
export function importQoderBackup(fileText: string, indexes: number[]): Promise<{imported: number; skipped: number; warnings: string[]}> {
  return call("import_qoder_backup", { fileText, indexes });
}

export function renameQoderAccount(bundleId: string, label: string): Promise<QoderBundle> {
  return call("rename_qoder_account", { bundleId, label });
}

export function deleteQoderAccount(bundleId: string): Promise<{ ok: boolean }> {
  return call("delete_qoder_account", { bundleId });
}

/**
 * 切换 Qoder 账号包。`closeRunning` 默认 true：目标运行中时由后端关闭；
 * `relaunch` 默认 true：桌面端切换前在运行则写完后重新打开（CLI 不重开）。
 */
export function switchQoderAccount(
  bundleId: string,
  closeRunning = true,
  relaunch = true,
): Promise<QoderSwitchResult> {
  return call("switch_qoder_account", { bundleId, closeRunning, relaunch });
}

export function getCodebuddyIdeStatus(): Promise<CodeBuddyCnIdeStatus> {
  return call("get_codebuddy_ide_status");
}

/**
 * 切换 CodeBuddy IDE（国际版）账号（可同时复制 / 同步会话）。
 *
 * `restart` 默认 true：IDE 运行时由后端先关闭、写入后再重新打开。
 * `copySessions` 非空时切换前把勾选会话复制到目标账号（默认沿用会话 id，冲突才重随机）；
 * `syncSelections` 与国内版同形；两者都不传时行为与纯切换逐字一致。
 */
export function switchCodebuddyIdeAccount(
  accountId: string,
  restart = true,
  copySessions?: VscodeSessionRef[],
  syncSelections?: SessionSyncSelection[],
): Promise<CodeBuddyCnIdeSwitchResult> {
  return call("switch_codebuddy_ide_account", { accountId, restart, copySessions, syncSelections });
}

/** 列出当前国际版 CodeBuddy IDE 账号可复制的会话（未登录/未安装时返回空列表）。 */
export function listCodebuddyIntlIdeSessions(): Promise<VscodeSessionList> {
  return call("list_codebuddy_intl_ide_sessions");
}

/**
 * 预览「当前国际版 CodeBuddy IDE 账号 → 目标账号」可同步的关联会话。
 *
 * 只读：`defaultChecked` 与 `availableModes` 是勾选权限的唯一来源，前端不得自行扩大。
 */
export function codebuddyIntlIdeSessionLinksPreview(
  targetAccountId: string,
): Promise<SessionLinksPreview> {
  return call("codebuddy_intl_ide_session_links_preview", { targetAccountId });
}

export function detectCodebuddyIdeAccount(): Promise<{
  ok: boolean;
  found: boolean;
  matched?: boolean;
  accountId?: string;
  message?: string;
}> {
  return call("detect_codebuddy_ide_account");
}


export function deleteAccount(accountId: string): Promise<{ ok: boolean }> {
  return call("delete_account", { accountId });
}

/**
 * 更新账号本地展示字段（备注 / 显示选择）。
 * patch 只传需要改的项：`note`（字符串或 null 清空）、`displayField`。
 */
export function updateAccountDisplay(
  accountId: string,
  patch: { note?: string | null; displayField?: DisplayField },
): Promise<{ ok: boolean; account: AccountMeta }> {
  if (demoModeEnabled) {
    return Promise.resolve(
      screenshotDemoResponse("update_account_display", { accountId, patch }) as {
        ok: boolean;
        account: AccountMeta;
      },
    );
  }
  return call("update_account_display", { accountId, patch });
}

/** 发起登录：国内版为扫码授权，国际版为浏览器 Web 登录授权；`variant` 缺省为国内版（档位由后端记忆，轮询无需再传）。 */
export function oauthStart(variant?: WbVariant): Promise<OAuthStartResult> {
  return call("oauth_start", variantArgs(variant));
}

export function oauthStatus(loginId: string): Promise<OAuthPollResult> {
  return call("oauth_status", { loginId });
}

/** 导入本机当前登录态；`variant` 缺省为国内版（对应各自的登录态文件）。 */
export function importLocal(variant?: WbVariant): Promise<{ ok: boolean; account: AccountMeta }> {
  return call("import_local", variantArgs(variant));
}

export function exportAccounts(accountIds: string[]): Promise<{ ok: boolean; accounts: AccountRecord[] }> {
  return call("export_accounts", { accountIds });
}

/** 桌面端：把完整记录写入用户选择的路径（系统保存对话框产物）。 */
export function exportAccountsToPath(
  accountIds: string[],
  path: string,
): Promise<{ ok: boolean; path: string }> {
  return call("export_accounts_to_path", { accountIds, path });
}

export function previewImportAccounts(
  fileText: string,
): Promise<{ accounts: ImportPreviewAccount[]; total: number }> {
  return call("preview_import_accounts", { fileText });
}

export function importAccounts(fileText: string, indexes: number[]): Promise<ImportResult> {
  return call("import_accounts", { fileText, indexes });
}

export function switchAccount(args: {
  accountId: string;
  restart?: boolean;
  shareSessions?: boolean;
  copySessionIds?: string[];
  syncSelections?: SessionSyncSelection[];
}): Promise<SwitchResult> {
  return call("switch_account", args as unknown as Record<string, unknown>);
}

/** 切换进度（webui 轮询用；桌面端走事件，此函数无副作用）。 */
export function switchProgress(): Promise<{ running: boolean; progress: string | null }> {
  return call("switch_progress");
}

/** 当前登录态的会话列表；`variant` 缺省为国内版。 */
export function listSessions(variant?: WbVariant): Promise<{
  sessions: Session[];
  current: string | null;
}> {
  return call("list_sessions", variantArgs(variant));
}

/** 指定账号名下的会话列表（会话管理页的源账号视角）；账号不存在时后端返回明确错误。 */
export function listAccountSessions(accountId: string, client?: SessionGroupClient): Promise<{
  /** 插件侧会话没有 `cwd`（只有 `workspaceHash`）：调用方补齐默认值后再交给会话树。 */
  sessions: (Omit<Session, "cwd"> & { cwd?: string })[];
  current: string | null;
  variant?: WbVariant;
  /** 插件侧：插件数据仓根目录（`null` = 未找到目录，与「该账号无会话」区分）。 */
  dataRoot?: string | null;
  sourceUid?: string;
}> {
  return call("list_account_sessions", { accountId, ...(client ? { client } : {}) });
}

/** 把勾选会话复制到指定账号；返回 core 同形的复制报告（copied / alreadyLinked / errors）。 */
export function copySessions(
  targetAccountId: string,
  sessionIds: string[],
): Promise<SessionCopyReport & { variant?: WbVariant }> {
  return call("copy_sessions", { targetAccountId, sessionIds });
}

/**
 * 跨档把会话从**显式源账号**复制到**显式目标账号**（会话管理页用）。
 *
 * 与 `copySessions` 同形，只多一个 `sourceAccountId`：源可为国内版或国际版账号，
 * 不再要求源是当前登录账号；报告另带 `sourceVariant` / `targetVariant`。
 */
export function copySessionsCross(
  sourceAccountId: string,
  targetAccountId: string,
  sessionIds: string[],
): Promise<SessionCopyReport & { variant?: WbVariant }> {
  return call("copy_sessions_cross", { sourceAccountId, targetAccountId, sessionIds });
}

/**
 * 预览「当前账号 → 目标账号」可同步的关联会话（只读）。
 *
 * 默认勾选与可选模式都来自后端：前端只按 `defaultChecked` / `availableModes` 渲染，
 * 不自行扩大权限。`variant` 缺省由后端取目标账号自身档位。
 */
export function sessionLinksPreview(
  targetAccountId: string,
  variant?: WbVariant,
): Promise<SessionLinksPreview> {
  const args: Record<string, unknown> = { targetAccountId };
  if (variant === "ai") args.variant = variant;
  return call("session_links_preview", args);
}

/**
 * 预览「显式来源账号 → 显式目标账号」可同步的关联会话（只读；跨档支持）。
 *
 * 与 `sessionLinksPreview` 同形，多一个 `sourceAccountId`；成员内容按成员自身档位读取
 * （跨档组的源读源档、目标读目标档）。报告在跨档时带 `sourceVariant` / `targetVariant`。
 */
export function sessionLinksPreviewCross(
  sourceAccountId: string,
  targetAccountId: string,
): Promise<SessionLinksPreview> {
  return call("session_links_preview_cross", { sourceAccountId, targetAccountId });
}

/**
 * 把显式来源账号的新增同步到显式目标账号（跨档支持；会话管理页用）。
 *
 * `syncSelections` 与切号弹窗同形（含预览凭据，执行时后端逐项复核）；
 * 目标档客户端运行时会返回明确错误（不写半成品）。
 */
export function sessionSyncCross(
  sourceAccountId: string,
  targetAccountId: string,
  syncSelections: SessionSyncSelection[],
): Promise<SessionSyncReport> {
  return call("session_sync_cross", { sourceAccountId, targetAccountId, syncSelections });
}

export function listSessionGroups(client: SessionGroupClient, variantScope?: WbVariant): Promise<SessionGroupList> {
  return call("list_session_groups", { client, ...(variantScope ? { variantScope } : {}) });
}

export function getSessionGroup(client: SessionGroupClient, groupId: string, variantScope?: WbVariant): Promise<SessionGroupDetail> {
  return call("get_session_group", { client, groupId, ...(variantScope ? { variantScope } : {}) });
}

export function previewSessionGroupPair(args: {
  client: SessionGroupClient;
  groupId: string;
  sourceMemberId: string;
  targetMemberId: string;
  variantScope?: WbVariant;
}): Promise<SessionGroupPairPreview> {
  return call("preview_session_group_pair", args as unknown as Record<string, unknown>);
}

export function syncSessionGroupPair(args: {
  client: SessionGroupClient;
  groupId: string;
  sourceMemberId: string;
  targetMemberId: string;
  previewToken: string;
  mode: SessionSyncMode;
  variantScope?: WbVariant;
  /** 已获用户授权（确认框）时传 `true`：插件侧运行中允许关闭并重开 VS Code。 */
  restart?: boolean;
}): Promise<SessionGroupActionReport> {
  if (demoModeEnabled) return Promise.reject(new Error(DEMO_UNAVAILABLE_MESSAGE));
  return call("sync_session_group_pair", args as unknown as Record<string, unknown>);
}

export function syncSessionGroupUnify(args: {
  client: "workbuddy" | "vscodeExt";
  groupId: string;
  sourceMemberId: string;
  targets: { targetMemberId: string; previewToken: string; mode: SessionSyncMode }[];
  /** 已获用户授权（确认框）时传 `true`：插件侧运行中允许关闭并重开 VS Code。 */
  restart?: boolean;
}): Promise<SessionGroupActionReport> {
  if (demoModeEnabled) return Promise.reject(new Error(DEMO_UNAVAILABLE_MESSAGE));
  return call("sync_session_group_unify", args as unknown as Record<string, unknown>);
}

export function syncSessionGroupSafeBatch(client: SessionGroupClient, groupId: string, variantScope?: WbVariant, restart?: boolean): Promise<SessionGroupActionReport> {
  if (demoModeEnabled) return Promise.reject(new Error(DEMO_UNAVAILABLE_MESSAGE));
  return call("sync_session_group_safe_batch", { client, groupId, ...(variantScope ? { variantScope } : {}), ...(restart ? { restart: true } : {}) });
}

export function addSessionGroupMember(args: {
  client: SessionGroupClient;
  groupId: string;
  sourceMemberId: string;
  targetAccountId: string;
  variantScope?: WbVariant;
  /** 已获用户授权（确认框）时传 `true`：插件侧运行中允许关闭并重开 VS Code。 */
  restart?: boolean;
}): Promise<{ status: "linked" | "alreadyLinked" | "copiedUnlinked" | "failed"; editorError?: string; restartedEditor?: boolean; /** 内容缺失 / 索引丢失而被跳过的会话。 */ skipped?: { id: string; error: string }[]; [key: string]: unknown }> {
  if (demoModeEnabled) return Promise.reject(new Error(DEMO_UNAVAILABLE_MESSAGE));
  return call("add_session_group_member", args as unknown as Record<string, unknown>);
}

/** 插件：把来源账号的勾选会话复制到目标账号并登记关联（无现成组则新建关联组）。 */
export function copyLinkedSessions(args: {
  client: SessionGroupClient;
  sourceAccountId: string;
  targetAccountId: string;
  sessionIds: string[];
  /** 已获用户授权（确认框）时传 `true`：运行中允许关闭并重开 VS Code。 */
  restart?: boolean;
}): Promise<{ status: "linked" | "alreadyLinked" | "copiedUnlinked" | "failed"; editorError?: string; restartedEditor?: boolean; [key: string]: unknown }> {
  if (demoModeEnabled) return Promise.reject(new Error(DEMO_UNAVAILABLE_MESSAGE));
  return call("copy_linked_sessions", args as unknown as Record<string, unknown>);
}

/** 预检：当前是否需要关闭 VS Code（运行中 且 目标账号含当前登录账号）；供前端决定是否先弹确认框。 */
export function vscodeRestartPrecheck(targetAccountIds: string[]): Promise<{ required: boolean; running: boolean }> {
  return call("vscode_restart_precheck", { targetAccountIds });
}

/**
 * 取消关联：把成员从会话组移除（只解除管理关系，不删除账号内的会话内容）。
 *
 * `groupRemoved` 表示移除后组内已无成员，该组已被删除。
 */
export function unlinkSessionGroupMember(args: {
  client: SessionGroupClient;
  groupId: string;
  memberId: string;
  variantScope?: WbVariant;
}): Promise<{ status: "removed" | "groupRemoved"; client: SessionGroupClient; groupId: string; memberId: string; remaining: number }> {
  if (demoModeEnabled) return Promise.reject(new Error(DEMO_UNAVAILABLE_MESSAGE));
  return call("unlink_session_group_member", args as unknown as Record<string, unknown>);
}

/**
 * 删除会话组：组内所有成员一起解除关联（只解除管理关系，不删除账号内的会话内容）。
 */
export function deleteSessionGroup(args: {
  client: SessionGroupClient;
  groupId: string;
  variantScope?: WbVariant;
}): Promise<{ status: "groupRemoved"; client: SessionGroupClient; groupId: string; removed: number }> {
  if (demoModeEnabled) return Promise.reject(new Error(DEMO_UNAVAILABLE_MESSAGE));
  return call("delete_session_group", args as unknown as Record<string, unknown>);
}

/** 打开系统设置授权面板（桌面端专用；webui 模式由服务进程权限决定，无操作）。 */
export function openPermissionSettings(
  target?: "app_management" | "all_files",
): Promise<void> {
  if (demoModeEnabled) return Promise.reject(new Error(DEMO_UNAVAILABLE_MESSAGE));
  if (isWebui()) return Promise.resolve();
  return call("open_permission_settings", { target: target ?? "app_management" });
}

/** 权限自检：桌面端写探针（按档位写在对应登录态文件旁）；webui 模式由服务进程权限决定。 */
export function checkAuthPermission(variant?: WbVariant): Promise<{
  ok: boolean;
  message?: string;
  error?: string;
  dir?: string;
  hint?: string;
}> {
  if (demoModeEnabled) return Promise.reject(new Error(DEMO_UNAVAILABLE_MESSAGE));
  if (isWebui()) {
    return Promise.resolve({
      ok: true,
      message: "webui 模式由服务进程（终端启动）的权限决定，无需额外授权",
      hint: "",
    });
  }
  return call("check_auth_permission", variantArgs(variant));
}

/** 在 Finder 中显示当前 App（桌面端专用；webui 无操作）。 */
export function revealAppInFinder(): Promise<void> {
  if (demoModeEnabled) return Promise.reject(new Error(DEMO_UNAVAILABLE_MESSAGE));
  if (isWebui()) return Promise.resolve();
  return call("reveal_app_in_finder");
}

/** 后端持久化的悬浮栏启用状态；默认值由后端决定。 */
export function getCompanionEnabled(): Promise<boolean> {
  try {
    requireCompanionDesktop();
    return call("get_companion_enabled");
  } catch (error) {
    return Promise.reject(error);
  }
}

/** 返回后端确认的最终状态，不在前端单独持久化。 */
export function setCompanionEnabled(enabled: boolean): Promise<boolean> {
  try {
    requireCompanionDesktop();
    return call("set_companion_enabled", { enabled });
  } catch (error) {
    return Promise.reject(error);
  }
}

export function openCompanionSettings(): Promise<void> {
  try {
    requireCompanionDesktop();
    return call("open_companion_settings");
  } catch (error) {
    return Promise.reject(error);
  }
}

// ---------------------------------------------------------------------------
// 阶段 3：签到 + token 刷新
// ---------------------------------------------------------------------------

export async function getCheckinStatus(accountId: string): Promise<{
  ok: boolean;
  todayCheckedIn?: boolean;
  result?: string;
  reason?: string;
  error?: string;
  raw?: unknown;
  /** 该行所属档位（档位取账号自身）；缺省按国内版处理。 */
  variant?: WbVariant;
}> {
  // 两个宿主都只查询目标账号；Web 端不再为每个账号重复请求整份列表。
  return call("get_checkin_status", { accountId });
}

export function getCreditExpiry(accountId: string): Promise<CreditExpiry> {
  return call("get_credit_expiry", { accountId });
}

export function getCreditStatistics(refresh = false): Promise<CreditStatistics> {
  return call("get_credit_statistics", refresh ? { refresh: true } : undefined);
}

export function getTokenStatistics(days?: number): Promise<TokenStatistics> { return call("get_token_statistics", days ? { days } : undefined); }

/**
 * 模型限额台账：一次返回**全部账号**当前受限的模型与官方恢复时刻。
 *
 * 不传档位：扫描本身就是全局的（两档位各扫一遍）。后端把 hook 信号与日志扫描
 * 合并后返回，日志扫描按 5 分钟节流（`scannedAt` 是最近一次真实扫描时刻）。
 */
export function getRateLimits(): Promise<RateLimitsPayload> {
  return call("get_rate_limits");
}

/** 限额 hook 安装状态（脚本 + 三处客户端配置）。 */
export function getRateLimitHookStatus(): Promise<RateLimitHookStatus> {
  return call("get_rate_limit_hook_status");
}

/** 安装限额 hook（幂等、写前备份；返回安装后的状态）。 */
export function installRateLimitHook(): Promise<RateLimitHookStatus> {
  return call("install_rate_limit_hook");
}

/** 卸载限额 hook（移除注册条目并尽量逐字节还原配置）。 */
export function uninstallRateLimitHook(): Promise<RateLimitHookStatus> {
  return call("uninstall_rate_limit_hook");
}

/** 限额监听开关（关闭后不扫日志、不渲染限额 chip）。 */
export function getRateLimitConfig(): Promise<RateLimitConfig> {
  return call("get_rate_limit_config");
}

export function saveRateLimitConfig(config: RateLimitConfig): Promise<RateLimitConfig> {
  return call("save_rate_limit_config", {
    config: config as unknown as Record<string, unknown>,
  });
}

export function checkin(accountId: string): Promise<CheckinResult> {
  return call("checkin", { accountId });
}

/**
 * 批量签到：不传档位时覆盖全部档位；显式传入时只处理该档位。
 * 关闭自动签到的账号会被跳过，并逐账号返回 skipped 原因（设置页与托盘同样遵守）。
 * `respectWindow=true` 时遵守签到时间段：窗口外整轮返回 skipped /
 * `outside_checkin_window`，不发起任何签到请求；
 * 缺省不下发该字段，保持设置页 / 托盘 / 旧客户端的立即签到语义。
 *
 * 这里**不能**用 `variantArgs`：`checkin_all` 的缺省语义是「全部档位」，国内版若
 * 缺省不传参，账号页在国内版 Tab 触发的批量签到会打到国际版账号。显式下发 `cn`
 * 与改造前等价（改造前账号库里只有国内版账号）。
 */
export function checkinAll(variant?: WbVariant, respectWindow?: boolean): Promise<{
  accounts: { accountId: string; email: string; result: string; error?: string; inactive?: boolean; reason?: string; rewards?: {ok: boolean; message: string; remaining?: string[]; warnings?: string[]; total?: number; claimed?: number; claimable?: number; blocked?: number} }[];
  status?: string;
  reason?: string;
}> {
  const args: Record<string, unknown> = variant ? { variant } : {};
  if (respectWindow === true) args.respectWindow = true;
  return call("checkin_all", args);
}

export function runDailyTasks(variant: WbVariant): ReturnType<typeof checkinAll> {
  return call("run_daily_tasks", { variant });
}

export function getAutoCheckinConfig(): Promise<CheckinConfig> {
  return call("get_auto_checkin_config");
}

export function saveAutoCheckinConfig(config: CheckinConfig): Promise<CheckinConfig> {
  return call("save_auto_checkin_config", {
    config: config as unknown as Record<string, unknown>,
  });
}

export function getCheckinLogs(): Promise<{ logs: CheckinLog[] }> {
  return call("get_checkin_logs");
}

/**
 * 今日领取记忆（claim ledger）。服务端按 Asia/Shanghai 切日，只返回当天条目；
 * 前端按 platform + variant + accountId 派生按钮态与卡片 chip。
 *
 * `state` 语义：claimed=已领完（没有可领项）、partial=还剩可领项、
 * blocked=今日无可领取（未开放/企业版/受限）、error=失败可重试、
 * unconfirmed=结果未知（不要重复提交）、skipped=本次跳过。
 */
export type ClaimState = "claimed" | "partial" | "blocked" | "error" | "unconfirmed" | "skipped";
export type ClaimScope = "checkin" | "daily" | "rewards";
export interface ClaimCounts {
  total: number;
  claimed: number;
  claimable: number;
  blocked: number;
}
export interface ClaimLedgerEntry {
  day: string;
  platform: string;
  variant: string | null;
  accountId: string;
  scope: ClaimScope;
  state: ClaimState;
  counts: ClaimCounts;
  tasks: { label: string; state: ClaimState }[];
  message: string;
  updatedAt: number;
  source: string;
  attemptCount: number;
}
export interface ClaimLedger {
  day: string;
  timezone: string;
  entries: ClaimLedgerEntry[];
}

export function getClaimLedger(): Promise<ClaimLedger> {
  return call("get_claim_ledger");
}

/** 清除今日领取记忆；不传参数时清空全部平台。 */
export function clearClaimLedger(platform?: string, scope?: ClaimScope): Promise<{ removed: number }> {
  return call("clear_claim_ledger", { platform, scope });
}

export async function getTravelStatus(accountId: string): Promise<TravelStatus> {
  if (demoModeEnabled) {
    return screenshotDemoResponse("get_travel_status", { accountId }) as TravelStatus;
  }
  if (isWebui()) {
    // webui 端为批量接口，按 accountId 过滤
    const all = await httpCall<{
      accounts: { accountId: string; email: string; label: TravelStatus["label"]; rewardCredit: number | null; locationName?: string | null; arriveAt?: number | null }[];
    }>("get_travel_status");
    const one = all.accounts.find((a) => a.accountId === accountId);
    return one
      ? { label: one.label, rewardCredit: one.rewardCredit, locationName: one.locationName ?? null, arriveAt: one.arriveAt ?? null }
      : { label: "untraveled", rewardCredit: null, locationName: null, arriveAt: null };
  }
  return call("get_travel_status", { accountId });
}

export function getAutoTravelConfig(): Promise<TravelConfig> {
  return call("get_auto_travel_config");
}

export function saveAutoTravelConfig(config: TravelConfig): Promise<TravelConfig> {
  return call("save_auto_travel_config", {
    config: config as unknown as Record<string, unknown>,
  });
}

export function getAutoRotateConfig(): Promise<AutoRotateConfig> {
  return call("get_auto_rotate_config");
}

export function saveAutoRotateConfig(config: AutoRotateConfig): Promise<AutoRotateConfig> {
  return call("save_auto_rotate_config", {
    config: config as unknown as Record<string, unknown>,
  });
}

export function getRotateStatus(): Promise<RotateStatus> {
  return call("rotate_status");
}

/**
 * 手动触发一次轮换检查。
 *
 * `notify`（可选）：因存活门控被推迟、但除门控外本来会切换时由 core 组装好的提示内容。
 * 桌面端由宿主（Rust）直接投递系统通知，这里只保留字段以描述完整返回契约；
 * 无头 server 不投递，调用方按需自行处理。
 */
export function runRotate(): Promise<{
  status: string;
  reason?: string;
  error?: string;
  to?: string;
  notify?: { title: string; body: string };
}> {
  return call("run_rotate");
}

export function getRotateLogs(): Promise<{ logs: RotateLog[] }> {
  return call("get_rotate_logs");
}

export function refreshAccountToken(accountId: string): Promise<AccountMeta> {
  return call("refresh_account_token", { accountId });
}

// ---------------------------------------------------------------------------
// 阶段 4：自动更新
// ---------------------------------------------------------------------------

export function getGithubConfig(): Promise<GithubConfig> {
  return call("get_github_config");
}

export function saveGithubConfig(config: GithubConfig): Promise<GithubConfig> {
  return call("save_github_config", {
    config: config as unknown as Record<string, unknown>,
  });
}

function recordUpdateDiagnostic(event: "clicked" | "resolved" | "rejected" | "timeout", elapsedMs: number) {
  if (!RC_DIAGNOSTICS_ENABLED || demoModeEnabled || isWebui()) return;
  void invoke("record_update_diagnostic", { event, elapsedMs: Math.round(elapsedMs) }).catch(() => {});
}

export async function checkUpdate(proxy?: string, force?: boolean): Promise<UpdateInfo> {
  const started = performance.now();
  recordUpdateDiagnostic("clicked", 0);
  try {
    const result = await waitForUpdateCheck(call<UpdateInfo>("check_update", { proxy: proxy?.trim() || null, force: force ?? false }));
    recordUpdateDiagnostic("resolved", performance.now() - started);
    return result;
  } catch (error) {
    recordUpdateDiagnostic(error instanceof UpdateCheckTimeoutError ? "timeout" : "rejected", performance.now() - started);
    throw error;
  }
}

export function revealRcDiagnostics(): Promise<void> {
  return call("reveal_rc_diagnostics");
}

// ---------------------------------------------------------------------------
// WebDAV 备份 / 跨设备同步
// ---------------------------------------------------------------------------

export function getWebdavConfig(): Promise<WebdavConfig> {
  return call("get_webdav_config");
}

/** 保存 WebDAV 配置；`password` 留空表示沿用已存口令（脱敏视图不回显明文）。 */
export function saveWebdavConfig(config: WebdavConfigInput): Promise<WebdavConfig> {
  return call("save_webdav_config", {
    config: config as unknown as Record<string, unknown>,
  });
}

export function webdavTestConnection(): Promise<{ ok: boolean; dirUrl: string }> {
  return call("webdav_test_connection");
}

export function webdavBackup(passphrase: string): Promise<WebdavActionResult> {
  return call("webdav_backup", { passphrase });
}

export function webdavRestorePreview(
  passphrase: string,
): Promise<WebdavRestorePreview> {
  return call("webdav_restore_preview", { passphrase });
}

export function webdavRestore(
  passphrase: string,
  indexes: number[],
  restoreConfigs: boolean,
  expectedSnapshot: string,
): Promise<WebdavRestoreResult> {
  return call("webdav_restore", { passphrase, indexes, restoreConfigs, expectedSnapshot });
}

export function webdavSyncNow(passphrase: string): Promise<WebdavActionResult> {
  return call("webdav_sync_now", { passphrase });
}

export function webdavInfo(): Promise<WebdavRemoteInfo> {
  return call("webdav_info");
}

export function relaunchApp(): Promise<void> {
  return call("relaunch_app");
}

// ---------------------------------------------------------------------------
// 统一更新服务（桌面端；`update-state` 事件是阶段与进度的唯一来源）
// ---------------------------------------------------------------------------

/** 浏览器 / 演示模式没有更新服务：与弹窗既有文案逐字一致。 */
const UPDATE_UNSUPPORTED_MESSAGE = "浏览器 webui 模式不能直接安装桌面更新包";

/**
 * 更新状态快照（前端首屏初始化；之后由 `update-state` 事件推送）。
 *
 * webui 没有更新服务、演示模式禁止真实下载，两者都回落到静态快照：
 * 演示模式给「有新版」态，保证演示页 / 截图里的升级入口与外链完整。
 */
export function updateState(): Promise<UpdateSnapshot> {
  if (demoModeEnabled) {
    // 复用只读演示数据的版本号，避免版本号在两处硬编码。
    const demo = screenshotDemoResponse("check_update") as UpdateInfo;
    return Promise.resolve({
      phase: "available",
      latest: demo.latest ?? null,
      percent: null,
      message: null,
      checkedAt: null,
    });
  }
  if (isWebui()) {
    return Promise.resolve({
      phase: "idle",
      latest: null,
      percent: null,
      message: null,
      checkedAt: null,
    });
  }
  return call("update_state");
}

/** 启动更新包下载（异步，立即返回；进度走 `update-state` 事件与托盘）。 */
export function updateDownload(): Promise<void> {
  if (demoModeEnabled) return Promise.reject(new Error(DEMO_UNAVAILABLE_MESSAGE));
  if (isWebui()) return Promise.reject(new Error(UPDATE_UNSUPPORTED_MESSAGE));
  return call<unknown>("update_download").then(() => undefined);
}

/** 安装已下载的更新包并重启（用户点「重启以完成升级」时调用）。 */
export function updateRestart(): Promise<void> {
  if (demoModeEnabled) return Promise.reject(new Error(DEMO_UNAVAILABLE_MESSAGE));
  if (isWebui()) return Promise.reject(new Error(UPDATE_UNSUPPORTED_MESSAGE));
  return call<unknown>("update_restart").then(() => undefined);
}

// ---------------------------------------------------------------------------
// 开机自启（仅桌面端；webui 不提供同名接口，卡片也不在 webui 渲染）
// ---------------------------------------------------------------------------

/** 查询系统当前的开机自启注册状态（桌面端）。 */
export function getLaunchAtLoginEnabled(): Promise<boolean> {
  if (demoModeEnabled) return call("get_launch_at_login_enabled");
  if (!isDesktop()) return Promise.resolve(false);
  return call("get_launch_at_login_enabled");
}

/** 注册 / 移除系统开机自启，返回回读后的权威状态（桌面端）。 */
export function setLaunchAtLoginEnabled(enabled: boolean): Promise<boolean> {
  if (demoModeEnabled) return Promise.reject(new Error(DEMO_UNAVAILABLE_MESSAGE));
  if (!isDesktop()) return Promise.resolve(false);
  return call("set_launch_at_login_enabled", { enabled });
}

/** 把 Tauri command / HTTP 抛出的错误统一为 Error。 */
export function asError(e: unknown): string {
  if (typeof e === "string") return e;
  if (e instanceof Error) return e.message;
  return JSON.stringify(e ?? "未知错误");
}

// ---------------------------------------------------------------------------
// 通知存档（toast 事后可查）
// ---------------------------------------------------------------------------

/** 记录一条应用内提示（由 `lib/notify.ts` 统一调用；失败不影响提示本身）。 */
export function recordNotification(
  level: AppNotification["level"],
  title: string,
  description?: string,
): Promise<{ recorded: boolean }> {
  if (demoModeEnabled) return Promise.resolve({ recorded: false });
  return call("record_notification", { level, title, description });
}

/** 读取最近的通知（新的在前，最多 100 条）。 */
export function listNotifications(): Promise<{ items: AppNotification[] }> {
  if (demoModeEnabled) return Promise.resolve({ items: [] });
  return call("list_notifications");
}

/** 清空通知存档。 */
export function clearNotifications(): Promise<{ cleared: boolean }> {
  if (demoModeEnabled) return Promise.resolve({ cleared: false });
  return call("clear_notifications");
}

// ---------------------------------------------------------------------------
// 错误日志（前端崩溃 / 未捕获错误落盘，见 lib/error-report.ts）
// ---------------------------------------------------------------------------

/**
 * 上报一条错误到本地错误日志（桌面端落盘 `~/.wb-switch/error.log`）。
 *
 * webui / 演示模式没有落盘通道：静默忽略（调用方的本地提示不受影响）。
 */
export function logError(kind: ErrorLogKind, message: string, detail?: string): Promise<void> {
  if (demoModeEnabled || isWebui()) return Promise.resolve();
  return call<unknown>("log_error", { kind, message, detail: detail ?? null }).then(
    () => undefined,
  );
}

/** 错误日志文件路径（设置页展示）。 */
export function getErrorLogPath(): Promise<string> {
  // 演示模式给一条与其它演示路径同风格的值，保证演示页 / 截图里界面完整。
  if (demoModeEnabled) return Promise.resolve("/demo/.wb-switch/error.log");
  // webui 没有落盘通道（不写服务端日志），设置页不展示路径。
  if (isWebui()) return Promise.resolve("");
  return call<string>("get_error_log_path");
}

/** 在文件管理器中定位错误日志（桌面端；日志尚未生成时由后端打开所在目录）。 */
export function revealErrorLog(): Promise<void> {
  if (demoModeEnabled || isWebui()) return Promise.resolve();
  return call<unknown>("reveal_error_log").then(() => undefined);
}

export type TraeClient = "desktop";
export interface TraeCreditPack { name: string; remaining: number; total: number; used: number; expiresAt?: number | null }
export interface TraeCredit {remaining:number|null;total?:number|null;used?:number|null;unlimited:boolean;available:boolean;checkedAt:number;unit:"usd"|"credits"|"unknown";expiresAt?:number|null;periodEndAt?:number|null;packs?:TraeCreditPack[]}
export interface TraeAccount { id:string;uid:string;name:string;email:string|null;variant:"cn"|"ai";client:TraeClient;capturedAt:number;expiresAt:number|null;needsLogin:boolean;credits?:TraeCredit|null }
export interface TraeStatus { accounts:TraeAccount[];clients:{variant:"cn"|"ai";client:TraeClient;name:string;detected:boolean;configured:boolean;running:boolean|null;activeAccountId:string|null;recognitionError:string|null}[];nativeCliSupported:false }
export interface TraeInput {variant?:"cn"|"ai";client?:TraeClient;accountId?:string;name?:string;closeRunning?:boolean;relaunch?:boolean;dryRun?:boolean;timeoutSeconds?:number;refresh?:boolean;settings?:TraeAutomation["settings"];passphrase?:string;data?:string;snapshotId?:string;path?:string}
export function getTraeStatus():Promise<TraeStatus> {return call("get_trae_status");}
export function traeAction(action:"capture"|"switch"|"credits"|"checkin"|"rename"|"delete"|"open"|"statistics"|"get_settings"|"save_settings"|"automation_run"|"backup_export"|"backup_preview"|"backup_import"|"cloud_backup"|"cloud_preview"|"cloud_restore"|"cloud_sync",input:TraeInput):Promise<unknown> {return call("trae_action",{action,input});}

export interface TraeSnapshot {accountId:string;name:string;variant:"cn"|"ai";at:number;credit:TraeCredit}
export interface TraeStatistics {generatedAt:number;retentionDays:number;variant:"cn"|"ai";accounts:TraeAccount[];snapshots:TraeSnapshot[];errors:{accountId:string;message:string}[]}
export interface TraeRegionSettings {refreshEnabled:boolean;intervalMinutes:number;dailyEnabled:boolean;dailyTime:string;alertsEnabled:boolean;expiryDays:number;lowBalance:number;rotateEnabled:boolean;rotateBelow:number;cooldownMinutes:number}
export interface TraeAutomation {settings:{cn:TraeRegionSettings;ai:TraeRegionSettings};logs:{at:number;region:"cn"|"ai";action:string;outcome:string}[]}
export interface TraeBackupPreview {accounts:{name:string;variant:"cn"|"ai";exists:boolean}[];count:number;snapshotId:string}
export function getTraeStatistics(variant:"cn"|"ai",refresh=false):Promise<TraeStatistics> {return traeAction("statistics",{variant,refresh}) as Promise<TraeStatistics>;}
export function getTraeAutomation():Promise<TraeAutomation> {return traeAction("get_settings",{}) as Promise<TraeAutomation>;}

// ---------------------------------------------------------------------------
// MiniMax Code
// ---------------------------------------------------------------------------

export type MinimaxRegion = "cn" | "en";
export interface MinimaxCreditPack { name:string; remaining:number; total:number; used:number; expiresAt?:number|null }
/** 额度与会员信息；`available=false` 表示官方未返回可识别余额（未知余额不算零）。 */
export interface MinimaxCredit {
  available:boolean; unit:"credits"|string; unlimited:boolean;
  remaining:number|null; total?:number|null; used?:number|null;
  checkedAt:number; expiresAt?:number|null;
  planName?:string|null; planExpiresAt?:number|null; hasTokenPlan?:boolean;
  dailyGiftRemaining?:number|null;
  packs?:MinimaxCreditPack[];
  error?:string|null;
}
export interface MinimaxAccount {
  id:string; uid:string; name:string; email:string|null; region:MinimaxRegion;
  capturedAt:number; expiresAt:number|null; needsLogin:boolean; active:boolean;
  credits?:MinimaxCredit|null;
  /** 仅在 `login` / `identity` 的结果里出现：新登录就是客户端当前登录的那个账号。 */
  matchesLiveClient?:boolean;
  /** 仅在 `identity` 的结果里出现：本次顺带合并掉的重复卡片数。 */
  mergedDuplicates?:number;
}
export interface MinimaxClient {
  region:MinimaxRegion; name:string;
  detected:boolean; configured:boolean;
  running:boolean|null; activeAccountId:string|null;
  tokenExpired:boolean;
  expiresAt:number|null;
  identity:{uid:string;name:string|null;email:string|null}|null;
  recognitionError:string|null;
}
/** 桌面端与 CLI 共用一份登录态，因此 `sharedLogin` 恒为 true、只有一个切换目标。 */
export interface MinimaxStatus { accounts:MinimaxAccount[]; clients:MinimaxClient[]; sharedLogin:true }
export interface MinimaxInput {region?:MinimaxRegion;accountId?:string;name?:string;closeRunning?:boolean;relaunch?:boolean;dryRun?:boolean;loginId?:string;timeoutSeconds?:number;refresh?:boolean;settings?:MinimaxAutomation["settings"];passphrase?:string;data?:string;snapshotId?:string;path?:string}
/** 设备码授权会话；`userCode` 显示给用户，`device_code` / `code_verifier` 只留在后端进程内。 */
export interface MinimaxLoginStart {loginId:string;region:MinimaxRegion;userCode:string;verificationUri:string;verificationUriComplete:string|null;expiresIn:number;intervalMs:number}
export type MinimaxLoginPoll =
  | {status:"pending";retryAfterMs?:number;slowDown?:boolean}
  | {status:"authorized";account:MinimaxAccount}
  | {status:"failed"|"expired";error:string};
export function getMinimaxStatus():Promise<MinimaxStatus> {return call("get_minimax_status");}
export function minimaxAction(action:"capture"|"switch"|"credits"|"rename"|"delete"|"open"|"identity"|"login_start"|"login_poll"|"login_cancel"|"checkin"|"checkin_status"|"statistics"|"token_stats"|"get_settings"|"save_settings"|"automation_run"|"backup_export"|"backup_preview"|"backup_import"|"cloud_backup"|"cloud_preview"|"cloud_restore"|"cloud_sync",input:MinimaxInput):Promise<unknown> {return call("minimax_action",{action,input});}

export interface MinimaxCheckin {ok:boolean;state:"claimed"|"already"|"closed"|"failed"|"auth"|"unconfirmed";message:string;credits:number|null;warning?:string;balance?:MinimaxCredit}
export interface MinimaxSnapshot {accountId:string;name:string;region:MinimaxRegion;at:number;credit:MinimaxCredit}
export interface MinimaxStatistics {generatedAt:number;retentionDays:number;region:MinimaxRegion;accounts:MinimaxAccount[];snapshots:MinimaxSnapshot[];errors:{accountId:string;message:string}[]}
export type MinimaxRegionSettings = TraeRegionSettings;
export interface MinimaxAutomation {settings:{cn:MinimaxRegionSettings;en:MinimaxRegionSettings};logs:{at:number;region:MinimaxRegion;action:string;outcome:string}[]}
export interface MinimaxBackupPreview {accounts:{name:string;region:MinimaxRegion;exists:boolean}[];count:number;snapshotId:string}
export interface MinimaxTokens {input:number;output:number;cacheRead:number;cacheWrite:number;total:number;requests:number}
export interface MinimaxTokenStatistics {generatedAt:number;available:boolean;accountAttributionAvailable:false;regionAttributionAvailable:false;totals:MinimaxTokens;sessionCount:number;fileCount:number;skippedRecords:number;complete:boolean;daily:{date:string;model:string;tokens:MinimaxTokens}[];models:{model:string;tokens:MinimaxTokens}[]}
export function getMinimaxStatistics(region:MinimaxRegion,refresh=false):Promise<MinimaxStatistics> {return minimaxAction("statistics",{region,refresh}) as Promise<MinimaxStatistics>;}
export function getMinimaxAutomation():Promise<MinimaxAutomation> {return minimaxAction("get_settings",{}) as Promise<MinimaxAutomation>;}

export interface DoubaoWindow {key:string;name:string;usedPercent:number|null;remainingPercent:number|null;exhausted:boolean;resetsAt:number|null}
export interface DoubaoCredit {available:boolean;unit:"percent";remaining:number|null;windows:DoubaoWindow[];planName:string|null;planActive:boolean;planExpiresAt:number|null;checkedAt:number}
export interface DoubaoCheckin {ok:boolean;state:"available"|"already"|"claimed"|"closed"|"unconfirmed";message:string;credits:number|null;reward?:number|null;continuousDays?:number|null;lastCheckinDate?:string|null;earnedTotal?:number|null;checkedAt:number;warning?:string}
export interface DoubaoAccount {id:string;uid:string;name:string;capturedAt:number;active:boolean;needsLogin:boolean;switchable:boolean;credits:DoubaoCredit|null;checkin:DoubaoCheckin|null;warning?:string}
export interface DoubaoStatus {accounts:DoubaoAccount[];client:{name:string;detected:boolean;configured:boolean;running:boolean|null;activeAccountId:string|null;identity:{uid:string;name:string}|null;recognitionError:string|null};capabilities:{nativeCli:false;tokenStatistics:false;portableDesktopSession:false}}
export interface DoubaoStatistics {generatedAt:number;retentionDays:number;accounts:DoubaoAccount[];snapshots:{accountId:string;name:string;at:number;credit:DoubaoCredit}[];errors:{accountId:string;message:string}[]}
export type DoubaoSettings = TraeRegionSettings;
export interface DoubaoAutomation {settings:DoubaoSettings;logs:{at:number;action:string;outcome:string}[]}
export interface DoubaoBackupPreview {accounts:{name:string;exists:boolean}[];count:number;snapshotId:string;desktopSessionIncluded:false}
export interface DoubaoInput {accountId?:string;name?:string;closeRunning?:boolean;relaunch?:boolean;dryRun?:boolean;timeoutSeconds?:number;refresh?:boolean;settings?:DoubaoSettings;passphrase?:string;data?:string;snapshotId?:string;path?:string}
export function getDoubaoStatus():Promise<DoubaoStatus>{return call("get_doubao_status");}
export function doubaoAction(action:"capture"|"switch"|"credits"|"rename"|"delete"|"open"|"checkin"|"checkin_status"|"statistics"|"token_stats"|"get_settings"|"save_settings"|"automation_run"|"backup_export"|"backup_preview"|"backup_import"|"cloud_backup"|"cloud_preview"|"cloud_restore"|"cloud_sync",input:DoubaoInput):Promise<unknown>{return call("doubao_action",{action,input});}
export function getDoubaoStatistics(refresh=false):Promise<DoubaoStatistics>{return doubaoAction("statistics",{refresh}) as Promise<DoubaoStatistics>;}
export function getDoubaoAutomation():Promise<DoubaoAutomation>{return doubaoAction("get_settings",{}) as Promise<DoubaoAutomation>;}
