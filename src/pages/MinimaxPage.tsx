import { useAccountDensity, useBasicAccountSort, orderBasicAccounts, accountGrid } from "@/lib/account-layout";
import { AccountViewControls } from "@/components/account-view-controls";
import { ACCOUNT_CARD_CLASS } from "@/lib/account-layout";
import { useCallback, useEffect, useRef, useState } from "react";
import { Check, Clock3, Ellipsis, FileDown, FileUp, Gift, Fingerprint, Info, Loader2, LogIn, Pencil, RefreshCw, Sparkles, Trash2 } from "lucide-react";
import { toast } from "sonner";

import { ClaimActionButton, ClaimStateBadge } from "@/components/claim-action";
import { ClaimLedgerDialog } from "@/components/claim-ledger-dialog";
import { DemoAction } from "@/components/demo-action";
import { MinimaxLoginDialog } from "@/components/minimax-login-dialog";
import { MinimaxBackupDialog } from "@/components/minimax-backup-dialog";
import { MinimaxMark } from "@/components/product-marks";
import { AlertDialog, AlertDialogAction, AlertDialogCancel, AlertDialogContent, AlertDialogDescription, AlertDialogFooter, AlertDialogHeader, AlertDialogTitle } from "@/components/ui/alert-dialog";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Dialog, DialogContent, DialogDescription, DialogHeader, DialogTitle } from "@/components/ui/dialog";
import { DropdownMenu, DropdownMenuContent, DropdownMenuItem, DropdownMenuSeparator, DropdownMenuTrigger } from "@/components/ui/dropdown-menu";
import { Input } from "@/components/ui/input";
import { Tabs, TabsList, TabsTrigger } from "@/components/ui/tabs";
import * as api from "@/lib/api";
import { deriveClaimView, entryFor, ledgerEntriesFor } from "@/lib/claim-status";
import { useClaimLedger } from "@/lib/use-claim-ledger";
import { cn } from "@/lib/utils";
import { useVisibleInterval } from "@/lib/use-visible-interval";
import { useMinimaxStatus } from "@/stores/minimax-status";
import { useMinimaxView } from "@/stores/minimax-view";

const CARD_CLASS = ACCOUNT_CARD_CLASS;
const format = (value: number) => new Intl.NumberFormat("zh-CN", { maximumFractionDigits: 2 }).format(value);
/** `expiresAt` / `checkedAt` 是秒，`capturedAt` 是毫秒，统一在这里换算。 */
const formatTime = (seconds?: number | null) => seconds ? new Date(seconds * 1000).toLocaleString("zh-CN", { hour12: false }) : "—";
const formatMs = (ms?: number | null) => ms ? formatTime(ms / 1000) : "—";
const regionLabel = (region: api.MinimaxRegion) => region === "en" ? "国际版" : "国内版";

function formatExpiry(seconds?: number | null) {
  if (!seconds) return "长期有效";
  const date = new Date(seconds * 1000);
  if (Number.isNaN(date.getTime())) return "长期有效";
  return `${String(date.getMonth() + 1).padStart(2, "0")}/${String(date.getDate()).padStart(2, "0")} ${String(date.getHours()).padStart(2, "0")}:${String(date.getMinutes()).padStart(2, "0")}`;
}

/** 与 TRAE 积分面板同构：总额 + 最近到期的两个积分包；未知余额不按零显示。 */
function CreditBlock({ credit, loading, error, accountName }: { credit?: api.MinimaxCredit | null; loading: boolean; error?: string; accountName: string }) {
  const known = credit?.available === true && credit.remaining != null;
  const packs = (credit?.packs ?? []).filter(pack => pack.remaining > 0).sort((a, b) => (a.expiresAt ?? Number.POSITIVE_INFINITY) - (b.expiresAt ?? Number.POSITIVE_INFINITY));
  const soon = packs.find(pack => pack.expiresAt)?.expiresAt ?? null;
  return <div className="min-w-0">
    <div className="flex flex-wrap items-baseline gap-x-3 gap-y-1">
      <span className="flex items-center gap-1.5">
        <Sparkles className="size-4 shrink-0 stroke-[1.75] text-muted-foreground" aria-hidden />
        <strong className="font-semibold leading-none tabular-nums tracking-[-0.025em] text-foreground text-[20px]" style={{ fontFamily: '"Bricolage Grotesque Variable", "SF Pro Display", ui-sans-serif, sans-serif' }}>
          {known ? format(credit.remaining as number) : "—"}
        </strong>
      </span>
      <span className="text-[11px]">{known ? credit?.planName || "积分" : loading ? "查询中…" : error ? "查询失败" : credit ? "额度未知" : "待查询"}</span>
      <span className="ml-auto text-[11px] text-muted-foreground" title={soon ? `最近到期 ${formatExpiry(soon)}` : known ? "当前积分长期有效" : undefined}>
        {loading ? "更新中…" : credit?.checkedAt ? `${new Date(credit.checkedAt * 1000).toLocaleTimeString("zh-CN", { hour: "2-digit", minute: "2-digit", hour12: false })} 更新` : "—"}
      </span>
    </div>
    {known && <div className="mt-3 space-y-2">
      {packs.length === 0
        ? <div className="py-1 text-[11px] text-muted-foreground">暂无可用积分包</div>
        : packs.slice(0, 2).map((pack, index) => {
          const ratio = pack.total > 0 ? Math.min(100, Math.max(0, pack.remaining / pack.total * 100)) : 0;
          return <div key={`${pack.name}-${pack.expiresAt ?? "none"}-${index}`} className="min-w-0" title={`${pack.name} · 剩余 ${format(pack.remaining)} / ${format(pack.total)}`}>
            <div className="grid min-w-0 grid-cols-[auto_minmax(0,1fr)_auto] items-center gap-3 text-[11px]">
              <span className="rounded-lg bg-muted/80 px-1.5 py-0.5 font-medium tabular-nums text-foreground">{format(pack.remaining)}</span>
              <span className="truncate text-muted-foreground">{pack.name}</span>
              <span className="whitespace-nowrap tabular-nums text-muted-foreground">{formatExpiry(pack.expiresAt)}</span>
            </div>
            <div className="mt-1 h-1 overflow-hidden rounded-full bg-muted" aria-hidden><div className="h-full rounded-full bg-primary" style={{ width: `${ratio}%` }} /></div>
          </div>;
        })}
    </div>}
    {error ? <p className="mt-3 text-xs text-destructive">{error}</p>
      : !known && !loading ? <p className="mt-3 text-[11px] text-muted-foreground">{credit ? "官方暂未提供可识别的积分，未知余额不会按零计算。" : "点击刷新积分，查询官方可用积分。"}</p> : null}
    {credit?.planExpiresAt ? <p className="mt-2 text-[11px] text-muted-foreground">{accountName} · 套餐到期 {formatTime(credit.planExpiresAt)}</p> : null}
  </div>;
}

type Pending = { kind: "switch"; account: api.MinimaxAccount } | { kind: "delete"; account: api.MinimaxAccount };

export default function MinimaxPage() {
  const {compact,toggleCompact}=useAccountDensity();
  const [sort,setSort]=useBasicAccountSort("minimax");
  const { region, setRegion } = useMinimaxView();
  const { status, error: loadError, refresh } = useMinimaxStatus();
  const [busyKey, setBusyKey] = useState<string | null>(null);
  const mutationRunning = useRef(false);
  const [pending, setPending] = useState<Pending | null>(null);
  const [loginOpen, setLoginOpen] = useState(false);
  const [backupMode, setBackupMode] = useState<"import" | "export" | null>(null);
  const [claiming, setClaiming] = useState(false);
  const [claimResults, setClaimResults] = useState<Record<string, api.MinimaxCheckin>>({});
  /** 今日领取记忆：MiniMax 每次领取只有一个签到任务，用来避免重复点击。 */
  const { ledger: claimLedger, reload: reloadClaimLedger } = useClaimLedger();
  const [claimDetailsOpen, setClaimDetailsOpen] = useState(false);
  const [editing, setEditing] = useState<string | null>(null);
  const [draft, setDraft] = useState("");
  const cancelRename = useRef(false);
  const [info, setInfo] = useState<api.MinimaxAccount | null>(null);
  const [creditOverrides, setCreditOverrides] = useState<Record<string, api.MinimaxCredit>>({});
  const [creditErrors, setCreditErrors] = useState<Record<string, string>>({});
  const [creditLoading, setCreditLoading] = useState<Record<string, boolean>>({});
  const querying = useRef(new Set<string>());
  const busy = busyKey !== null || claiming;

  const client = (status?.clients ?? []).find(c => c.region === region);
  const accounts = orderBasicAccounts((status?.accounts ?? []).filter(a => a.region === region),sort,client?.activeAccountId);
  const claimView = deriveClaimView(
    claimLedger,
    "minimax",
    accounts.map(account => ({ id: account.id, variant: account.region })),
    "rewards",
  );
  const claimEntries = ledgerEntriesFor(claimLedger, "minimax", "rewards", region);
  const claimAccountNames = Object.fromEntries(accounts.map(account => [account.id, account.name]));
  const claimPendingAccounts = accounts
    .filter(account => !claimEntries.some(entry => entry.accountId === account.id))
    .map(account => ({ id: account.id, name: account.name }));

  useVisibleInterval(() => { void refresh(); }, 60_000);
  useEffect(() => {
    const onFocus = () => { void refresh(); };
    window.addEventListener("focus", onFocus);
    return () => window.removeEventListener("focus", onFocus);
  }, [refresh]);

  const queryCredits = useCallback(async (items: api.MinimaxAccount[]) => {
    for (const account of items) {
      if (account.needsLogin || querying.current.has(account.id)) continue;
      querying.current.add(account.id);
      setCreditLoading(values => ({ ...values, [account.id]: true }));
      try {
        const credit = await api.minimaxAction("credits", { accountId: account.id }) as api.MinimaxCredit;
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
  }, [accountKey, region, queryCredits]);

  async function claim(items: api.MinimaxAccount[]) {
    if (mutationRunning.current) return;
    mutationRunning.current = true; setClaiming(true);
    let claimed = 0, already = 0, closed = 0, failed = 0;
    try {
      for (const account of items) {
        let result: api.MinimaxCheckin;
        try { result = await api.minimaxAction("checkin", { accountId: account.id }) as api.MinimaxCheckin; }
        catch (error) { result = { ok: false, state: "failed", message: api.asError(error), credits: null }; }
        setClaimResults(values => ({ ...values, [account.id]: result }));
        if (result.balance) setCreditOverrides(values => ({ ...values, [account.id]: result.balance! }));
        if (result.state === "claimed") claimed++; else if (result.state === "already") already++; else if (result.state === "closed") closed++; else failed++;
      }
      // 只报数量；原因与账号明细去「今日领取详情」看。
      const message = `成功 ${claimed}，已领取 ${already}，未开放 ${closed}${failed ? `，失败 ${failed}` : ""}`;
      const options = { description: message, action: { label: "查看详情", onClick: () => setClaimDetailsOpen(true) } };
      if (failed) toast.warning("领取完成，有未完成项", options); else toast.success("领取完成", options);
    } finally { await refresh(true); await reloadClaimLedger(); setClaiming(false); mutationRunning.current = false; }
  }

  async function run(key: string, action: () => Promise<void>, message: string) {
    if (mutationRunning.current) return;
    mutationRunning.current = true; setBusyKey(key);
    try { await action(); toast.success(message); }
    catch (error) { toast.error(api.asError(error)); }
    finally { await refresh(true); mutationRunning.current = false; setBusyKey(null); }
  }
  function capture() {
    void run("capture", async () => { await api.minimaxAction("capture", { region }); setLoginOpen(false); }, "当前登录账号已添加");
  }
  function rename(account: api.MinimaxAccount) {
    if (cancelRename.current) { cancelRename.current = false; return; }
    const name = draft.trim();
    if (!name || name === account.name) { setEditing(null); return; }
    void run(`rename:${account.id}`, async () => { await api.minimaxAction("rename", { accountId: account.id, name }); setEditing(null); }, "账号名称已更新");
  }
  /** 占位的「账号 N」可以再问一次平台接口，补上真实 uid / 昵称并合并重复卡片。 */
  function resolveIdentity(account: api.MinimaxAccount) {
    void run(`identity:${account.id}`, async () => { await api.minimaxAction("identity", { accountId: account.id }); }, "账号身份已更新");
  }
  function creditOf(account: api.MinimaxAccount) {
    return creditOverrides[account.id] ?? account.credits ?? null;
  }
  const state = loadError ? "状态未知" : client?.running ? "运行中" : client?.configured ? "已登录 · 未运行" : client?.detected ? "未登录" : "未检测到安装";

  return <div className="mx-auto w-full max-w-[1180px] px-4 py-6 sm:px-8 sm:py-9">
    <header className="mb-6"><div className="flex flex-wrap items-start justify-between gap-4">
      <div className="min-w-0">
        <p className="mb-2 text-[11px] font-semibold tracking-[0.16em] text-primary">MINIMAX / CODE</p>
        <h1 className="text-[28px] font-semibold tracking-tight">账号管理</h1>
        <p className="mt-2 text-sm leading-6 text-muted-foreground">管理 MiniMax Code 国内版与国际版的账号、积分与登录状态。桌面端与 CLI 共用同一份登录态。</p>
        <Tabs className="mt-4 gap-0" value={region} onValueChange={value => setRegion(value === "en" ? "en" : "cn")}>
          <TabsList aria-label="MiniMax Code 区域">
            <TabsTrigger value="cn">国内版</TabsTrigger>
            <TabsTrigger value="en">国际版</TabsTrigger>
          </TabsList>
        </Tabs>
      </div>
      <div className="flex shrink-0 items-center gap-2.5 pt-1">
        <button type="button" className="inline-flex rounded-[22%] bg-muted-foreground/30 p-[2px]" aria-label={`MiniMax Code ${regionLabel(region)}：${state}`} title={`MiniMax Code ${regionLabel(region)}：${state}，点击刷新状态`} onClick={() => void refresh()}>
          <span className={cn("inline-flex rounded-[20%] p-[2px]", client?.running && "bg-primary")}><MinimaxMark size={28} global={region === "en"} /></span>
        </button>
      </div>
    </div></header>

    <div className="relative mb-6 overflow-visible rounded-2xl border border-border bg-muted/30 px-5 py-5 shadow-[0_6px_20px_rgba(15,23,42,.025)]">
      <div className="pointer-events-none absolute inset-0 overflow-hidden rounded-2xl"><div className="absolute -right-12 -top-20 size-44 rounded-full border-[28px] border-slate-400/[0.035]" /></div>
      <div className="relative flex flex-wrap items-center gap-x-5 gap-y-4">
        <div className="min-w-[190px] flex-1"><h2 className="text-sm font-semibold text-foreground">添加与迁移账号</h2><p className="mt-1 text-xs leading-5 text-muted-foreground">在官方客户端完成登录后，识别并添加到账号库</p></div>
        <DemoAction><Button className="h-10 bg-primary px-4 text-primary-foreground shadow-sm hover:bg-primary/90" disabled={busy} onClick={() => setLoginOpen(true)}><LogIn />登录添加</Button></DemoAction>
        <DemoAction><Button variant="ghost" size="sm" disabled={busy} onClick={() => setBackupMode("import")}><FileUp />导入备份</Button></DemoAction>
        <DemoAction><Button variant="ghost" size="sm" disabled={busy || !status?.accounts.length} onClick={() => setBackupMode("export")}><FileDown />导出</Button></DemoAction>
        <DemoAction><Button variant="ghost" size="sm" className="h-9 px-2.5" disabled={busy} onClick={capture} title="识别并保存官方客户端当前登录的账号"><RefreshCw className={busyKey === "capture" ? "animate-spin" : undefined} />识别本机账号</Button></DemoAction>
      </div>
    </div>

    {loadError && <div className="mb-4 flex flex-wrap items-center gap-2 text-sm text-destructive"><span>读取 MiniMax Code 状态失败：{loadError}</span><Button variant="outline" size="sm" onClick={() => void refresh()}>重试</Button></div>}
    {client?.recognitionError && <p className="mb-4 text-sm text-destructive">{client.name}：{client.recognitionError}</p>}
    {client?.configured && client.tokenExpired && <div className="mb-4 flex items-start gap-2 rounded-xl border border-amber-500/30 bg-amber-500/5 px-3.5 py-2.5 text-xs leading-5 text-amber-700 dark:text-amber-300"><Clock3 className="mt-0.5 size-3.5 shrink-0" /><span>客户端当前登录的访问令牌已过期。本工具不代刷客户端的令牌；打开一次 MiniMax Code，它会用自己的刷新令牌更新。</span></div>}

    <section className="mt-7 min-w-0" aria-labelledby="minimax-accounts-title">
      <div className="mb-4 flex flex-wrap items-center justify-between gap-3">
        <div className="flex items-center gap-2">
          <h2 id="minimax-accounts-title" className="text-base font-semibold tracking-tight">账号</h2>
          <Badge variant="secondary" className="h-6 min-w-6 rounded-full border-0 px-1.5 text-[11px] tabular-nums text-muted-foreground shadow-none" aria-label={`${accounts.length} 个${regionLabel(region)} MiniMax Code 账号`}>{accounts.length}</Badge>
        </div>
        <div className="ml-auto flex max-w-full flex-wrap items-center justify-end gap-1"><AccountViewControls compact={compact} toggleCompact={toggleCompact} sort={sort} setSort={setSort}/>
          <DemoAction><Button variant="ghost" size="sm" className="h-9 rounded-lg text-xs" disabled={busy || accounts.length === 0 || accounts.some(a => creditLoading[a.id])} onClick={() => void queryCredits(accounts)} aria-label="刷新积分" title="查询当前区域所有账号的官方积分"><RefreshCw className={accounts.some(a => creditLoading[a.id]) ? "animate-spin" : undefined} />刷新积分</Button></DemoAction>
          <ClaimActionButton view={claimView} running={claiming} disabled={busyKey !== null || !accounts.length} hint="逐个账号领取 MiniMax 每日签到积分（已领完的账号会直接跳过）" onClaim={() => void claim(accounts)} />
        </div>
      </div>
      {!status && !loadError ? <div className="flex items-center gap-2 py-16 text-sm text-muted-foreground"><Loader2 className="size-4 animate-spin" />加载账号…</div>
        : accounts.length === 0 && !(client?.configured && !client.activeAccountId) ? <div className="rounded-2xl border border-dashed border-border bg-muted/15 px-6 py-14 text-center text-sm text-muted-foreground"><p>暂无{regionLabel(region)} MiniMax Code 账号。</p><p className="mt-2 text-xs">点击上方「登录添加」；已有本机登录可直接识别。</p></div>
        : <div className={accountGrid(compact)}>
          {client?.configured && !client.activeAccountId && <article className={CARD_CLASS}>
            <header className="flex min-h-[52px] items-center gap-3 border-b border-border bg-primary/5 px-3.5 py-1.5"><MinimaxMark size={24} global={region === "en"} /><div><h3 className="text-sm font-semibold">本机当前登录</h3><p className="text-xs text-muted-foreground">{client.name} · 尚未添加</p></div></header>
            <div className={cn("flex-1 text-xs leading-5 text-muted-foreground",compact?"px-3.5 pb-4 pt-3":"px-5 pb-5 pt-4")}>已识别到官方客户端的登录，可直接添加后管理和切换。</div>
            <footer className="border-t px-3.5 py-2"><DemoAction><Button size="sm" variant="outline" disabled={busy} onClick={capture}>{busyKey === "capture" ? <Loader2 className="animate-spin" /> : <LogIn />}添加当前账号</Button></DemoAction></footer>
          </article>}
          {accounts.map(account => {
            const active = account.active || client?.activeAccountId === account.id;
            const loading = creditLoading[account.id];
            return <article key={account.id} className={CARD_CLASS}>
              <header className={cn("relative flex items-center gap-2 border-b border-border",compact?"min-h-[52px] px-3.5 py-1.5":"min-h-[104px] px-5 py-3", active ? "bg-primary/5" : "bg-muted/30")}>
                <div className="pointer-events-none absolute inset-0 overflow-hidden"><div className={cn("absolute -right-10 -top-16 size-20 rounded-full blur-2xl", active ? "bg-primary/15" : "bg-muted/30")} />{active && <div className="absolute right-5 top-[64%] -translate-y-1/2 rotate-[7deg] opacity-[0.075] saturate-50 grayscale-[10%]"><MinimaxMark size={40} global={region === "en"} /></div>}</div>
                <div className="relative z-10 min-w-0 flex-1">
                  {editing === account.id
                    ? <Input autoFocus aria-label="账号名称" maxLength={80} value={draft} onChange={e => setDraft(e.target.value)} onBlur={() => rename(account)} onKeyDown={e => { if (e.key === "Enter") rename(account); if (e.key === "Escape") { cancelRename.current = true; setEditing(null); } }} className="h-8" />
                    : <h3 className="truncate text-[13px] font-semibold" title={account.name}>{account.name}</h3>}
                  {!api.isDemoMode() && account.email && <p className="truncate text-[11px] text-muted-foreground">{account.email}</p>}
                </div>
                <span className="relative z-10 inline-flex items-center gap-1.5">
                  {active
                    ? <span className="relative inline-flex size-7 shrink-0 items-center justify-center rounded-lg border border-primary/25 bg-primary/10" aria-label="当前账号" title="客户端当前账号"><MinimaxMark size={15} global={region === "en"} /><span className="absolute -right-1 -top-1 flex size-3.5 items-center justify-center rounded-full bg-primary text-primary-foreground"><Check className="size-2.5" strokeWidth={3} /></span></span>
                    : <DemoAction><Button variant="outline" size="icon" className="size-7 rounded-lg" disabled={busy || account.needsLogin} onClick={() => setPending({ kind: "switch", account })} aria-label="切换到此账号" title={account.needsLogin ? "登录已过期，请重新登录添加" : "将 MiniMax Code 切换到此账号"}>{busyKey === account.id ? <Loader2 className="size-4 animate-spin" /> : <MinimaxMark size={15} global={region === "en"} />}</Button></DemoAction>}
                </span>
                <DropdownMenu><DropdownMenuTrigger asChild><Button variant="ghost" size="icon" className="relative z-20 size-7 rounded-lg text-muted-foreground" aria-label={`管理账号 ${account.name}`} disabled={busy}><Ellipsis className="size-4" /></Button></DropdownMenuTrigger><DropdownMenuContent align="end" className="w-40">
                  {!api.isDemoMode() && (() => {
                    const entry = entryFor(claimLedger, "minimax", { id: account.id, variant: account.region }, "rewards");
                    const settled = entry?.state === "claimed" || entry?.state === "blocked";
                    return <DropdownMenuItem disabled={busy || settled} onSelect={() => void claim([account])}><Gift />{settled ? (entry?.state === "claimed" ? "今日已领完" : "今日无可领取") : "领取积分"}</DropdownMenuItem>;
                  })()}
                  {!api.isDemoMode() && <DropdownMenuItem disabled={loading} onSelect={() => void queryCredits([account])}><RefreshCw />刷新积分</DropdownMenuItem>}
                  <DropdownMenuItem onSelect={() => setInfo(account)}><Info />账号信息</DropdownMenuItem>
                  {!api.isDemoMode() && account.uid.startsWith("epoch:") && <DropdownMenuItem disabled={loading} onSelect={() => resolveIdentity(account)}><Fingerprint />识别身份</DropdownMenuItem>}
                  {!api.isDemoMode() && <><DropdownMenuItem onSelect={() => { cancelRename.current = false; setDraft(account.name); setEditing(account.id); }}><Pencil />重命名</DropdownMenuItem><DropdownMenuSeparator /><DropdownMenuItem className="text-destructive focus:bg-destructive/5 focus:text-destructive" onSelect={() => setPending({ kind: "delete", account })}><Trash2 />删除账号</DropdownMenuItem></>}
                </DropdownMenuContent></DropdownMenu>
              </header>
              <div className="flex-1 px-3.5 pb-4 pt-3 text-xs leading-5 text-muted-foreground">
                <CreditBlock credit={creditOf(account)} loading={!!loading} error={creditErrors[account.id]} accountName={account.name} />
                {(() => {
                  const entry = entryFor(claimLedger, "minimax", { id: account.id, variant: account.region }, "rewards");
                  return entry ? (
                    <div className="mt-2">
                      <ClaimStateBadge entry={entry} onClick={() => setClaimDetailsOpen(true)} />
                    </div>
                  ) : null;
                })()}
                {claimResults[account.id] && <p className={cn("mt-3 text-xs", claimResults[account.id].ok ? "text-muted-foreground" : "text-amber-700")} role="status">{claimResults[account.id].message}{claimResults[account.id].warning ? ` · ${claimResults[account.id].warning}` : ""}</p>}
                {account.needsLogin && <div className="mt-3 flex items-center justify-between gap-2 text-amber-700 dark:text-amber-300"><span>登录已过期</span><DemoAction><Button size="sm" variant="ghost" disabled={busy} onClick={() => setLoginOpen(true)}>重新登录</Button></DemoAction></div>}
              </div>
              <footer className="flex items-center gap-2 border-t border-border px-3.5 py-2 text-[11px] text-muted-foreground">
                <span>添加于 {formatMs(account.capturedAt)}</span>
                <span className="ml-auto">登录有效期 {formatTime(account.expiresAt)}</span>
              </footer>
            </article>;
          })}
        </div>}
    </section>
    <p className="mt-6 text-xs leading-6 text-muted-foreground">切换会先退出 MiniMax Code，写入账号凭据后重新打开；本机项目与历史保留，云端会话仍归原账号所有。CLI 与桌面端共用登录态，切换后两者同时生效。</p>

    <ClaimLedgerDialog
      open={claimDetailsOpen}
      onOpenChange={setClaimDetailsOpen}
      title={`今日领取详情 · MiniMax ${regionLabel(region)}`}
      entries={claimEntries}
      accountNames={claimAccountNames}
      pendingAccounts={claimPendingAccounts}
    />
    <MinimaxLoginDialog open={loginOpen} onOpenChange={open => { if (!busy) setLoginOpen(open); }} region={region} label={regionLabel(region)} />
    <MinimaxBackupDialog mode={backupMode} onClose={() => setBackupMode(null)} />

    <Dialog open={!!info} onOpenChange={open => { if (!open) setInfo(null); }}><DialogContent>
      <DialogHeader><DialogTitle>账号信息</DialogTitle><DialogDescription>{info?.name}</DialogDescription></DialogHeader>
      <div className="space-y-3 text-sm text-muted-foreground">
        <p className="break-all">账号 ID：{info?.uid}</p>
        {info?.email && <p className="break-all">邮箱：{info.email}</p>}
        <div className="rounded-lg border p-3"><p className="font-medium text-foreground">{info ? regionLabel(info.region) : ""} · 桌面端与 CLI 共用</p><p className="mt-1">添加于 {formatMs(info?.capturedAt)}</p><p>登录有效期 {formatTime(info?.expiresAt)}</p></div>
      </div>
    </DialogContent></Dialog>

    <AlertDialog open={!!pending} onOpenChange={open => { if (!open && !busy) setPending(null); }}>
      <AlertDialogContent>
        <AlertDialogHeader>
          <AlertDialogTitle>{pending?.kind === "delete" ? `删除账号「${pending.account.name}」？` : `切换到「${pending?.account.name ?? ""}」？`}</AlertDialogTitle>
          <AlertDialogDescription>{pending?.kind === "delete" ? "删除本工具保存的登录副本，不影响 MiniMax Code 当前登录。删除后需重新添加该账号才能再切回。" : "请先保存正在进行的工作。切换会正常退出 MiniMax Code，写入凭据后重新打开；保留本机项目与历史。客户端拒绝退出时会停止切换。"}</AlertDialogDescription>
        </AlertDialogHeader>
        <AlertDialogFooter>
          <AlertDialogCancel disabled={busy}>取消</AlertDialogCancel>
          <AlertDialogAction disabled={busy} onClick={event => {
            event.preventDefault(); if (!pending) return; const item = pending;
            if (item.kind === "delete") void run(`delete:${item.account.id}`, async () => { await api.minimaxAction("delete", { accountId: item.account.id }); setPending(null); }, "账号已删除");
            else void run(item.account.id, async () => { await api.minimaxAction("switch", { region: item.account.region, accountId: item.account.id, closeRunning: true, relaunch: true }); setPending(null); }, "MiniMax Code 账号已切换");
          }}>{busy ? <Loader2 className="animate-spin" /> : null}{pending?.kind === "delete" ? "删除" : "关闭并切换"}</AlertDialogAction>
        </AlertDialogFooter>
      </AlertDialogContent>
    </AlertDialog>
  </div>;
}
