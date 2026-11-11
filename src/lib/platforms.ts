import type { LucideIcon } from "lucide-react";
import { useEffect, useState } from "react";
import { Link2, MessagesSquare, SlidersHorizontal, Sparkles, User } from "lucide-react";

import { useSupportedTools, type ToolId } from "./supported-tools";

/**
 * 平台注册表：侧栏平台栏、平台导航与路由都由这里生成。
 *
 * 平台按产品线命名，彼此平级、账号库互不相通。新增平台（如 Trae）= 在这里加一条
 * + 一个后端模块；页面里不要写 `platform === "xxx"` 判断能力，改这里的 `pages`。
 */
export type PlatformId = "codebuddy" | "qoder" | "trae" | "minimax" | "doubao";

/** 平台内的页面。同一个 key 在不同平台由各自的页面组件实现。 */
export type PlatformPageKey = "accounts" | "sessions" | "token-stats" | "credit-stats" | "settings";

export interface PlatformPageDef {
  key: PlatformPageKey;
  label: string;
  icon: LucideIcon;
}

export interface PlatformDef {
  id: PlatformId;
  /** 平台栏 tooltip 与平台导航标题。 */
  label: string;
  /** 关联客户端：用于兼容旧版尚未设置平台显示偏好的用户。 */
  tools: ToolId[];
  /** 该平台支持的页面，按导航顺序；第一项是平台首页。 */
  pages: PlatformPageDef[];
}

const ACCOUNTS: PlatformPageDef = { key: "accounts", label: "账号管理", icon: User };

export const PLATFORMS: PlatformDef[] = [
  {
    id: "codebuddy",
    label: "WorkBuddy / CodeBuddy",
    tools: ["workbuddy", "codebuddyIde", "codebuddyCli", "vscodeExt", "jetbrains"],
    pages: [
      ACCOUNTS,
      { key: "sessions", label: "关联会话", icon: Link2 },
      { key: "token-stats", label: "Token 统计", icon: MessagesSquare },
      { key: "credit-stats", label: "积分统计", icon: Sparkles },
      { key: "settings", label: "平台设置", icon: SlidersHorizontal },
    ],
  },
  {
    id: "qoder",
    label: "Qoder",
    tools: ["qoder"],
    pages: [
      ACCOUNTS,
      { key: "token-stats", label: "Token 统计", icon: MessagesSquare },
      { key: "credit-stats", label: "积分统计", icon: Sparkles },
      { key: "settings", label: "平台设置", icon: SlidersHorizontal },
    ],
  },
  { id: "trae", label: "TRAE", tools: ["trae"], pages: [ACCOUNTS, { key: "credit-stats", label: "积分统计", icon: Sparkles }, { key: "settings", label: "平台设置", icon: SlidersHorizontal }] },
  /** MiniMax Code 的桌面端与 CLI 共用登录态及本机会话用量。 */
  { id: "minimax", label: "MiniMax Code", tools: ["minimax"], pages: [ACCOUNTS, { key: "token-stats", label: "Token 统计", icon: MessagesSquare }, { key: "credit-stats", label: "积分统计", icon: Sparkles }, { key: "settings", label: "平台设置", icon: SlidersHorizontal }] },
  { id: "doubao", label: "豆包", tools: ["doubao"], pages: [ACCOUNTS, {key: "credit-stats", label: "额度统计", icon: Sparkles}, {key: "settings", label: "平台设置", icon: SlidersHorizontal}] },
];

export function platformPath(platform: PlatformId, page: PlatformPageKey = "accounts"): string {
  return `/p/${platform}/${page}`;
}

export function findPlatform(id: string | undefined): PlatformDef | undefined {
  return PLATFORMS.find((p) => p.id === id);
}

const PLATFORM_VISIBILITY_EVENT = "wb-switch:platform-visibility-changed";

export function readPlatformVisibility(platform: PlatformDef, tools: Record<ToolId, boolean>): boolean {
  try {
    const stored = localStorage.getItem(`wb-switch.platform-visible.${platform.id}`);
    if (stored !== null) return stored === "1";
  } catch { /* 存储不可用时兼容原有工具开关。 */ }
  return platform.tools.some(tool => tools[tool]);
}

export function setPlatformVisible(id: PlatformId, visible: boolean): void {
  try { localStorage.setItem(`wb-switch.platform-visible.${id}`, visible ? "1" : "0"); }
  catch { /* 与其他界面偏好一致，存储不可用时不影响账号。 */ }
  window.dispatchEvent(new Event(PLATFORM_VISIBILITY_EVENT));
}

export function usePlatformVisibility(): Record<PlatformId, boolean> {
  const tools = useSupportedTools();
  const [, refresh] = useState(0);
  useEffect(() => {
    const sync = () => refresh(value => value + 1);
    window.addEventListener("storage", sync);
    window.addEventListener(PLATFORM_VISIBILITY_EVENT, sync);
    return () => {
      window.removeEventListener("storage", sync);
      window.removeEventListener(PLATFORM_VISIBILITY_EVENT, sync);
    };
  }, []);
  return Object.fromEntries(PLATFORMS.map(p => [p.id, readPlatformVisibility(p, tools)])) as Record<PlatformId, boolean>;
}

/** 全部隐藏时返回空列表，由路由保留全局设置入口。 */
export function useEnabledPlatforms(): PlatformDef[] {
  const visible = usePlatformVisibility();
  return PLATFORMS.filter(p => visible[p.id]);
}

const LAST_PLATFORM_KEY = "wb-switch.platform";
/** 上一版「腾讯系 / Qoder」两段式切换的存储键，读到时迁移。 */
const LEGACY_VENDOR_KEY = "wb-switch.vendor";

export function readLastPlatform(): PlatformId | undefined {
  try {
    const stored = localStorage.getItem(LAST_PLATFORM_KEY);
    if (findPlatform(stored ?? undefined)) return stored as PlatformId;
    if (localStorage.getItem(LEGACY_VENDOR_KEY) === "qoder") return "qoder";
  } catch {
    /* 读不到就用默认平台 */
  }
  return undefined;
}

export function rememberPlatform(id: PlatformId): void {
  try {
    localStorage.setItem(LAST_PLATFORM_KEY, id);
    localStorage.removeItem(LEGACY_VENDOR_KEY);
  } catch {
    /* 隐私模式下写入失败：只影响下次启动落在哪个平台 */
  }
}
