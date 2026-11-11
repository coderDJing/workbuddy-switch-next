import type { ReactNode } from "react";
import { Link, NavLink } from "react-router-dom";
import { Settings } from "lucide-react";

import { AppIconMark, DoubaoMark, MinimaxMark, QoderMark, TraeMark, WorkBuddyMark } from "@/components/product-marks";
import { Badge } from "@/components/ui/badge";
import { Tooltip, TooltipContent, TooltipTrigger } from "@/components/ui/tooltip";
import { platformPath, type PlatformDef, type PlatformId } from "@/lib/platforms";
import { cn } from "@/lib/utils";
import { AiCliDialog } from "@/components/ai-cli-dialog";

/** 平台栏图标：按产品线取产品图标。 */
export function PlatformMark({ id, size }: { id: PlatformId; size: number }) {
  if (id === "doubao") return <DoubaoMark size={size} />;
  if (id === "minimax") return <MinimaxMark size={size} />;
  return id === "trae" ? <TraeMark size={size} /> : id === "qoder" ? <QoderMark size={size} /> : <WorkBuddyMark size={size} />;
}

const railItemClass = (active: boolean) =>
  cn(
    "relative flex size-10 items-center justify-center rounded-xl outline-none transition-colors focus-visible:ring-2 focus-visible:ring-sidebar-ring/50",
    active ? "bg-primary/10 ring-1 ring-primary/20" : "opacity-70 hover:bg-foreground/[0.05] hover:opacity-100",
  );

/** 激活指示：平台栏左缘的竖条。 */
function ActiveBar({ show }: { show: boolean }) {
  if (!show) return null;
  return <span aria-hidden className="absolute -left-2 top-2 h-6 w-1 rounded-r-full bg-primary" />;
}

interface RailProps {
  platforms: PlatformDef[];
  /** 当前所在平台；在全局设置页时为上次所在平台（不高亮）。 */
  activePlatform?: PlatformId;
  onSettings: boolean;
  /** 各平台客户端是否在运行；缺省不画状态点。 */
  running: Partial<Record<PlatformId, boolean>>;
  statusLabels: Partial<Record<PlatformId, string>>;
  topPadding: string;
  macTitleBar: boolean;
}

/** 最左侧平台栏：应用标识、各平台图标（带运行状态点）、底部全局设置。 */
export function PlatformRail({ platforms, activePlatform, onSettings, running, statusLabels, topPadding, macTitleBar }: RailProps) {
  return (
    <nav
      aria-label="平台"
      className={cn(
        "next-platform-rail relative flex w-12 shrink-0 flex-col items-center gap-2 bg-sidebar pb-4 md:w-14",
        "after:absolute after:bottom-0 after:right-0 after:w-px after:bg-sidebar-border after:content-['']",
        macTitleBar ? "after:top-11" : "after:top-0",
        topPadding,
      )}
    >
      <Tooltip>
        <TooltipTrigger asChild>
          <span className="mb-3 inline-flex">
            <AppIconMark size={32} className="drop-shadow-sm" />
          </span>
        </TooltipTrigger>
        <TooltipContent side="right">WorkBuddy Switch Next</TooltipContent>
      </Tooltip>
      {platforms.map((p) => {
        const active = !onSettings && p.id === activePlatform;
        const isRunning = running[p.id];
        return (
          <Tooltip key={p.id}>
            <TooltipTrigger asChild>
              <Link to={platformPath(p.id)} aria-label={p.label} aria-current={active ? "page" : undefined} className={railItemClass(active)}>
                <ActiveBar show={active} />
                <PlatformMark id={p.id} size={28} />
                {isRunning !== undefined && (
                  <span
                    aria-hidden
                    className={cn(
                      "absolute bottom-1 right-1 size-2 rounded-full ring-1 ring-sidebar",
                      isRunning ? "bg-emerald-500" : "bg-zinc-500 dark:bg-zinc-400",
                    )}
                  />
                )}
              </Link>
            </TooltipTrigger>
            <TooltipContent side="right">
              {p.label} · {statusLabels[p.id] ?? "客户端"}：{isRunning === undefined ? "状态未知" : isRunning ? "运行中" : "未运行"}
            </TooltipContent>
          </Tooltip>
        );
      })}
      <div className="mt-auto">
        <Tooltip>
          <TooltipTrigger asChild>
            <Link to="/settings" aria-label="设置" aria-current={onSettings ? "page" : undefined} className={railItemClass(onSettings)}>
              <ActiveBar show={onSettings} />
              <Settings className="size-5 text-sidebar-foreground" />
            </Link>
          </TooltipTrigger>
          <TooltipContent side="right">设置</TooltipContent>
        </Tooltip>
      </div>
    </nav>
  );
}

interface NavProps {
  platform?: PlatformDef;
  /** 用户在设置里隐藏「关联会话」时为 false。 */
  sessionsNavEnabled: boolean;
  demoMode: boolean;
  topPadding: string;
  /** 底部区域（版本与更新、悬浮窗开关）。 */
  footer: ReactNode;
}

/** 平台导航：只列出当前平台支持的页面。 */
export function PlatformNav({ platform, sessionsNavEnabled, demoMode, topPadding, footer }: NavProps) {
  const pages = platform?.pages.filter((page) => page.key !== "sessions" || sessionsNavEnabled) ?? [];
  const sections = [
    { label: "工作空间", keys: ["accounts", "sessions"] },
    { label: "用量洞察", keys: ["token-stats", "credit-stats"] },
    { label: "自动化与配置", keys: ["settings"] },
  ];
  return (
    <aside className={cn("flex min-h-0 w-14 shrink-0 flex-col border-r border-sidebar-border bg-sidebar px-1.5 pb-4 md:w-[224px] md:px-4", topPadding)}>
      <div className="hidden px-2 pb-6 pt-1 md:block">
        <div className="flex items-center gap-2 font-semibold tracking-tight"><span className="text-[15px]">Switch</span><span className="rounded-md bg-primary px-1.5 py-0.5 text-[10px] tracking-widest text-primary-foreground">NEXT</span></div>
        <p className="mt-1.5 text-[11px] text-muted-foreground">账号 · 用量 · 自动化</p>
      </div>
      <div className="mb-3 hidden rounded-xl border border-sidebar-border bg-card/70 px-3 py-3 md:block">
        <p className="mb-1 text-[10px] font-medium tracking-wider text-muted-foreground">当前平台</p>
        <div className="text-[13px] font-semibold tracking-tight" title={platform?.label}>{platform?.label ?? "所有平台已隐藏"}</div>
        {demoMode && (
          <Badge variant="secondary" className="mt-1 h-5 border-0 px-1.5 text-[10px] text-sidebar-foreground/60 shadow-none">
            演示模式
          </Badge>
        )}
      </div>
      <nav className="min-h-0 flex-1 overflow-y-auto pb-4" aria-label={platform ? `${platform.label} 导航` : "应用导航"}>
        {platform && sections.filter(section => pages.some(page => section.keys.includes(page.key))).map(section => <div key={section.label} className="mb-4 space-y-1">
          <p className="hidden px-3 pb-1 pt-2 text-[10px] font-medium tracking-wide text-muted-foreground md:block">{section.label}</p>
          {pages.filter(page => section.keys.includes(page.key)).map((page) => (
          <NavLink
            key={page.key}
            to={platformPath(platform.id, page.key)}
            aria-label={page.label}
            title={page.label}
            className={({ isActive }) =>
              cn(
                "flex items-center justify-center gap-2.5 rounded-lg px-2 py-3 text-[13px] outline-none transition-colors focus-visible:ring-2 focus-visible:ring-sidebar-ring/50 md:justify-start md:px-3 md:py-2.5",
                isActive
                  ? "bg-primary/10 font-semibold text-primary ring-1 ring-inset ring-primary/10"
                  : "text-muted-foreground hover:bg-foreground/[0.04] hover:text-foreground",
              )
            }
          >
            <page.icon className="size-4 shrink-0" aria-hidden />
            <span className="hidden md:inline">{page.label}</span>
          </NavLink>
        ))}</div>)}
      </nav>
      {platform && <AiCliDialog platform={platform.id} />}
      {footer}
    </aside>
  );
}
