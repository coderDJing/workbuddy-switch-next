import { useEffect, useState } from "react";
import { Link } from "react-router-dom";
import { Bell, Check, Cloud, Gift, History, Layers2, Loader2, RefreshCw, Save, Workflow, Zap } from "lucide-react";
import { toast } from "sonner";
import { Button } from "@/components/ui/button";
import { Card, CardContent, CardHeader, CardTitle, CardDescription } from "@/components/ui/card";
import { Input } from "@/components/ui/input";
import { Switch } from "@/components/ui/switch";
import { Tabs, TabsList, TabsTrigger } from "@/components/ui/tabs";
import { TimePicker } from "@/components/ui/time-picker";
import { Dialog, DialogContent, DialogHeader, DialogTitle, DialogDescription } from "@/components/ui/dialog";
import * as api from "@/lib/api";
import { useQoderView } from "@/stores/qoder-view";
import { Badge } from "@/components/ui/badge";
import { QoderMark } from "@/components/product-marks";

export default function QoderSettingsPage() {
  const { variant, setVariant } = useQoderView();
  const [settings, setSettings] = useState<api.QoderSettings | null>(null);
  const [savedSettings, setSavedSettings] = useState<api.QoderSettings | null>(null);
  const [logs, setLogs] = useState<api.QoderAutomationStatus["logs"]>([]);
  const [error, setError] = useState("");
  const [busy, setBusy] = useState(false);
  const [hooks, setHooks] = useState<boolean | null>(null);
  const [hookError, setHookError] = useState("");
  const [password, setPassword] = useState("");
  const [preview, setPreview] = useState<api.QoderCloudResult | null>(null);
  const [selected, setSelected] = useState<number[]>([]);
  const [confirmBackup, setConfirmBackup] = useState(false);
  const [reload, setReload] = useState(0);
  useEffect(() => {
    let cancelled = false; setError("");
    api.getQoderAutomation().then(s => { if (!cancelled) { setSettings(s.settings); setSavedSettings(s.settings); setLogs(s.logs); } })
      .catch(e => { if (!cancelled) setError(api.asError(e)); });
    const timer = window.setInterval(() => {
      api.getQoderAutomation().then(s => { if (!cancelled) setLogs(s.logs); }).catch(() => {});
    }, 30000);
    return () => { cancelled = true; window.clearInterval(timer); };
  }, [reload]);
  useEffect(() => {
    let cancelled = false; setHooks(null); setHookError("");
    api.getQoderCompanion(variant === "ai").then(s => { if (!cancelled) setHooks(s.installed); })
      .catch(e => { if (!cancelled) setHookError(api.asError(e)); });
    return () => { cancelled = true; };
  }, [variant]);
  const c = settings?.[variant];
  const dirty = JSON.stringify(settings) !== JSON.stringify(savedSettings);
  const update = (patch: Partial<api.QoderRegionSettings>) => setSettings(s => s ? { ...s, [variant]: { ...s[variant], ...patch } } : s);
  async function work(fn: () => Promise<void>) {
    setBusy(true); try { await fn(); } catch (e) { toast.error(api.asError(e)); } finally { setBusy(false); }
  }
  async function cloud(action: "backup" | "sync" | "preview" | "restore") {
    await work(async () => {
      const result = await api.qoderCloud(action, password, selected, preview?.snapshotId);
      if (action === "preview") { setPreview(result); setSelected(result.accounts?.map(a => a.index) ?? []); return; }
      setPreview(null); setConfirmBackup(false); setPassword("");
      if (result.warnings?.length) toast.warning(`已导入 ${result.imported ?? 0} 个账号，部分未完成`, { description: result.warnings.join("；") });
      else toast.success(action === "backup" ? `已加密备份 ${result.count ?? 0} 个 Qoder 账号` : action === "sync" ? `已同步 ${result.count ?? 0} 个 Qoder 账号` : `已导入 ${result.imported ?? 0} 个，跳过 ${result.skipped ?? 0} 个已有账号`);
    });
  }
  const toggle = (key: "dailyEnabled" | "alertsEnabled" | "rotateEnabled", label: string) => <div className="flex items-center justify-between gap-4"><span className="text-sm font-medium">{label}</span><Switch aria-label={label} checked={c?.[key] ?? false} disabled={busy || !c} onCheckedChange={v => update({ [key]: v })} /></div>;
  const number = (key: "expiryDays" | "lowBalance" | "rotateBelow" | "cooldownMinutes", label: string, min: number, max: number) => <label className="flex flex-wrap items-center justify-between gap-2 text-sm"><span>{label}</span><Input aria-label={label} type="number" min={min} max={max} value={c?.[key] ?? ""} className="w-24 bg-background tabular-nums" disabled={busy || !c} onChange={e => update({ [key]: Number(e.target.value) })} /></label>;
  return <div className="mx-auto w-full max-w-[1180px] space-y-8 px-4 py-6 sm:px-8 sm:py-9">
    <header className="flex flex-wrap items-start justify-between gap-4"><div>
      <p className="mb-2 text-[11px] font-semibold tracking-[0.16em] text-primary">QODER / AUTOMATION</p>
      <h1 className="text-[28px] font-semibold tracking-tight">平台设置</h1><p className="mt-2 text-sm leading-6 text-muted-foreground">让领取、提醒与账号轮换按你的习惯运行。</p></div>
      <Tabs value={variant} onValueChange={v => setVariant(v === "ai" ? "ai" : "cn")}><TabsList aria-label="Qoder 设置区域"><TabsTrigger value="cn" disabled={busy}><QoderMark size={16} />国内版</TabsTrigger><TabsTrigger value="ai" disabled={busy}><QoderMark size={16} global />国际版</TabsTrigger></TabsList></Tabs></header>
    {error ? <div role="alert" className="rounded-xl border p-4 text-destructive">{error}<Button variant="ghost" onClick={() => setReload(r => r + 1)}>重试</Button></div> : !settings ? <Loader2 className="animate-spin" aria-label="加载设置" /> : null}
    <section aria-labelledby="qoder-automation-title" className="space-y-4">
      <div className="flex flex-wrap items-center justify-between gap-3"><div><h2 id="qoder-automation-title" className="flex items-center gap-2 text-base font-semibold"><Zap className="size-4 text-primary" />自动任务</h2><p className="mt-1 text-xs leading-5 text-muted-foreground">{variant === "ai" ? "国际版" : "国内版"}独立配置 · 仅在本工具运行期间执行</p></div><Badge variant="secondary" className="gap-1.5 font-normal">{c ? `${[c.dailyEnabled, c.alertsEnabled, c.rotateEnabled].filter(Boolean).length} / 3 项${dirty ? "已选择（待保存）" : "已启用"}` : "读取配置中"}</Badge></div>
    <div className="grid gap-4 xl:grid-cols-3">
      <Card className="shadow-none"><CardHeader><Gift className="mb-2 size-5 text-primary" /><CardTitle>每日积分</CardTitle><CardDescription className="leading-6">定时领取，错过后启动补领。每天最多尝试 3 次，失败间隔 30 分钟。</CardDescription></CardHeader><CardContent className="space-y-4">
        {toggle("dailyEnabled", "自动领取每日积分")}
        <div className="flex flex-wrap items-center justify-between gap-3 text-sm"><span>领取时间（本机时区）</span><TimePicker value={c?.dailyTime ?? "09:00"} onChange={dailyTime => update({ dailyTime })} disabled={!c || busy} /></div>
      </CardContent></Card>
      <Card className="shadow-none"><CardHeader><Bell className="mb-2 size-5 text-primary" /><CardTitle>积分提醒</CardTitle><CardDescription className="leading-6">每 5 分钟检查余额与到期情况。同类提醒每天一次，发送到通知中心。</CardDescription></CardHeader><CardContent className="space-y-4">
        {toggle("alertsEnabled", "启用积分及登录异常提醒")}
        {number("expiryDays", "到期前提醒（天）", 0, 90)}{number("lowBalance", "余额不足提醒（积分）", 0, 1000000)}
      </CardContent></Card>
      <Card className="shadow-none"><CardHeader><Workflow className="mb-2 size-5 text-primary" /><CardTitle>CLI 自动轮换</CardTitle><CardDescription className="leading-6">低余额时优先使用同区域的临期积分，其次按余额排序。仅在桌面端与 CLI 都关闭时切换。</CardDescription></CardHeader><CardContent className="space-y-4">
        {toggle("rotateEnabled", "启用 CLI 空闲自动轮换")}
        <div className="space-y-4">{number("rotateBelow", "轮换余额阈值（积分）", 0, 1000000)}{number("cooldownMinutes", "轮换冷却时间（分钟）", 5, 1440)}</div>
        <p className="text-xs leading-5 text-muted-foreground">查询失败或当前账号未识别时跳过轮换。</p>
      </CardContent></Card>
    </div>
      <div className="flex flex-wrap items-center gap-3 rounded-xl border bg-card px-4 py-3">
        <p role="status" className="mr-auto flex items-center gap-2 text-xs text-muted-foreground">{dirty ? <span className="size-1.5 rounded-full bg-amber-500" /> : <Check className="size-3.5" />}{!settings ? "等待配置加载" : dirty ? "有更改待保存" : "配置已同步"}</p>
        <Button variant="outline" size="sm" disabled={busy || !settings || dirty} title={dirty ? "请先保存设置，再检查任务" : undefined} onClick={() => work(async () => { const result = await api.runQoderAutomation(); setLogs((await api.getQoderAutomation()).logs); toast.info(result.skipped ? "已有任务执行中" : "已按保存的设置检查任务"); })}><RefreshCw />检查已启用的任务</Button>
        <Button size="sm" disabled={busy || !settings || !dirty} onClick={() => work(async () => { const saved = await api.saveQoderAutomation(settings!); setSettings(saved); setSavedSettings(saved); toast.success("Qoder 设置已保存"); })}>{busy ? <Loader2 className="animate-spin" /> : <Save />}保存设置</Button>
      </div>
    </section>
    <section aria-labelledby="qoder-integrations-title" className="space-y-4">
    <div><h2 id="qoder-integrations-title" className="text-base font-semibold">连接与备份</h2><p className="mt-1 text-xs leading-5 text-muted-foreground">跟踪会话进度，让账号在设备之间延续。</p></div>
    <div className="grid items-start gap-4 xl:grid-cols-2">
    <Card className="shadow-none"><CardHeader><Layers2 className="mb-2 size-5 text-primary" /><CardTitle>会话悬浮窗</CardTitle><CardDescription className="leading-6">接入 Qoder CLI 的开始、完成、失败及权限等待状态。只显示接入后产生的事件；需保持桌面版悬浮窗服务运行。</CardDescription></CardHeader><CardContent className="space-y-3">
      <p className="text-sm text-muted-foreground">{hookError || (hooks === null ? "正在读取接入状态…" : hooks ? "CLI Hooks 已配置；重新打开 Qoder CLI 后生效。" : "尚未接入")}</p>
      <div className="flex flex-wrap gap-2"><Button variant="outline" disabled={busy || !api.isDesktop()} onClick={() => work(async () => { const s = await api.setQoderCompanion(variant === "ai", true); setHooks(s.installed); setHookError(""); toast.success("已接入，请重新打开 Qoder CLI"); })}>接入会话悬浮窗</Button>
        <Button variant="ghost" disabled={busy || !hooks} onClick={() => work(async () => { const s = await api.setQoderCompanion(variant === "ai", false); setHooks(s.installed); toast.success("已移除本工具的 Qoder Hooks"); })}>移除接入</Button></div>
      {!api.isDesktop() && <p className="text-xs text-muted-foreground">请在桌面版完成悬浮窗接入。</p>}
    </CardContent></Card>
    <Card className="shadow-none"><CardHeader><Cloud className="mb-2 size-5 text-primary" /><CardTitle>WebDAV 加密备份与同步</CardTitle><CardDescription className="leading-6">包含国内版与国际版账号，复用全局 WebDAV 连接，独立加密保存。加密密码仅本次使用。</CardDescription></CardHeader><CardContent className="space-y-4">
      <Link className="text-sm text-primary underline underline-offset-4" to="/settings">配置 WebDAV 连接</Link>
      <p className="text-sm text-muted-foreground">同步合并两端账号；同一账号授权冲突时停止。恢复仅添加缺少的账号和客户端，已有授权与当前登录保持不变。</p>
      <Input type="password" autoComplete="off" aria-label="Qoder 备份加密密码" placeholder="加密密码（至少 8 位）" value={password} disabled={busy} onChange={e => { setPassword(e.target.value); setPreview(null); }} />
      <div className="flex flex-wrap gap-2"><Button variant="outline" disabled={busy || password.length < 8} onClick={() => setConfirmBackup(true)}>备份到云端</Button><Button variant="outline" disabled={busy || password.length < 8} onClick={() => cloud("sync")}>合并同步</Button><Button variant="outline" disabled={busy || password.length < 8} onClick={() => cloud("preview")}>预览云端恢复</Button>{busy && <Loader2 className="size-5 animate-spin self-center" />}</div>
    </CardContent></Card>
    </div></section>
    <Card className="shadow-none"><CardHeader><CardTitle className="flex items-center gap-2"><History className="size-4 text-primary" />自动任务记录</CardTitle><CardDescription>最近 100 条，仅显示{variant === "ai" ? "国际版" : "国内版"}。</CardDescription></CardHeader><CardContent className="space-y-2 text-sm">
      {logs.filter(l => l.region === variant).length === 0 ? <div className="rounded-xl border border-dashed p-6 text-center"><p className="text-sm font-medium">还没有任务记录</p><p className="mt-1 text-xs leading-6 text-muted-foreground">启用并保存自动任务后，执行结果会出现在这里。</p></div> : logs.filter(l => l.region === variant).map((l,i) => <div key={`${l.at}-${i}`} className="flex flex-wrap justify-between gap-2 border-b py-3 last:border-0"><span className="text-xs tabular-nums text-muted-foreground">{new Date(l.at).toLocaleString("zh-CN", { hour12: false })}</span><span>{l.action === "daily" ? "每日领取" : "CLI 轮换"} · {({success:"成功",failed:"失败，将按重试规则处理",running:"Qoder 运行中，已跳过",deferred:"状态变化，已推迟","no-candidate":"没有符合条件的账号"} as Record<string,string>)[l.outcome] ?? l.outcome}</span></div>)}
    </CardContent></Card>
    <Dialog open={confirmBackup} onOpenChange={v => { if (!busy) setConfirmBackup(v); }}><DialogContent><DialogHeader><DialogTitle>覆盖云端 Qoder 备份？</DialogTitle><DialogDescription>将使用本机全部 Qoder 账号生成加密备份，替换云端 Qoder 文件。需要保留两端账号时请选择合并同步。</DialogDescription></DialogHeader><Button disabled={busy} onClick={() => cloud("backup")}>{busy ? "备份中…" : "确认备份"}</Button></DialogContent></Dialog>
    <Dialog open={preview !== null} onOpenChange={v => { if (!v && !busy) setPreview(null); }}><DialogContent><DialogHeader><DialogTitle>恢复 Qoder 账号</DialogTitle><DialogDescription>勾选要添加的账号；已保存的账号授权会跳过。</DialogDescription></DialogHeader>
      <div className="max-h-72 space-y-3 overflow-y-auto">{preview?.accounts?.map(a => <label key={a.index} className="flex items-center gap-3 text-sm"><input type="checkbox" disabled={busy} checked={selected.includes(a.index)} onChange={e => setSelected(s => e.target.checked ? [...s,a.index] : s.filter(i => i !== a.index))} /><span>{a.label} · {a.global ? "国际版" : "国内版"}</span></label>)}</div>
      <Button disabled={busy || selected.length === 0} onClick={() => cloud("restore")}>{busy ? "恢复中…" : `恢复所选 ${selected.length} 个账号`}</Button>
    </DialogContent></Dialog>
  </div>;
}
