import { useCallback, useEffect, useRef, useState } from "react";
import { ArrowRight, Check, Clock3, Columns3, Ellipsis, ExternalLink, FileDown, FileUp, Gift, Info, Loader2, LogIn, Pencil, RefreshCw, Rows3, SlidersHorizontal, Sparkles, Trash2 } from "lucide-react";
import { toast } from "sonner";
import { TraeBackupDialog } from "@/components/trae-backup-dialog";
import { DemoAction } from "@/components/demo-action";
import { TraeMark } from "@/components/product-marks";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Tabs, TabsList, TabsTrigger } from "@/components/ui/tabs";
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from "@/components/ui/select";
import { Dialog, DialogContent, DialogDescription, DialogFooter, DialogHeader, DialogTitle } from "@/components/ui/dialog";
import { AlertDialog, AlertDialogAction, AlertDialogCancel, AlertDialogContent, AlertDialogDescription, AlertDialogFooter, AlertDialogHeader, AlertDialogTitle } from "@/components/ui/alert-dialog";
import { DropdownMenu, DropdownMenuContent, DropdownMenuItem, DropdownMenuSeparator, DropdownMenuTrigger } from "@/components/ui/dropdown-menu";
import * as api from "@/lib/api";
import { avatarTone } from "@/lib/avatar-tone";
import { cn } from "@/lib/utils";
import { normalizeVariant, variantLabel } from "@/lib/variant";
import { groupTraeAccounts, orderTraeAccounts, TRAE_SORT_OPTIONS, type TraeAccountGroup, type TraeSortKey } from "@/lib/trae-accounts";
import { useVisibleInterval } from "@/lib/use-visible-interval";
import { useTraeStatus } from "@/stores/trae-status";
import { useTraeView } from "@/stores/trae-view";

const CARD_CLASS = "flex min-w-0 flex-col overflow-hidden rounded-2xl border border-border bg-card shadow-[0_1px_2px_rgba(15,23,42,.025),0_10px_28px_rgba(15,23,42,.035)] transition-shadow hover:shadow-[0_2px_4px_rgba(15,23,42,.04),0_14px_34px_rgba(15,23,42,.055)]";
type Pending = { kind: "switch"; account: api.TraeAccount } | { kind: "delete"; group: TraeAccountGroup };
const format = (value: number) => new Intl.NumberFormat("zh-CN", { maximumFractionDigits: 2 }).format(value);
const formatTime = (seconds?: number | null) => seconds ? new Date(seconds * 1000).toLocaleString("zh-CN", { hour12: false }) : "—";
const clientLabel = (_client: api.TraeClient) => "TRAE 桌面端";

function formatPackExpiry(seconds?: number | null) {
  if (!seconds) return "长期有效";
  const date = new Date(seconds * 1000);
  if (Number.isNaN(date.getTime())) return "长期有效";
  return `${String(date.getMonth() + 1).padStart(2, "0")}/${String(date.getDate()).padStart(2, "0")} ${String(date.getHours()).padStart(2, "0")}:${String(date.getMinutes()).padStart(2, "0")}`;
}
function formatPackDateTime(seconds?: number | null) {
  if (!seconds) return "—";
  const date = new Date(seconds * 1000);
  if (Number.isNaN(date.getTime())) return "—";
  return date.toLocaleString("zh-CN", { year: "numeric", month: "2-digit", day: "2-digit", hour: "2-digit", minute: "2-digit", hour12: false });
}
function visiblePacks(credit?: api.TraeCredit | null): api.TraeCreditPack[] {
  const known = credit?.available === true && (credit.unlimited || credit.remaining != null);
  const packs = (credit?.packs ?? []).filter(pack => pack.remaining > 0);
  const rows = packs.length || !known || credit?.unlimited || credit?.remaining == null || credit.remaining <= 0 ? packs : [{
    name: credit.unit === "usd" ? "美元额度" : "积分包",
    remaining: credit.remaining,
    total: credit.total ?? credit.remaining,
    used: credit.used ?? Math.max(0, (credit.total ?? credit.remaining) - credit.remaining),
    expiresAt: credit.expiresAt,
  }];
  return [...rows].sort((a, b) => (a.expiresAt ?? Number.POSITIVE_INFINITY) - (b.expiresAt ?? Number.POSITIVE_INFINITY));
}
function PackRow({ pack, compact, unit, placeholder }: { pack?: api.TraeCreditPack; compact: boolean; unit: string; placeholder?: string }) {
  if (!pack) return <div className="min-w-0"><div className={cn("grid min-w-0 grid-cols-[auto_minmax(0,1fr)_auto] items-center gap-3 text-muted-foreground", compact ? "text-[11px]" : "text-xs")}><span className={compact ? "py-0.5" : "py-1"}>{placeholder}</span><span className="invisible">{"\u00a0"}</span><span className="invisible">{"\u00a0"}</span></div><div className={cn("h-1", compact ? "mt-1" : "mt-1.5")} aria-hidden /></div>;
  const soon = pack.expiresAt != null && pack.expiresAt * 1000 > Date.now() && pack.expiresAt * 1000 - Date.now() <= 7 * 86400000;
  const ratio = pack.total > 0 ? Math.min(100, Math.max(0, pack.remaining / pack.total * 100)) : 0;
  const amount = `${format(pack.remaining)} ${unit}`;
  const expiry = formatPackExpiry(pack.expiresAt);
  return <div className="min-w-0" title={`${pack.name} · 剩余 ${format(pack.remaining)} / ${format(pack.total)} · ${pack.expiresAt ? `${expiry} 到期` : expiry}`}>
    <div className={cn("grid min-w-0 grid-cols-[auto_minmax(0,1fr)_auto] items-center gap-3", compact ? "text-[11px]" : "text-xs")}>
      <span className={cn("rounded-lg bg-muted/80 font-medium tabular-nums text-foreground", compact ? "px-1.5 py-0.5" : "px-2 py-1")}>{amount}</span>
      <span className="truncate text-muted-foreground">{pack.name}</span>
      <span className={cn("whitespace-nowrap tabular-nums", soon ? "text-orange-600" : "text-muted-foreground")}>{expiry}</span>
    </div>
    <div className={cn("h-1 overflow-hidden rounded-full bg-muted", compact ? "mt-1" : "mt-1.5")} aria-hidden><div className={cn("h-full rounded-full", soon ? "bg-orange-500" : "bg-primary")} style={{ width: `${ratio}%` }} /></div>
  </div>;
}
function CreditPanel({ credit, loading, error, compact, accountName }: { credit?: api.TraeCredit | null; loading: boolean; error?: string; compact: boolean; accountName: string }) {
  const [packsOpen, setPacksOpen] = useState(false);
  const known = credit?.available === true && (credit.unlimited || credit.remaining != null);
  const number = credit?.unlimited ? "不限量" : known && credit?.remaining != null ? `${credit.unit === "usd" ? "$" : ""}${format(credit.remaining)}` : "—";
  const packs = visiblePacks(credit);
  const unit = credit?.unit === "usd" ? "美元" : "积分";
  const nearest = packs.find(pack => pack.expiresAt)?.expiresAt;
  const soonAmount = packs.filter(pack => pack.expiresAt != null && pack.expiresAt * 1000 > Date.now() && pack.expiresAt * 1000 - Date.now() <= 7 * 86400000).reduce((sum, pack) => sum + pack.remaining, 0);
  return <div className="min-w-0">
    <div className="flex flex-wrap items-baseline gap-x-3 gap-y-1">
      <span className="flex items-center gap-1.5"><Sparkles className="size-4 shrink-0 stroke-[1.75] text-muted-foreground" aria-hidden />
        <strong className={cn("font-semibold leading-none tabular-nums tracking-[-0.025em] text-foreground", compact ? "text-[20px]" : "text-[22px]")} style={{ fontFamily: '"Bricolage Grotesque Variable", "SF Pro Display", ui-sans-serif, sans-serif' }}>{number}</strong>
      </span>
      <span className={compact ? "text-[11px]" : "text-xs"}>{known ? credit?.unlimited ? "不限量额度" : `${packs.length} 个${unit === "美元" ? "额度包" : "积分包"}` : loading ? "查询中…" : credit ? "额度未知" : "待查询"}</span>
      <span className="ml-auto inline-flex items-center gap-1.5 text-[11px]" title={soonAmount > 0 ? `${format(soonAmount)} ${unit}将在 7 天内到期` : nearest ? `最近到期 ${formatPackExpiry(nearest)}` : known ? `当前${unit}长期有效` : undefined}><Clock3 className="size-3.5 shrink-0" />{loading ? "更新中…" : credit?.checkedAt ? `${new Date(credit.checkedAt * 1000).toLocaleTimeString("zh-CN", { hour: "2-digit", minute: "2-digit", hour12: false })} 更新` : "—"}</span>
    </div>
    {known && !credit?.unlimited && <div className={cn("flex items-center justify-between gap-2", compact ? "mt-3" : "mt-4")}><span className="text-[11px] font-medium text-muted-foreground">近期到期</span>{packs.length > 2 && <button type="button" className="inline-flex shrink-0 cursor-pointer items-center gap-1 text-[11px] font-medium text-primary transition-colors hover:text-primary/80 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-primary/30" onClick={() => setPacksOpen(true)}>查看全部积分包<ArrowRight className="size-3.5" /></button>}</div>}
    {known && !credit?.unlimited && <div className={cn(compact ? "mt-3 space-y-2" : "mt-4 space-y-2.5")}>{packs.length === 0 ? <div className="py-1 text-[11px] text-muted-foreground">暂无可用积分</div> : [0, 1].map(index => {
      const pack = packs[index];
      return <PackRow key={pack ? `${pack.name}-${pack.expiresAt ?? "none"}-${index}` : "empty-pack"} pack={pack} compact={compact} unit={unit} placeholder="暂无其他积分包" />;
    })}</div>}
    {error ? <p className="mt-3 text-xs text-destructive">{error}</p> : !known && !loading ? <p className="mt-3 text-[11px]">{credit ? "官方暂未提供可识别的额度，未知余额不会按零计算。" : "点击刷新积分，查询官方可用额度。"}</p> : null}
    <Dialog open={packsOpen} onOpenChange={setPacksOpen}><DialogContent className="sm:max-w-md"><DialogHeader><DialogTitle>全部积分包</DialogTitle><DialogDescription>{accountName} · 共 {packs.length} 个{unit === "美元" ? "额度包" : "积分包"}</DialogDescription></DialogHeader>
      {packs.length === 0 ? <div className="px-1 py-6 text-center text-sm text-muted-foreground">当前没有可展示的积分包。</div> : <div className="max-h-[60vh] min-w-0 overflow-y-auto divide-y divide-border/60">{packs.map((pack, index) => {
        const ratio = pack.total > 0 ? Math.min(100, Math.max(0, pack.remaining / pack.total * 100)) : 0;
        const soon = pack.expiresAt != null && pack.expiresAt * 1000 > Date.now() && pack.expiresAt * 1000 - Date.now() <= 7 * 86400000;
        return <div key={`${pack.name}-${pack.expiresAt ?? "none"}-${index}`} className="min-w-0 py-3 first:pt-0 last:pb-0"><div className="flex min-w-0 items-start justify-between gap-3"><div className="min-w-0"><div className="truncate text-sm font-medium">{pack.name}</div><div className="mt-1 text-[11px] text-muted-foreground">{pack.expiresAt ? `到期 ${formatPackDateTime(pack.expiresAt)}` : "长期有效"}</div></div><div className="shrink-0 text-right text-xs"><div className="font-medium">{format(pack.remaining)} / {format(pack.total)}</div><div className="mt-1 text-[11px] text-muted-foreground">已用 {format(pack.used)}</div></div></div><div className="mt-2 h-1.5 overflow-hidden rounded-full bg-muted" aria-hidden><div className={cn("h-full rounded-full", soon ? "bg-orange-500/80" : "bg-primary/75")} style={{ width: `${ratio}%` }} /></div></div>;
      })}</div>}
    </DialogContent></Dialog>
  </div>;
}

export default function TraePage() {
  const { variant, setVariant } = useTraeView();
  const { status, error: loadError, refresh } = useTraeStatus();
  const [compact, setCompact] = useState(() => { try { return localStorage.getItem("wb-switch.compact") !== "0"; } catch { return true; } });
  const [sortKey, setSortKey] = useState<TraeSortKey>(() => { try { const value = localStorage.getItem("wb-switch.trae.sort"); return TRAE_SORT_OPTIONS.some(o => o.value === value) ? value as TraeSortKey : "active"; } catch { return "active"; } });
  const [busyKey, setBusyKey] = useState<string | null>(null);
  const mutationRunning = useRef(false);
  const [pending, setPending] = useState<Pending | null>(null);
  const [loginOpen, setLoginOpen] = useState(false);
  const [backupMode, setBackupMode] = useState<"import" | "export" | null>(null);
  const [editing, setEditing] = useState<string | null>(null);
  const [draft, setDraft] = useState("");
  const cancelRename = useRef(false);
  const [info, setInfo] = useState<TraeAccountGroup | null>(null);
  const [creditOverrides, setCreditOverrides] = useState<Record<string, api.TraeCredit>>({});
  const [creditErrors, setCreditErrors] = useState<Record<string, string>>({});
  const [creditLoading, setCreditLoading] = useState<Record<string, boolean>>({});
  const [claiming, setClaiming] = useState(false);
  const querying = useRef(new Set<string>());
  const busy = busyKey !== null;
  const clients = (status?.clients ?? []).filter(c => c.variant === variant);
  const accounts = (status?.accounts ?? []).filter(a => a.variant === variant);
  const groups = orderTraeAccounts(groupTraeAccounts(accounts), sortKey, clients);
  const targets: api.TraeClient[] = ["desktop"];
  const current = clients.filter(c => c.configured && !c.activeAccountId);
  const statusFor = (client: api.TraeClient) => clients.find(c => c.client === client);

  useVisibleInterval(() => { void refresh(); }, 60_000);
  useEffect(() => {
    const onFocus = () => { void refresh(); };
    window.addEventListener("focus", onFocus);
    return () => window.removeEventListener("focus", onFocus);
  }, [refresh]);

  const queryCredits = useCallback(async (items: api.TraeAccount[]) => {
    // Each account has one displayed balance.
    const representatives = groupTraeAccounts(items).flatMap(group => group.accounts.filter(a => !a.needsLogin).sort((a, b) => b.capturedAt - a.capturedAt).slice(0, 1));
    for (const account of representatives) {
      if (account.needsLogin || querying.current.has(account.id)) continue;
      querying.current.add(account.id);
      setCreditLoading(values => ({ ...values, [account.id]: true }));
      try {
        let credit: api.TraeCredit;
        try {
          credit = await api.traeAction("credits", { accountId: account.id }) as api.TraeCredit;
        } catch (error) {
          const message = api.asError(error);
          // 和状态刷新抢锁时先等一次，不要把瞬时失败留在卡片上。
          if (!message.includes("正在更新")) throw error;
          await new Promise(resolve => window.setTimeout(resolve, 400));
          credit = await api.traeAction("credits", { accountId: account.id }) as api.TraeCredit;
        }
        setCreditOverrides(values => ({ ...values, [account.id]: credit }));
        setCreditErrors(values => ({ ...values, [account.id]: "" }));
      } catch (error) { setCreditErrors(values => ({ ...values, [account.id]: api.asError(error) })); }
      finally { querying.current.delete(account.id); setCreditLoading(values => ({ ...values, [account.id]: false })); }
    }
  }, []);
  const accountKey = accounts.map(a => a.id).join("|");
  const accountsRef = useRef(accounts);
  accountsRef.current = accounts;
  useEffect(() => {
    if (!api.isDemoMode()) void queryCredits(accountsRef.current.filter(a => !a.credits || Date.now() / 1000 - a.credits.checkedAt > 300));
  }, [accountKey, variant, queryCredits]);

  async function run(key: string, action: () => Promise<void>, message: string) {
    if (mutationRunning.current) return;
    mutationRunning.current = true; setBusyKey(key);
    try { await action(); toast.success(message); }
    catch (error) { toast.error(api.asError(error)); }
    finally { await refresh(true); mutationRunning.current = false; setBusyKey(null); }
  }
  async function claimCheckins() {
    if (claiming || variant !== "cn") return;
    setClaiming(true);
    const outcomes: string[] = [];
    let failed = 0;
    try {
      for (const account of accountsRef.current.filter(item => item.variant === "cn" && !item.needsLogin)) {
        try {
          const result = await api.traeAction("checkin", { accountId: account.id }) as { ok?: boolean; message?: string; credits?: number | null };
          if (!result.ok) failed++;
          const gain = typeof result.credits === "number" && result.credits > 0 ? `（+${result.credits} 积分）` : "";
          outcomes.push(`${account.name}：${result.message || "已处理"}${gain}`);
        } catch (error) { failed++; outcomes.push(`${account.name}：${api.asError(error)}`); }
      }
      if (!outcomes.length) { toast.warning("没有可签到的国内版账号"); return; }
      (failed ? toast.warning : toast.success)("签到处理完成", { description: outcomes.join("；"), duration: 10000 });
      await queryCredits(accountsRef.current.filter(item => item.variant === "cn"));
    } finally { setClaiming(false); }
  }
  function capture(client: api.TraeClient) {
    void run(`capture:${client}`, async () => { await api.traeAction("capture", { variant, client }); setLoginOpen(false); }, "当前登录账号已添加");
  }
  function rename(group: TraeAccountGroup) {
    if (cancelRename.current) { cancelRename.current = false; return; }
    const name = draft.trim();
    if (!name || name === group.name) { setEditing(null); return; }
    void run(`rename:${group.key}`, async () => { for (const account of group.accounts) await api.traeAction("rename", { accountId: account.id, name }); setEditing(null); }, "账号名称已更新");
  }
  function toggleCompact() {
    setCompact(value => { try { localStorage.setItem("wb-switch.compact", value ? "0" : "1"); } catch { /* 本次选择有效。 */ } return !value; });
  }
  function groupCredit(group: TraeAccountGroup) {
    return group.accounts.flatMap(a => [creditOverrides[a.id], a.credits]).filter((c): c is api.TraeCredit => !!c).sort((a, b) => b.checkedAt - a.checkedAt)[0];
  }
  function switchControls(group: TraeAccountGroup) {
    return <span className="inline-flex items-center gap-1.5">{targets.map(client => {
      const account = group.targets[client];
      const active = account && statusFor(client)?.activeAccountId === account.id;
      if (active) return <span key={client} className="relative inline-flex size-7 shrink-0 items-center justify-center rounded-lg border border-primary/25 bg-primary/10" aria-label={`${clientLabel(client)} 当前账号`} title={`${clientLabel(client)} 当前账号`}>
        <TraeMark size={15} global={variant === "ai"} /><span className="absolute -right-1 -top-1 flex size-3.5 items-center justify-center rounded-full bg-primary text-primary-foreground"><Check className="size-2.5" strokeWidth={3} /></span>
      </span>;
      return <DemoAction key={client}><Button variant="outline" size="icon" className="size-7 rounded-lg" disabled={busy || !account || account.needsLogin} onClick={() => account && setPending({ kind: "switch", account })} aria-label={`切换 ${clientLabel(client)}`} title={!account ? `${clientLabel(client)} 尚未添加此账号` : account.needsLogin ? "登录已过期，请重新登录添加" : `将 ${clientLabel(client)} 切换到此账号`}>
        {busyKey === account?.id ? <Loader2 className="size-4 animate-spin" /> : <TraeMark size={15} global={variant === "ai"} />}
      </Button></DemoAction>;
    })}</span>;
  }

  return <div className="mx-auto w-full max-w-[1180px] px-4 py-6 sm:px-8 sm:py-9">
    <header className="mb-6"><div className="flex flex-wrap items-start justify-between gap-4">
      <div className="min-w-0"><p className="mb-2 text-[11px] font-semibold tracking-[0.16em] text-primary">TRAE / WORKSPACE</p><h1 className="text-[28px] font-semibold tracking-tight">账号管理</h1><p className="mt-2 text-sm leading-6 text-muted-foreground">管理国内版和国际版的 TRAE 账号、积分与登录状态。</p>
        <Tabs className="mt-4 gap-0" value={variant} onValueChange={value => setVariant(normalizeVariant(value))}><TabsList aria-label="TRAE 档位"><TabsTrigger value="cn">国内版</TabsTrigger><TabsTrigger value="ai">国际版</TabsTrigger></TabsList></Tabs>
      </div>
      <div className="flex shrink-0 items-center gap-2.5 pt-1">{targets.map(client => {
        const target = statusFor(client);
        const state = loadError ? "状态未知" : target?.running ? "运行中" : target?.configured ? "已登录 · 未运行" : target?.detected ? "未登录" : "未检测到登录";
        return <button key={client} type="button" className="inline-flex rounded-[22%] bg-muted-foreground/30 p-[2px]" aria-label={`${clientLabel(client)}：${state}`} title={`${clientLabel(client)}：${state}，点击刷新状态`} onClick={() => void refresh()}>
          <span className={cn("inline-flex rounded-[20%] p-[2px]", target?.running && "bg-primary")}><TraeMark size={28} global={variant === "ai"} /></span>
        </button>;
      })}</div>
    </div></header>

    <div className="relative mb-6 overflow-visible rounded-2xl border border-border bg-muted/30 px-5 py-5 shadow-[0_6px_20px_rgba(15,23,42,.025)]">
      <div className="pointer-events-none absolute inset-0 overflow-hidden rounded-2xl"><div className="absolute -right-12 -top-20 size-44 rounded-full border-[28px] border-slate-400/[0.035]" /></div>
      <div className="relative flex flex-wrap items-center gap-x-5 gap-y-4">
        <div className="min-w-[190px] flex-1"><h2 className="text-sm font-semibold text-foreground">添加与迁移账号</h2><p className="mt-1 text-xs leading-5 text-muted-foreground">在官方客户端完成登录后，识别并添加到账号库</p></div>
        <DemoAction><Button className="h-10 bg-primary px-4 text-primary-foreground shadow-sm hover:bg-primary/90" disabled={busy} onClick={() => { setLoginOpen(true); }}><LogIn />登录添加</Button></DemoAction>
        <DemoAction><Button variant="ghost" size="sm" className="h-9 px-2.5" disabled={busy} onClick={() => capture("desktop")} title="识别并保存官方客户端当前登录的账号"><RefreshCw className={busyKey === "capture:desktop" ? "animate-spin" : undefined} />识别本机账号</Button></DemoAction>
        <div className="flex items-center gap-1"><DemoAction><Button variant="ghost" size="sm" className="h-9 px-2.5" disabled={busy} onClick={() => setBackupMode("import")}><FileUp />导入备份</Button></DemoAction><DemoAction><Button variant="ghost" size="sm" className="h-9 px-2.5" disabled={busy || !(status?.accounts ?? []).length} onClick={() => setBackupMode("export")}><FileDown />导出</Button></DemoAction></div>
      </div>
    </div>

    <TraeBackupDialog mode={backupMode} onClose={() => setBackupMode(null)} />
    {loadError && <div className="mb-4 flex flex-wrap items-center gap-2 text-sm text-destructive"><span>读取 TRAE 状态失败：{loadError}</span><Button variant="outline" size="sm" onClick={() => void refresh()}>重试</Button></div>}
    {clients.filter(c => c.recognitionError).map(c => <p key={c.client} className="mb-4 text-sm text-destructive">{clientLabel(c.client)}：{c.recognitionError}</p>)}

    <section className="mt-7 min-w-0" aria-labelledby="trae-accounts-title">
      <div className="mb-4 flex flex-wrap items-center justify-between gap-3">
        <div className="flex items-center gap-2"><h2 id="trae-accounts-title" className="text-base font-semibold tracking-tight">账号</h2><Badge variant="secondary" className="h-6 min-w-6 rounded-full border-0 px-1.5 text-[11px] tabular-nums text-muted-foreground shadow-none" aria-label={`${groups.length} 个${variantLabel(variant)} TRAE 账号`}>{groups.length}</Badge></div>
        <div className="ml-auto flex max-w-full flex-wrap items-center justify-end gap-1">
          <Select value={sortKey} onValueChange={value => { setSortKey(value as TraeSortKey); try { localStorage.setItem("wb-switch.trae.sort", value); } catch { /* 本次选择有效。 */ } }}><SelectTrigger size="sm" className="w-auto min-w-32 gap-1.5 border-0 bg-transparent px-2 text-xs text-muted-foreground shadow-none hover:bg-accent hover:text-foreground" aria-label="账号排序方式"><SlidersHorizontal className="size-4" /><SelectValue /></SelectTrigger><SelectContent>{TRAE_SORT_OPTIONS.map(option => <SelectItem key={option.value} value={option.value}>{option.label}</SelectItem>)}</SelectContent></Select>
          <Button variant="ghost" size="icon" className={cn("size-9 rounded-lg", compact && "bg-accent text-accent-foreground")} onClick={toggleCompact} aria-label={compact ? "切换为宽松模式" : "切换为紧凑模式"} title={compact ? "切换为宽松模式" : "切换为紧凑模式"}>{compact ? <Rows3 /> : <Columns3 />}</Button>
          <DemoAction><Button variant="ghost" size="sm" className="h-9 rounded-lg text-xs" disabled={busy || accounts.length === 0 || accounts.some(a => creditLoading[a.id])} onClick={() => void queryCredits(accounts)} aria-label="刷新积分" title="查询当前档位所有账号的官方额度"><RefreshCw className={accounts.some(a => creditLoading[a.id]) ? "animate-spin" : undefined} />刷新积分</Button></DemoAction>
          <DemoAction><Button variant="outline" size="sm" className="h-9 rounded-lg text-xs" disabled={busy || claiming || variant !== "cn" || accounts.length === 0} onClick={() => void claimCheckins()} aria-label="领取积分" title={variant === "cn" ? "领取国内版每日签到积分。同一官方设备每天只能签一个账号" : "国际版签到暂未接入"}>{claiming ? <Loader2 className="animate-spin" /> : <Gift />}领取积分</Button></DemoAction>
        </div>
      </div>
      {!status && !loadError ? <div className="flex items-center gap-2 py-16 text-sm text-muted-foreground"><Loader2 className="size-4 animate-spin" />加载账号…</div>
        : groups.length === 0 && current.length === 0 ? <div className="rounded-2xl border border-dashed border-border bg-muted/15 px-6 py-14 text-center text-sm text-muted-foreground"><p>暂无{variantLabel(variant)} TRAE 账号。</p><p className="mt-2 text-xs">点击上方「登录添加」；已有本机登录可直接识别。</p></div>
        : <div className={cn("grid min-w-0 gap-5", compact ? "grid-cols-[repeat(auto-fit,minmax(min(100%,300px),1fr))]" : "grid-cols-[repeat(auto-fit,minmax(min(100%,340px),1fr))]")}>
          {current.map(c => <article key={`live:${c.client}`} className={CARD_CLASS}>
            <header className={cn("flex items-center gap-3 border-b border-border bg-primary/5", compact ? "min-h-[52px] px-3.5 py-1.5" : "min-h-[104px] px-5 py-3")}><TraeMark size={compact ? 24 : 40} global={variant === "ai"} /><div><h3 className="text-sm font-semibold">本机当前登录</h3><p className="text-xs text-muted-foreground">{clientLabel(c.client)} · 尚未添加</p></div></header>
            <div className={cn("flex-1 text-xs leading-5 text-muted-foreground", compact ? "px-3.5 pb-4 pt-3" : "px-5 pb-5 pt-4")}>已识别到官方客户端的登录，可直接添加后管理和切换。</div>
            <footer className="border-t px-3.5 py-2"><DemoAction><Button size="sm" variant="outline" disabled={busy} onClick={() => capture(c.client)}>{busyKey === `capture:${c.client}` ? <Loader2 className="animate-spin" /> : <LogIn />}添加当前账号</Button></DemoAction></footer>
          </article>)}
          {groups.map(group => {
            const active = group.accounts.some(a => statusFor(a.client)?.activeAccountId === a.id);
            const groupError = group.accounts.map(a => creditErrors[a.id]).find(Boolean);
            const needsLogin = group.accounts.every(a => a.needsLogin);
            return <article key={group.key} className={CARD_CLASS}>
              <header className={cn("relative flex items-center gap-2 border-b border-border", compact ? "min-h-[52px] px-3.5 py-1.5" : "min-h-[104px] px-5 py-3", active ? "bg-primary/5" : "bg-muted/30")}>
                <div className="pointer-events-none absolute inset-0 overflow-hidden"><div className={cn("absolute -right-10 -top-16 rounded-full blur-2xl", compact ? "size-20" : "size-24", active ? "bg-primary/15" : "bg-muted/30")} />{active && <div className="absolute right-5 top-[64%] -translate-y-1/2 rotate-[7deg] opacity-[0.075] saturate-50 grayscale-[10%]"><TraeMark size={compact ? 40 : 56} global={variant === "ai"} /></div>}</div>
                {!compact && <div className={cn("relative z-10 flex size-12 shrink-0 items-center justify-center rounded-full text-base font-semibold ring-4 ring-white/65", avatarTone(group.name))}>{group.name.charAt(0).toUpperCase()}</div>}
                <div className="relative z-10 min-w-0 flex-1">{editing === group.key ? <Input autoFocus aria-label="账号名称" maxLength={80} value={draft} onChange={e => setDraft(e.target.value)} onBlur={() => rename(group)} onKeyDown={e => { if (e.key === "Enter") rename(group); if (e.key === "Escape") { cancelRename.current = true; setEditing(null); } }} className="h-8" /> : <h3 className={cn("truncate font-semibold", compact ? "text-[13px]" : "text-sm")} title={group.name}>{group.name}</h3>}{!compact && <p className="truncate text-xs text-muted-foreground">{group.accounts.find(a => a.email)?.email ?? "TRAE 账号"}</p>}</div>
                {compact && <span className="relative z-10">{switchControls(group)}</span>}
                <DropdownMenu><DropdownMenuTrigger asChild><Button variant="ghost" size="icon" className="relative z-20 size-7 rounded-lg text-muted-foreground" aria-label={`管理账号 ${group.name}`} disabled={busy}><Ellipsis /></Button></DropdownMenuTrigger><DropdownMenuContent align="end" className="w-40">
                  {!api.isDemoMode() && <DropdownMenuItem disabled={group.accounts.some(a => creditLoading[a.id])} onSelect={() => void queryCredits(group.accounts)}><RefreshCw />刷新积分</DropdownMenuItem>}
                  <DropdownMenuItem onSelect={() => setInfo(group)}><Info />账号信息</DropdownMenuItem>
                  {!api.isDemoMode() && <><DropdownMenuItem onSelect={() => { cancelRename.current = false; setDraft(group.name); setEditing(group.key); }}><Pencil />重命名</DropdownMenuItem><DropdownMenuSeparator /><DropdownMenuItem className="text-destructive focus:bg-destructive/5 focus:text-destructive" onSelect={() => setPending({ kind: "delete", group })}><Trash2 />删除账号</DropdownMenuItem></>}
                </DropdownMenuContent></DropdownMenu>
              </header>
              <div className={cn("flex-1 text-xs leading-5 text-muted-foreground", compact ? "px-3.5 pb-4 pt-3" : "px-5 pb-5 pt-4")}><CreditPanel credit={groupCredit(group)} loading={group.accounts.some(a => creditLoading[a.id])} error={groupError} compact={compact} accountName={group.name} />{needsLogin && <div className="mt-3 flex items-center justify-between gap-2 text-amber-700 dark:text-amber-300"><span>登录已过期</span><DemoAction><Button size="sm" variant="ghost" disabled={busy} onClick={() => { setLoginOpen(true); }}>重新登录</Button></DemoAction></div>}</div>
              {!compact && <footer className="flex items-center gap-2 border-t border-border px-5 py-2.5">{switchControls(group)}{active && <span className="text-xs text-muted-foreground">当前使用</span>}</footer>}
            </article>;
          })}
        </div>}
    </section>
    <p className="mt-6 text-xs leading-6 text-muted-foreground">切换保留本机项目与历史，云端会话仍归原账号所有。</p>

    <Dialog open={loginOpen} onOpenChange={open => { if (!busy) setLoginOpen(open); }}><DialogContent><DialogHeader><DialogTitle>登录添加 TRAE {variantLabel(variant)}</DialogTitle><DialogDescription>在官方客户端完成登录后，返回此处添加账号。</DialogDescription></DialogHeader>
      <div className="flex items-center gap-3 rounded-xl border bg-muted/30 p-4"><TraeMark size={40} global={variant === "ai"} /><div><p className="text-sm font-medium">TRAE {variantLabel(variant)}</p><p className="mt-1 text-xs text-muted-foreground">登录信息由系统加密保存在本机。</p></div></div>
      <DemoAction><Button variant="outline" disabled={busy} onClick={() => void run("open", async () => { await api.traeAction("open", { variant, client: "desktop" }); }, "已打开官方客户端，请完成登录")}><ExternalLink />打开官方客户端登录</Button></DemoAction>
      <DialogFooter><Button variant="outline" disabled={busy} onClick={() => setLoginOpen(false)}>取消</Button><DemoAction><Button disabled={busy} onClick={() => capture("desktop")}>{busy ? <Loader2 className="animate-spin" /> : <LogIn />}已完成登录，添加账号</Button></DemoAction></DialogFooter>
    </DialogContent></Dialog>

    <Dialog open={!!info} onOpenChange={open => { if (!open) setInfo(null); }}><DialogContent><DialogHeader><DialogTitle>账号信息</DialogTitle><DialogDescription>{info?.name}</DialogDescription></DialogHeader><div className="space-y-3 text-sm text-muted-foreground"><p className="break-all">账号 ID：{info?.accounts[0].uid}</p>{info?.accounts.find(a => a.email)?.email && <p className="break-all">邮箱：{info.accounts.find(a => a.email)?.email}</p>}{info?.accounts.map(a => <div key={a.id} className="rounded-lg border p-3"><p className="font-medium text-foreground">{clientLabel(a.client)}</p><p className="mt-1">添加于 {formatTime(a.capturedAt)}</p><p>登录有效期 {formatTime(a.expiresAt)}</p></div>)}</div></DialogContent></Dialog>

    <AlertDialog open={!!pending} onOpenChange={open => { if (!open && !busy) setPending(null); }}><AlertDialogContent><AlertDialogHeader><AlertDialogTitle>{pending?.kind === "delete" ? `删除账号「${pending.group.name}」？` : `切换到「${pending?.kind === "switch" ? pending.account.name : ""}」？`}</AlertDialogTitle><AlertDialogDescription>{pending?.kind === "delete" ? "删除本工具保存的登录副本，不影响 TRAE 当前登录。删除后需重新添加该账号才能再切回。" : "请先保存正在进行的工作。切换会正常退出 TRAE，完成后重新打开；保留本机项目与历史。客户端拒绝退出时会停止切换。"}</AlertDialogDescription></AlertDialogHeader><AlertDialogFooter><AlertDialogCancel disabled={busy}>取消</AlertDialogCancel><AlertDialogAction disabled={busy} onClick={event => {
      event.preventDefault(); if (!pending) return; const item = pending;
      if (item.kind === "delete") void run(`delete:${item.group.key}`, async () => { for (const account of item.group.accounts) await api.traeAction("delete", { accountId: account.id }); setPending(null); }, "账号已删除");
      else void run(item.account.id, async () => { await api.traeAction("switch", { variant: item.account.variant, client: item.account.client, accountId: item.account.id, closeRunning: true, relaunch: true }); setPending(null); }, "TRAE 账号已切换");
    }}>{busy ? <Loader2 className="animate-spin" /> : null}{pending?.kind === "delete" ? "删除" : "关闭并切换"}</AlertDialogAction></AlertDialogFooter></AlertDialogContent></AlertDialog>
  </div>;
}
