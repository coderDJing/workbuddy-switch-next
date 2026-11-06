import { useCallback, useEffect, useRef, useState } from "react";
import { Check, Clock3, Columns3, Ellipsis, FileDown, FileUp, Gift, Info, Loader2, LogIn, Pencil, RefreshCw, Rows3, SlidersHorizontal, Sparkles, Trash2 } from "lucide-react";
import { toast } from "sonner";

import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Tabs, TabsList, TabsTrigger } from "@/components/ui/tabs";
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from "@/components/ui/select";
import { Dialog, DialogContent, DialogDescription, DialogHeader, DialogTitle } from "@/components/ui/dialog";
import { DropdownMenu, DropdownMenuContent, DropdownMenuItem, DropdownMenuSeparator, DropdownMenuTrigger } from "@/components/ui/dropdown-menu";
import {
  AlertDialog,
  AlertDialogAction,
  AlertDialogCancel,
  AlertDialogContent,
  AlertDialogDescription,
  AlertDialogFooter,
  AlertDialogHeader,
  AlertDialogTitle,
} from "@/components/ui/alert-dialog";
import { DemoAction } from "@/components/demo-action";
import { QoderCliMark, QoderMark } from "@/components/product-marks";
import { QoderLoginDialog } from "@/components/qoder-login-dialog";
import { QoderBackupDialog } from "@/components/qoder-backup-dialog";
import * as api from "@/lib/api";
import type { QoderBundle, QoderCredit, QoderTarget, QoderTargetStatus } from "@/lib/types";
import { cn } from "@/lib/utils";
import { avatarTone } from "@/lib/avatar-tone";
import { normalizeVariant, variantLabel } from "@/lib/variant";
import { groupQoderAccounts, orderQoderAccounts, qoderExpiry, QODER_SORT_OPTIONS, type QoderAccountGroup, type QoderSortKey } from "@/lib/qoder-accounts";
import { qoderTargets, useQoderView } from "@/stores/qoder-view";
import { useQoderStatus } from "@/stores/qoder-status";

type Pending = { kind: "switch"; bundle: QoderBundle } | { kind: "delete"; group: QoderAccountGroup };

const TARGET_LABELS: Record<QoderTarget, string> = {
  desktop: "Qoder CN", cli: "Qoder CN CLI", global_desktop: "Qoder 国际版", global_cli: "Qoder 国际版 CLI",
};
const isCli = (target: QoderTarget) => target === "cli" || target === "global_cli";

function formatTime(ms?: number | null): string {
  if (!ms) return "—";
  return new Date(ms).toLocaleString("zh-CN", { hour12: false });
}

type CreditObserver = (id: string, credit: QoderCredit) => void;
type LoadingObserver = (id: string, loading: boolean) => void;

function QoderCreditPanel({ bundleIds, liveTarget, compact, creditId, refreshVersion, onCredit, onLoading }: {
  bundleIds: string[]; liveTarget?: QoderTarget; compact: boolean; creditId: string; refreshVersion: number;
  onCredit: CreditObserver; onLoading: LoadingObserver;
}) {
  const [credit, setCredit] = useState<QoderCredit | null>(null);
  const [loading, setLoading] = useState(true);
  const [updatedAt, setUpdatedAt] = useState<number | null>(null);
  const idsKey = bundleIds.join("|");
  useEffect(() => {
    let stopped = false;
    let inFlight = false;
    async function load() {
      if (inFlight) return;
      inFlight = true;
      setLoading(true);
      onLoading(creditId, true);
      try {
        const value = await api.getQoderCredits(idsKey ? idsKey.split("|") : [], liveTarget);
        if (!stopped) { setCredit(value); onCredit(creditId, value); if (value.ok) setUpdatedAt(Date.now()); }
      } catch {
        if (!stopped) { const value: QoderCredit = { ok: false, error: "积分查询失败，请稍后重试" }; setCredit(value); onCredit(creditId, value); }
      } finally {
        inFlight = false;
        if (!stopped) { setLoading(false); onLoading(creditId, false); }
      }
    }
    void load();
    const timer = window.setInterval(() => void load(), 5 * 60 * 1000);
    return () => { stopped = true; window.clearInterval(timer); };
  }, [idsKey, liveTarget, refreshVersion, creditId, onCredit, onLoading]);
  const format = (value: number) => new Intl.NumberFormat("zh-CN", { maximumFractionDigits: 2 }).format(value);
  if (loading && !credit) return <div className="flex items-center gap-2 py-3 text-sm text-muted-foreground"><Loader2 className="size-4 animate-spin" />积分查询中…</div>;
  if (!credit?.ok) return <div className="py-3 text-sm text-destructive">{credit?.error ?? "等待积分数据…"}</div>;
  const ratio = credit.total > 0 ? Math.min(100, Math.max(0, credit.remaining / credit.total * 100)) : 0;
  const expires = qoderExpiry(credit);
  return <div className="min-w-0">
    {expires && expires - Date.now() <= 7 * 86400000 ? <div className="mb-3 rounded-lg bg-amber-500/10 px-2 py-1 text-xs text-amber-700 dark:text-amber-300">有积分将在 {Math.max(1, Math.ceil((expires - Date.now()) / 86400000))} 天内到期，建议优先使用</div> : null}
    <div className="flex items-baseline gap-x-3 gap-y-1">
      <span className="flex items-center gap-1.5"><Sparkles className="size-4 shrink-0 stroke-[1.75] text-muted-foreground" aria-hidden="true" /><strong className={cn("font-semibold leading-none tabular-nums tracking-[-0.025em] text-foreground", compact ? "text-[20px]" : "text-[22px]")} style={{ fontFamily: '"Bricolage Grotesque Variable", "SF Pro Display", ui-sans-serif, sans-serif' }}>{format(credit.remaining)}</strong></span>
      <span className={cn("text-muted-foreground", compact ? "text-[11px]" : "text-xs")}>剩余积分</span>
      <div className={cn("ml-auto flex items-center gap-1.5 text-muted-foreground", compact ? "text-[11px]" : "text-xs")}><Clock3 className="size-3.5 shrink-0" /><span className="whitespace-nowrap tabular-nums">{loading ? "更新中…" : updatedAt ? `${new Date(updatedAt).toLocaleTimeString("zh-CN", { hour: "2-digit", minute: "2-digit", hour12: false })} 更新` : "—"}</span></div>
    </div>
    <div className={cn("text-[11px] font-medium text-muted-foreground", compact ? "mt-3" : "mt-4")}>积分总览</div>
    <div className={cn("space-y-2", compact ? "mt-3" : "mt-4")}>
      <div className="flex items-center justify-between gap-3 text-[11px]"><span className="rounded-lg bg-muted/80 px-1.5 py-0.5 font-medium tabular-nums text-foreground">{format(credit.remaining)} 积分</span><span className="tabular-nums">总计 {format(credit.total)}</span></div>
      <div className="h-1 overflow-hidden rounded-full bg-muted"><div className="h-full rounded-full bg-primary" style={{ width: `${ratio}%` }} /></div>
      <div className="text-[11px] tabular-nums text-muted-foreground">已用 {format(credit.used)} 积分</div>
      {(credit.plan?.total || credit.addOn?.total || credit.planExpiresAt || credit.dedicated?.length) ? <div className="space-y-1.5 border-t border-border/60 pt-2">
        {credit.plan?.total ? <div className="flex items-center justify-between gap-2"><span>套餐积分 · 剩余 {format(credit.plan.remaining)}</span><span className="shrink-0 tabular-nums">{credit.planExpiresAt ? `套餐 ${new Date(credit.planExpiresAt).toLocaleDateString("zh-CN")} 到期` : "套餐到期时间未提供"}</span></div> : credit.planExpiresAt ? <div className="flex items-center justify-between gap-2"><span>套餐</span><span className="shrink-0 tabular-nums">{new Date(credit.planExpiresAt).toLocaleDateString("zh-CN")} 到期</span></div> : null}
        {credit.addOn?.total ? <div className="flex items-center justify-between gap-2"><span>额外积分 · 剩余 {format(credit.addOn.remaining)}</span><span className="shrink-0">到期时间未提供</span></div> : null}
        {credit.dedicated?.filter((part) => part.total > 0).map((part, index) => <div key={`${part.expiresAt}-${index}`} className="flex items-center justify-between gap-2"><span>专属积分 · 剩余 {format(part.remaining)}</span><span className="shrink-0 tabular-nums">{part.expiresAt ? `${new Date(part.expiresAt).toLocaleDateString("zh-CN")} 到期` : "到期时间未提供"}</span></div>)}
      </div> : null}
    </div>
  </div>;
}

function stateText(s: QoderTargetStatus | undefined): string {
  if (!s?.installed) return "未检测到";
  if (!s.loggedIn) return "未登录";
  return s.running ? "运行中" : "已接入";
}

function TargetMark({ target, size }: { target: QoderTarget; size: number }) {
  const global = target === "global_desktop" || target === "global_cli";
  return isCli(target) ? <QoderCliMark size={size} global={global} /> : <QoderMark size={size} global={global} />;
}

/** 页顶状态徽标：与 WorkBuddy / CodeBuddy 各端同一套样式（接入 = 品牌色描边，未接入 = 灰）。 */
function HeaderBadge({ target, status }: { target: QoderTarget; status: QoderTargetStatus | undefined }) {
  return (
    <span className="group relative inline-flex cursor-default">
      <span
        className={
          status?.loggedIn
            ? "inline-flex rounded-[22%] bg-primary p-[2px] shadow-sm shadow-primary/40"
            : "inline-flex rounded-[22%] bg-muted-foreground/30 p-[2px]"
        }
      >
        <TargetMark target={target} size={28} />
      </span>
      <span className="pointer-events-none absolute right-0 top-full z-50 mt-2 hidden whitespace-nowrap rounded-md bg-popover px-2.5 py-1.5 text-xs text-popover-foreground shadow-lg ring-1 ring-black/5 group-hover:block">
        {status?.label ?? TARGET_LABELS[target]}：{stateText(status)} · 当前账号：
        {status?.currentLabel ?? (status?.loggedIn ? status.echoName ?? "尚未导入" : "未登录")}
      </span>
    </span>
  );
}

interface CardProps {
  group: QoderAccountGroup;
  targets: readonly QoderTarget[];
  targetStatus: (target: QoderTarget) => QoderTargetStatus | undefined;
  compact: boolean;
  /** 任一 Qoder 操作进行中：禁用全部按钮，防止并发切换。 */
  busy: boolean;
  busyKey: string | null;
  editing: boolean;
  draft: string;
  onDraftChange: (value: string) => void;
  onStartRename: () => void;
  onCommitRename: () => void;
  onCancelRename: () => void;
  onSwitch: (bundle: QoderBundle) => void;
  onDelete: () => void;
  refreshVersion: number;
  onCredit: CreditObserver;
  onLoading: LoadingObserver;
}

/** Qoder 账号卡片：外观与 WorkBuddy / CodeBuddy 账号卡片一致（圆角卡片、header 标识、footer 操作）。 */
function QoderAccountCard({ group, targets, targetStatus, compact, busy, busyKey, editing, draft, onDraftChange, onStartRename, onCommitRename, onCancelRename, onSwitch, onDelete, refreshVersion, onCredit, onLoading }: CardProps) {
  const [infoOpen, setInfoOpen] = useState(false);
  const [localRefresh, setLocalRefresh] = useState(0);
  const activeFor = (target: QoderTarget) => group.bundles.some((bundle) => bundle.target === target && (bundle.id === targetStatus(target)?.currentId || (bundle.identityKey && bundle.identityKey === targetStatus(target)?.currentIdentityKey)));
  const activeTargets = targets.filter(activeFor);
  const active = activeTargets.length > 0;
  const first = group.bundles[0];
  const liveTarget = activeTargets[0];
  const switchControls = <span className="inline-flex items-center gap-1.5">{targets.map((target) => {
    const bundle = group.targets[target];
    const targetLabel = TARGET_LABELS[target];
    if (activeFor(target)) return <span key={target} className="relative inline-flex size-7 shrink-0 items-center justify-center rounded-lg border border-primary/25 bg-primary/10" aria-label={`${targetLabel} 当前账号`} title={`${targetLabel} 当前账号`}>
      <TargetMark target={target} size={15} /><span className="absolute -right-1 -top-1 flex size-3.5 items-center justify-center rounded-full bg-primary text-primary-foreground"><Check className="size-2.5" strokeWidth={3} /></span>
    </span>;
    return <DemoAction key={target}><Button variant="outline" size="icon" className="size-7 rounded-lg" disabled={busy || !bundle} onClick={() => bundle && onSwitch(bundle)} aria-label={`切换 ${targetLabel}`} title={bundle ? `将 ${targetLabel} 切换到此账号` : `${targetLabel} 尚未准备好，请先登录客户端并重新添加`}>
      {bundle && busyKey === bundle.id ? <Loader2 className="size-4 animate-spin" /> : <TargetMark target={target} size={15} />}
    </Button></DemoAction>;
  })}</span>;
  return (
    <article
      className={cn(
        "flex min-w-0 flex-col overflow-hidden rounded-2xl border border-border bg-card shadow-[0_1px_2px_rgba(15,23,42,.025),0_10px_28px_rgba(15,23,42,.035)] transition-shadow hover:shadow-[0_2px_4px_rgba(15,23,42,.04),0_14px_34px_rgba(15,23,42,.055)]",
      )}
    >
      <header className={cn("relative flex items-center gap-2 border-b border-border", compact ? "min-h-[52px] px-3.5 py-1.5" : "min-h-[104px] px-5 py-3", active ? "bg-primary/5" : "bg-muted/30")}>
        <div className="pointer-events-none absolute inset-0 overflow-hidden">
          <div className={cn("absolute -right-10 -top-16 rounded-full blur-2xl", compact ? "size-20" : "size-24", active ? "bg-primary/15" : "bg-muted/30")} />
          {activeTargets.map((target, index) => (
            <div key={target} className={cn(
              "absolute top-[64%] -translate-y-1/2 opacity-[0.075] saturate-50 grayscale-[10%]",
              activeTargets.length > 1
                ? index === 0 ? "right-[68px] rotate-[8deg]" : "right-1 -rotate-[8deg]"
                : "right-5 rotate-[7deg]",
            )}>
              <TargetMark target={target} size={compact ? 40 : 56} />
            </div>
          ))}
        </div>
        {!compact && <div className={cn("relative z-10 flex size-12 shrink-0 items-center justify-center rounded-full text-base font-semibold ring-4 ring-white/65", avatarTone(group.label))}>{group.label.charAt(0).toUpperCase()}</div>}
        <div className="relative z-10 min-w-0 flex-1">
          {editing ? (
            <Input
              autoFocus
              aria-label="账号名称"
              value={draft}
              onChange={(e) => onDraftChange(e.target.value)}
              onBlur={onCommitRename}
              onKeyDown={(e) => {
                if (e.key === "Enter") onCommitRename();
                if (e.key === "Escape") onCancelRename();
              }}
              className="h-8"
            />
          ) : (
            <div className={cn("truncate font-semibold", compact ? "text-[13px]" : "text-sm")} title={group.label}>
              {group.label}
            </div>
          )}
          {!compact && <div className="truncate text-xs text-muted-foreground">
            Qoder 账号
          </div>}
        </div>
        {compact && <span className="relative z-10">{switchControls}</span>}
        <DropdownMenu>
          <DropdownMenuTrigger asChild>
            <Button variant="ghost" size="icon" className="relative z-20 size-7 rounded-lg text-muted-foreground" aria-label={`管理账号 ${group.label}`} disabled={busy}><Ellipsis /></Button>
          </DropdownMenuTrigger>
          <DropdownMenuContent align="end" className="w-40">
            {!api.isDemoMode() && <DropdownMenuItem onSelect={() => setLocalRefresh((value) => value + 1)}><RefreshCw />刷新积分</DropdownMenuItem>}
            <DropdownMenuItem onSelect={() => setInfoOpen(true)}><Info />账号信息</DropdownMenuItem>
            {!api.isDemoMode() && <><DropdownMenuItem onSelect={onStartRename}><Pencil />重命名</DropdownMenuItem><DropdownMenuSeparator /><DropdownMenuItem className="text-destructive focus:bg-destructive/5 focus:text-destructive" onSelect={onDelete}><Trash2 />删除账号</DropdownMenuItem></>}
          </DropdownMenuContent>
        </DropdownMenu>
      </header>
      <div className={cn("flex-1 text-xs leading-5 text-muted-foreground", compact ? "px-3.5 pb-4 pt-3" : "px-5 pb-5 pt-4")}>
        <QoderCreditPanel bundleIds={group.bundles.map((bundle) => bundle.id)} liveTarget={liveTarget} compact={compact} creditId={group.key} refreshVersion={refreshVersion + localRefresh} onCredit={onCredit} onLoading={onLoading} />
      </div>
      {!compact && <footer className="flex items-center gap-2 border-t border-border px-5 py-2.5">{switchControls}{active && <span className="text-xs text-muted-foreground">当前使用</span>}</footer>}
      <Dialog open={infoOpen} onOpenChange={setInfoOpen}>
        <DialogContent><DialogHeader><DialogTitle>账号信息</DialogTitle><DialogDescription>{group.label}</DialogDescription></DialogHeader>
          <div className="space-y-2 text-sm text-muted-foreground"><div>添加于 {formatTime(first.capturedAt)}</div><div>上次切换 {formatTime(Math.max(0, ...group.bundles.map((b) => b.lastSwitchedAt ?? 0)))}</div></div>
        </DialogContent>
      </Dialog>
    </article>
  );
}

/** 本机已登录但尚未保存的账号，也应出现在账号列表中。 */
function CurrentLoginCard({ status, compact, busy, saving, onSave, needsAuthorization, refreshVersion, onCredit, onLoading }: { status: QoderTargetStatus; compact: boolean; busy: boolean; saving: boolean; onSave: () => void; needsAuthorization: boolean; refreshVersion: number; onCredit: CreditObserver; onLoading: LoadingObserver }) {
  return (
    <article className="flex min-w-0 flex-col overflow-hidden rounded-2xl border border-primary/50 bg-card shadow-sm">
      <header className={cn("flex items-center gap-3 border-b border-border bg-primary/5", compact ? "min-h-[52px] px-3.5 py-1.5" : "min-h-[104px] px-5 py-3")}>
        {!compact && <TargetMark target={status.target} size={32} />}
        <div className="min-w-0 flex-1">
          <div className={cn("truncate font-semibold", compact ? "text-[13px]" : "text-sm")}>{status.echoName || "当前登录"}</div>
          <div className="truncate text-xs text-muted-foreground">{status.label}</div>
        </div>
        <Badge variant="secondary" className="shrink-0 gap-1"><Check className="size-3" />当前</Badge>
      </header>
      <div className={cn("flex-1 text-xs leading-5 text-muted-foreground", compact ? "px-3.5 pb-4 pt-3" : "px-5 pb-5 pt-4")}>
        <QoderCreditPanel bundleIds={[]} liveTarget={status.target} compact={compact} creditId={`live:${status.target}`} refreshVersion={refreshVersion} onCredit={onCredit} onLoading={onLoading} />
        <p className="mt-3">{needsAuthorization ? "已检测到本机登录，请授权访问系统密钥后识别账号。" : "已检测到本机登录，但自动识别未完成，请重试。"}</p>
      </div>
      <footer className={cn("border-t border-border", compact ? "px-3.5 py-2" : "px-5 py-2.5")}>
        <DemoAction>
          <Button size="sm" className="h-7 rounded-full px-3 text-xs" disabled={busy} onClick={onSave}>
            {saving && <Loader2 className="animate-spin" />}{needsAuthorization ? "授权识别" : "重试识别"}
          </Button>
        </DemoAction>
      </footer>
    </article>
  );
}

interface Props {
  compact?: boolean;
  onToggleCompact?: () => void;
}

/**
 * Qoder 账号管理：与 WorkBuddy / CodeBuddy 账号页同一套版式（页顶徽标、添加账号面板、账号卡片网格）。
 *
 * Qoder 账号与 WorkBuddy / CodeBuddy 账号库不相通，这里的「账号」是账号包 —— 整组加密凭据文件的
 * 本机副本。认领 = 保存现场登录态；切换 = 备份现场后整组写回并校验。
 */
export function QoderAccountsView({ compact = false, onToggleCompact }: Props) {
  const variant = useQoderView((state) => state.variant);
  const setVariant = useQoderView((state) => state.setVariant);
  const status = useQoderStatus((state) => state.status);
  const loadError = useQoderStatus((state) => state.error);
  const refresh = useQoderStatus((state) => state.refresh);
  const [busyKey, setBusyKey] = useState<string | null>(null);
  const [pending, setPending] = useState<Pending | null>(null);
  const [editingId, setEditingId] = useState<string | null>(null);
  const [draft, setDraft] = useState("");
  const [loginTarget, setLoginTarget] = useState<QoderTarget | null>(null);
  const [backupMode, setBackupMode] = useState<"import" | "export" | null>(null);
  const [autoAttempt, setAutoAttempt] = useState(0);
  const [autoReady, setAutoReady] = useState(api.isDemoMode());
  const [autoError, setAutoError] = useState<string | null>(null);
  const [autoFailures, setAutoFailures] = useState<QoderTarget[]>([]);
  const [authorizationRequired, setAuthorizationRequired] = useState<QoderTarget[]>([]);
  const autoInFlight = useRef<ReturnType<typeof api.autoCaptureQoderAccounts> | null>(null);
  const [creditVersion, setCreditVersion] = useState(0);
  const [claimingRewards, setClaimingRewards] = useState(false);
  const [credits, setCredits] = useState<Record<string, QoderCredit>>({});
  const [creditLoading, setCreditLoading] = useState<Record<string, boolean>>({});
  const [sortKey, setSortKey] = useState<QoderSortKey>(() => {
    try {
      const stored = localStorage.getItem("wb-switch.qoder.sort");
      return QODER_SORT_OPTIONS.find((option) => option.value === stored)?.value ?? "createdAt";
    } catch { return "createdAt"; }
  });
  const onCredit = useCallback<CreditObserver>((id, credit) => setCredits((old) => ({ ...old, [id]: credit })), []);
  const onLoading = useCallback<LoadingObserver>((id, loading) => setCreditLoading((old) => old[id] === loading ? old : { ...old, [id]: loading }), []);

  useEffect(() => {
    if (api.isDemoMode()) { void refresh(); return; }
    let disposed = false;
    let retryTimer: number | undefined;
    async function reconcile() {
      let deferred = false;
      if (!autoInFlight.current) {
        const request = api.autoCaptureQoderAccounts();
        autoInFlight.current = request;
        void request.finally(() => { if (autoInFlight.current === request) autoInFlight.current = null; }).catch(() => {});
      }
      try {
        const result = await autoInFlight.current;
        if (!disposed) {
          if (result.deferred) {
            deferred = true;
            window.clearTimeout(retryTimer);
            retryTimer = window.setTimeout(() => void reconcile(), 3000);
            return;
          }
          setAutoError(null);
          setAutoFailures(result.failedTargets);
          setAuthorizationRequired(result.authorizationRequired);
        }
      } catch (error) {
        if (!disposed) setAutoError(api.asError(error));
      } finally {
        if (!disposed && !deferred) { setAutoReady(true); void refresh(); }
      }
    }
    void reconcile();
    const timer = window.setInterval(() => { if (document.visibilityState === "visible") void reconcile(); }, 60_000);
    const onFocus = () => { void reconcile(); };
    window.addEventListener("focus", onFocus);
    return () => { disposed = true; window.clearInterval(timer); window.clearTimeout(retryTimer); window.removeEventListener("focus", onFocus); };
  }, [refresh, autoAttempt]);

  async function run<T>(key: string, action: () => Promise<T>, onOk: (v: T) => void, failTitle: string) {
    setBusyKey(key);
    try {
      onOk(await action());
    } catch (e) {
      toast.error(failTitle, { description: api.asError(e) });
    } finally {
      setBusyKey(null);
      void refresh();
    }
  }

  function capture(target: QoderTarget) {
    void run(`capture:${target}`, () => api.captureQoderAccount(target), (b) => {
      setAuthorizationRequired((items) => items.filter((item) => item !== target));
      setAutoFailures((items) => items.filter((item) => item !== target));
      toast.success("已识别当前账号", { description: b.label });
    }, "账号识别失败");
  }

  function doSwitch(bundle: QoderBundle) {
    void run(bundle.id, () => api.switchQoderAccount(bundle.id), (r) => {
      if (r.unchanged) {
        toast.info(r.message ?? "已是该账号");
        return;
      }
      const notes = [r.autoSavedId ? "原登录态已自动保存为账号" : null, r.relaunched ? `已重新打开 ${TARGET_LABELS[bundle.target]}` : null, ...(r.warnings ?? [])].filter(Boolean);
      toast.success(`已切换到 ${bundle.label}`, { description: notes.join("；") || undefined });
    }, "切换失败");
  }

  function commitRename(group: QoderAccountGroup) {
    const label = draft.trim();
    setEditingId(null);
    if (!label || label === group.label) return;
    void run(group.key, async () => {
      for (const bundle of group.bundles) await api.renameQoderAccount(bundle.id, label);
    }, () => {}, "重命名失败");
  }

  function targetStatus(target: QoderTarget) {
    if (!status) return undefined;
    return { desktop: status.desktop, cli: status.cli, global_desktop: status.globalDesktop, global_cli: status.globalCli }[target];
  }

  /** 目标运行中才需要确认（会被关闭）；未运行直接切。 */
  function requestSwitch(bundle: QoderBundle) {
    if (targetStatus(bundle.target)?.running) setPending({ kind: "switch", bundle });
    else doSwitch(bundle);
  }

  const targets = qoderTargets(variant);
  const bundles = (status?.bundles ?? []).filter((bundle) => targets.includes(bundle.target));
  const groups = groupQoderAccounts(bundles);
  const orderedGroups = orderQoderAccounts(groups, sortKey, credits);
  const currentLogins = targets.map(targetStatus).filter((s): s is QoderTargetStatus => Boolean(autoReady && s?.loggedIn && !s.autoSuppressed && !s.currentId && !groups.some((g) => g.bundles.some((b) => b.target === s.target && b.identityKey && b.identityKey === s.currentIdentityKey))));
  const accountCount = groups.length + currentLogins.length;
  const busy = busyKey !== null;
  const refreshingCredits = [...groups.map((group) => group.key), ...currentLogins.map((s) => `live:${s.target}`)].some((key) => creditLoading[key]);

  async function claimRewards() {
    if (claimingRewards) return;
    setClaimingRewards(true);
    const progressToast = toast.loading("正在领取 Qoder 奖励…");
    const outcomes: string[] = [];
    let failed = 0;
    try {
      for (const group of groups) {
        const active = targets.find((target) => {
          const state = targetStatus(target);
          return group.bundles.some((bundle) => bundle.id === state?.currentId || (bundle.identityKey && bundle.identityKey === state?.currentIdentityKey));
        });
        try {
          const result = await api.claimQoderRewards(group.bundles.map((bundle) => bundle.id), active);
          if (!result.ok) failed++;
          outcomes.push(`${group.label}：${result.message}`);
        } catch (error) { failed++; outcomes.push(`${group.label}：${api.asError(error)}`); }
      }
      for (const state of currentLogins) {
        try {
          const result = await api.claimQoderRewards([], state.target);
          if (!result.ok) failed++;
          outcomes.push(`${TARGET_LABELS[state.target]}：${result.message}`);
        } catch (error) { failed++; outcomes.push(`${TARGET_LABELS[state.target]}：${api.asError(error)}`); }
      }
      (failed ? toast.warning : toast.success)("领奖处理完成", { description: outcomes.join("；"), duration: 10000 });
    } finally {
      toast.dismiss(progressToast);
      setCreditVersion((value) => value + 1);
      setClaimingRewards(false);
    }
  }

  return (
    <div className="mx-auto w-full max-w-[1180px] px-4 py-6 sm:px-8 sm:py-9">
      <header className="mb-6">
        <div className="flex flex-wrap items-start justify-between gap-4">
          <div className="min-w-0">
            <p className="mb-2 text-[11px] font-semibold tracking-[0.16em] text-primary">QODER / WORKSPACE</p>
            <h1 className="text-[28px] font-semibold tracking-tight">账号管理</h1>
            <p className="mt-2 text-sm leading-6 text-muted-foreground">
              管理国内版和国际版的桌面端及 CLI 账号，各端独立切换。
            </p>
            <Tabs className="mt-4 gap-0" value={variant} onValueChange={(value) => setVariant(normalizeVariant(value))}>
              <TabsList aria-label="Qoder 档位">
                <TabsTrigger value="cn">国内版</TabsTrigger>
                <TabsTrigger value="ai">国际版</TabsTrigger>
              </TabsList>
            </Tabs>
          </div>
          <div className="flex shrink-0 items-center gap-2.5 pt-1">
            {targets.map((target) => <HeaderBadge key={target} target={target} status={targetStatus(target)} />)}
          </div>
        </div>
      </header>

      <div className="relative mb-6 overflow-visible rounded-2xl border border-border bg-muted/30 px-5 py-5 shadow-[0_6px_20px_rgba(15,23,42,.025)]">
        <div className="pointer-events-none absolute inset-0 overflow-hidden rounded-2xl"><div className="absolute -right-12 -top-20 size-44 rounded-full border-[28px] border-slate-400/[0.035]" /></div>
        <div className="relative flex flex-wrap items-center gap-x-5 gap-y-4">
          <div className="min-w-[190px] flex-1">
            <h2 className="text-sm font-semibold text-foreground">添加与迁移账号</h2>
            <p className="mt-1 text-xs leading-5 text-muted-foreground">
              快速接入{variant === "ai" ? "国际版" : "新"}账号，或从已有环境恢复
            </p>
          </div>
          <div className="flex flex-wrap items-center gap-2.5">
            <DemoAction><Button className="h-10 bg-primary px-4 text-primary-foreground shadow-sm hover:bg-primary/90" disabled={busy} onClick={() => setLoginTarget(targets[0])}><LogIn />登录添加</Button></DemoAction>
          </div>
          <div className="flex items-center gap-1">
            <DemoAction><Button variant="ghost" size="sm" className="h-9 px-2.5" disabled={busy} onClick={() => setBackupMode("import")} title="从备份文件导入账号"><FileUp />导入备份</Button></DemoAction>
            <DemoAction><Button variant="ghost" size="sm" className="h-9 px-2.5" disabled={busy || !status?.bundles.length} onClick={() => setBackupMode("export")} title="导出账号备份"><FileDown />导出</Button></DemoAction>
          </div>
        </div>
      </div>

      {loadError && <p className="mb-4 text-sm text-destructive">读取 Qoder 状态失败：{loadError}</p>}
      {autoError && <div className="mb-4 flex items-center gap-2 text-sm text-destructive"><span>自动识别本机账号失败：{autoError}</span><Button variant="outline" size="sm" onClick={() => setAutoAttempt((value) => value + 1)}>重试</Button></div>}
      {autoFailures.some((target) => targets.includes(target)) && <div className="mb-4 flex items-center gap-2 text-sm text-destructive"><span>部分本机账号无法自动识别，请重试或重新登录。</span><Button variant="outline" size="sm" onClick={() => setAutoAttempt((value) => value + 1)}>重试识别</Button></div>}
      {authorizationRequired.filter((target) => targets.includes(target) && targetStatus(target)?.currentId && !targetStatus(target)?.autoSuppressed).map((target) => (
        <div key={target} className="mb-4 flex flex-wrap items-center gap-2 text-sm text-muted-foreground"><span>{TARGET_LABELS[target]} 需要系统授权才能识别账号或查询积分。</span><DemoAction><Button variant="outline" size="sm" disabled={busy} onClick={() => capture(target)}>{busyKey === `capture:${target}` && <Loader2 className="animate-spin" />}授权识别</Button></DemoAction></div>
      ))}

      <section className="mt-7 min-w-0" aria-labelledby="qoder-accounts-title">
        <div className="mb-4 flex flex-wrap items-center justify-between gap-3">
          <div className="flex items-center gap-2">
            <h2 id="qoder-accounts-title" className="text-base font-semibold tracking-tight">账号</h2>
            <Badge
              variant="secondary"
              className="h-6 min-w-6 rounded-full border-0 px-1.5 text-[11px] tabular-nums text-muted-foreground shadow-none"
              aria-label={`${accountCount} 个${variantLabel(variant)} Qoder 账号`}
            >
              {accountCount}
            </Badge>
          </div>
          <div className="ml-auto flex max-w-full flex-wrap items-center justify-end gap-1">
            <Select value={sortKey} onValueChange={(value) => { const next = value as QoderSortKey; setSortKey(next); try { localStorage.setItem("wb-switch.qoder.sort", next); } catch { /* Sorting still works for this session. */ } }}>
              <SelectTrigger size="sm" className="w-auto min-w-32 gap-1.5 border-0 bg-transparent px-2 text-xs text-muted-foreground shadow-none hover:bg-accent hover:text-foreground" aria-label="账号排序方式"><SlidersHorizontal className="size-4" aria-hidden="true" /><SelectValue /></SelectTrigger>
              <SelectContent>{QODER_SORT_OPTIONS.map((option) => <SelectItem key={option.value} value={option.value}>{option.label}</SelectItem>)}</SelectContent>
            </Select>
            {onToggleCompact && <Button variant="ghost" size="icon" className={cn("size-9 rounded-lg", compact && "bg-accent text-accent-foreground")} onClick={onToggleCompact} aria-label={compact ? "切换为宽松模式" : "切换为紧凑模式"} title={compact ? "切换为宽松模式" : "切换为紧凑模式"}>{compact ? <Rows3 /> : <Columns3 />}</Button>}
            <DemoAction><Button variant="ghost" size="sm" className="h-9 rounded-lg text-xs" disabled={refreshingCredits || accountCount === 0} onClick={() => setCreditVersion((value) => value + 1)} aria-label="刷新积分" title="查询当前档位所有账号的积分"><RefreshCw className={refreshingCredits ? "animate-spin" : undefined} />刷新积分</Button></DemoAction>
            <DemoAction><Button variant="outline" size="sm" className="h-9 rounded-lg text-xs" disabled={claimingRewards || accountCount === 0} onClick={() => void claimRewards()} aria-label="领取积分" title="领取可用奖励，完成后刷新积分">{claimingRewards ? <Loader2 className="animate-spin" /> : <Gift />}领取积分</Button></DemoAction>
          </div>
        </div>
        {(!autoReady || (!status && !loadError)) ? (
          <div className="flex items-center gap-2 py-16 text-sm text-muted-foreground">
            <Loader2 className="animate-spin" />
            加载账号…
          </div>
        ) : accountCount === 0 ? (
          <div className="rounded-xl border border-dashed px-4 py-16 text-center text-sm text-muted-foreground">
            <p>暂无{variantLabel(variant)} Qoder 账号。</p>
            <p className="mt-2 text-xs leading-5">
              点击上方「登录添加」；本机已登录账号会自动识别。
            </p>
          </div>
        ) : (
          <div className={cn("grid min-w-0 gap-5", compact ? "grid-cols-[repeat(auto-fit,minmax(min(100%,300px),1fr))]" : "grid-cols-[repeat(auto-fit,minmax(min(100%,340px),1fr))]")}>
            {currentLogins.map((s) => (
              <CurrentLoginCard key={s.target} status={s} compact={compact} busy={busy} saving={busyKey === `capture:${s.target}`} onSave={() => capture(s.target)} needsAuthorization={authorizationRequired.includes(s.target)} refreshVersion={creditVersion} onCredit={onCredit} onLoading={onLoading} />
            ))}
            {orderedGroups.map((group) => (
              <QoderAccountCard
                key={group.key}
                group={group}
                targets={targets}
                targetStatus={targetStatus}
                compact={compact}
                busy={busy}
                busyKey={busyKey}
                editing={editingId === group.key}
                draft={draft}
                onDraftChange={setDraft}
                onStartRename={() => {
                  setDraft(group.label);
                  setEditingId(group.key);
                }}
                onCommitRename={() => commitRename(group)}
                onCancelRename={() => setEditingId(null)}
                onSwitch={requestSwitch}
                onDelete={() => setPending({ kind: "delete", group })}
                refreshVersion={creditVersion}
                onCredit={onCredit}
                onLoading={onLoading}
              />
            ))}
          </div>
        )}
        <p className="mt-5 text-xs leading-5 text-muted-foreground">
          Qoder 的会话历史不按账号隔离，切换账号后仍可查看本机历史。
        </p>
      </section>

      <QoderBackupDialog mode={backupMode} onClose={() => setBackupMode(null)} groups={groupQoderAccounts(status?.bundles ?? [])} onImported={(result) => {
        const summary = `已导入 ${result.imported} 个账号${result.skipped ? `，${result.skipped} 个账号已存在` : ""}`;
        if (result.warnings.length) toast.warning(summary, { description: result.warnings.join("；"), duration: 15000 });
        else toast.success(summary);
        void refresh();
      }} />

      <AlertDialog open={pending !== null} onOpenChange={(open) => !open && setPending(null)}>
        <AlertDialogContent>
          <AlertDialogHeader>
            <AlertDialogTitle>
              {pending?.kind === "delete" ? `删除账号「${pending.group.label}」？` : `切换到「${pending?.kind === "switch" ? pending.bundle.label : ""}」？`}
            </AlertDialogTitle>
            <AlertDialogDescription>
              {pending?.kind === "delete"
                ? "删除本工具保存的桌面端和 CLI 凭据副本，不影响 Qoder 当前登录。删除后需重新添加该账号才能再切回。"
                : pending?.kind === "switch" && isCli(pending.bundle.target)
                  ? `${TARGET_LABELS[pending.bundle.target]} 正在运行，切换会关闭它，当前 CLI 会话会中断且不会自动重开。`
                  : `${pending?.kind === "switch" ? TARGET_LABELS[pending.bundle.target] : "Qoder"} 正在运行，切换时会先关闭它，写入完成后自动重新打开。`}
            </AlertDialogDescription>
          </AlertDialogHeader>
          <AlertDialogFooter>
            <AlertDialogCancel>取消</AlertDialogCancel>
            <AlertDialogAction
              onClick={() => {
                if (!pending) return;
                const item = pending;
                setPending(null);
                if (item.kind === "delete") {
                  void run(item.group.key, async () => {
                    for (const bundle of item.group.bundles) await api.deleteQoderAccount(bundle.id);
                  }, () => toast.success("账号已删除"), "删除失败");
                } else {
                  doSwitch(item.bundle);
                }
              }}
            >
              {pending?.kind === "delete" ? "删除" : "关闭并切换"}
            </AlertDialogAction>
          </AlertDialogFooter>
        </AlertDialogContent>
      </AlertDialog>
      {loginTarget && <QoderLoginDialog open onOpenChange={(open) => { if (!open) setLoginTarget(null); }} target={loginTarget} label={`Qoder ${variantLabel(variant)}`} />}
    </div>
  );
}
