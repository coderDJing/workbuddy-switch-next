import { useEffect, useState } from "react";
import { Link } from "react-router-dom";
import { Bell, Cloud, FileDown, FileUp, History, Loader2, RefreshCw, Save, Workflow } from "lucide-react";
import { toast } from "sonner";
import { TraeMark } from "@/components/product-marks";
import { DemoAction } from "@/components/demo-action";
import { TraeBackupDialog } from "@/components/trae-backup-dialog";
import { Button } from "@/components/ui/button";
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "@/components/ui/card";
import { Input } from "@/components/ui/input";
import { Switch } from "@/components/ui/switch";
import { Tabs, TabsList, TabsTrigger } from "@/components/ui/tabs";
import { Dialog, DialogContent, DialogDescription, DialogFooter, DialogHeader, DialogTitle } from "@/components/ui/dialog";
import * as api from "@/lib/api";
import { normalizeVariant } from "@/lib/variant";
import { useTraeView } from "@/stores/trae-view";
import { useTraeStatus } from "@/stores/trae-status";

const outcomes: Record<string, string> = { updated: "额度已更新", partial: "部分额度查询失败", running: "TRAE 运行中，已顺延", "unknown-process-state": "无法确认进程状态，未切换", switched: "已自动切换账号", deferred: "账号或状态变化，已停止切换", "no-accounts": "当前区域没有已保存账号" };
export default function TraeSettingsPage() {
  const { variant, setVariant } = useTraeView();
  const [settings, setSettings] = useState<api.TraeAutomation["settings"] | null>(null);
  const [saved, setSaved] = useState<api.TraeAutomation["settings"] | null>(null);
  const [logs, setLogs] = useState<api.TraeAutomation["logs"]>([]);
  const [error, setError] = useState("");
  const [busy, setBusy] = useState(false);
  const [reload, setReload] = useState(0);
  const [backupMode, setBackupMode] = useState<"import" | "export" | null>(null);
  const [passphrase, setPassphrase] = useState("");
  const [preview, setPreview] = useState<api.TraeBackupPreview | null>(null);
  const [confirmBackup, setConfirmBackup] = useState(false);
  useEffect(() => {
    let disposed = false;
    api.getTraeAutomation().then(result => { if (!disposed) { setSettings(result.settings); setSaved(result.settings); setLogs(result.logs); setError(""); } }).catch(error => { if (!disposed) setError(api.asError(error)); });
    const timer = window.setInterval(() => { void api.getTraeAutomation().then(result => { if (!disposed) setLogs(result.logs); }).catch(() => {}); }, 30_000);
    return () => { disposed = true; window.clearInterval(timer); };
  }, [reload]);
  const current = settings?.[variant];
  const dirty = JSON.stringify(settings) !== JSON.stringify(saved);
  const update = (patch: Partial<api.TraeRegionSettings>) => setSettings(value => value ? { ...value, [variant]: { ...value[variant], ...patch } } : value);
  async function work(action: () => Promise<void>) { if (busy) return; setBusy(true); try { await action(); } catch (error) { toast.error(api.asError(error)); } finally { setBusy(false); } }
  const toggle = (field: "refreshEnabled" | "alertsEnabled" | "rotateEnabled", label: string) => <div className="flex items-center justify-between gap-4"><span className="text-sm font-medium">{label}</span><Switch aria-label={label} checked={current?.[field] ?? false} disabled={busy || !current || api.isDemoMode()} onCheckedChange={value => update({ [field]: value })} /></div>;
  const numeric = (field: "intervalMinutes" | "expiryDays" | "lowBalance" | "rotateBelow" | "cooldownMinutes", label: string, min: number, max: number) => <label className="flex flex-wrap items-center justify-between gap-3 text-sm"><span>{label}</span><Input aria-label={label} type="number" min={min} max={max} className="w-24 bg-background tabular-nums" value={current?.[field] ?? ""} disabled={busy || !current || api.isDemoMode()} onChange={event => update({ [field]: Number(event.target.value) })} /></label>;
  async function cloud(action: "cloud_backup" | "cloud_preview" | "cloud_restore" | "cloud_sync") {
    await work(async () => {
      const result = await api.traeAction(action, { passphrase, snapshotId: preview?.snapshotId });
      if (action === "cloud_preview") { setPreview(result as api.TraeBackupPreview); return; }
      const summary = result as { count?: number; imported?: number; skipped?: number };
      setPassphrase(""); setPreview(null); setConfirmBackup(false); await useTraeStatus.getState().refresh(true);
      toast.success(action === "cloud_backup" ? `已加密备份 ${summary.count} 个 TRAE 账号` : action === "cloud_sync" ? `已同步 ${summary.count} 个 TRAE 账号` : `已恢复 ${summary.imported} 个账号，保留 ${summary.skipped} 个已有账号`);
    });
  }
  return <div className="mx-auto w-full max-w-[1180px] space-y-8 px-4 py-6 sm:px-8 sm:py-9">
    <header className="flex flex-wrap items-start justify-between gap-4"><div><h1 className="text-[28px] font-semibold tracking-tight">平台设置</h1><p className="mt-2 text-sm leading-6 text-muted-foreground">TRAE 自动查询、提醒、账号轮换与加密备份，国内版和国际版分别配置。</p></div><DemoAction><Button disabled={busy || !settings || !dirty} onClick={() => void work(async () => { const result = await api.traeAction("save_settings", { settings: settings! }) as api.TraeAutomation; setSettings(result.settings); setSaved(result.settings); toast.success("TRAE 设置已保存"); })}>{busy ? <Loader2 className="animate-spin" /> : <Save />}保存设置</Button></DemoAction></header>
    <Tabs value={variant} onValueChange={value => setVariant(normalizeVariant(value))}><TabsList aria-label="TRAE 设置区域"><TabsTrigger value="cn"><TraeMark size={16} />国内版</TabsTrigger><TabsTrigger value="ai"><TraeMark size={16} global />国际版</TabsTrigger></TabsList></Tabs>
    {error && <div className="flex items-center gap-3 text-sm text-destructive"><span>{error}</span><Button variant="outline" size="sm" onClick={() => setReload(value => value + 1)}>重试</Button></div>}
    <Card><CardHeader><CardTitle className="flex items-center gap-2"><RefreshCw className="size-4" />自动查询</CardTitle><CardDescription>后台定期查询官方额度并记录快照。需要保持桌面端或 WebUI 服务运行。</CardDescription></CardHeader><CardContent className="space-y-5">{toggle("refreshEnabled", "自动更新额度")}{numeric("intervalMinutes", "检查间隔（分钟）", 5, 1440)}</CardContent></Card>
    <Card><CardHeader><CardTitle className="flex items-center gap-2"><Bell className="size-4" />积分与登录提醒</CardTitle><CardDescription>余额不足、积分即将到期或登录过期时通知；同一账号的同类提醒每天最多一次。阈值使用账号原始计费单位。</CardDescription></CardHeader><CardContent className="space-y-5">{toggle("alertsEnabled", "启用提醒")}{numeric("lowBalance", "低余额阈值", 0, 1_000_000)}{numeric("expiryDays", "提前提醒到期（天）", 0, 90)}</CardContent></Card>
    <Card><CardHeader><CardTitle className="flex items-center gap-2"><Workflow className="size-4" />自动轮换</CardTitle><CardDescription>当前余额低于阈值时，选择同区域、同计费单位且余额更高的有效账号。TRAE 运行中会顺延，退出后再切换；额度未知或进程状态不明时保持当前账号。</CardDescription></CardHeader><CardContent className="space-y-5">{toggle("rotateEnabled", "启用自动轮换")}{numeric("rotateBelow", "低于此余额时轮换", 0, 1_000_000)}{numeric("cooldownMinutes", "轮换冷却时间（分钟）", 5, 1440)}</CardContent></Card>
    <Card><CardHeader><CardTitle className="flex items-center gap-2"><Cloud className="size-4" />账号备份与跨设备同步</CardTitle><CardDescription>备份用独立密码加密。云端使用全局 WebDAV 设置，并单独保存 TRAE 文件；导入与同步保留已有账号和当前登录。</CardDescription></CardHeader><CardContent className="space-y-5"><div className="flex flex-wrap gap-2"><DemoAction><Button variant="outline" disabled={busy} onClick={() => setBackupMode("export")}><FileDown />导出加密备份</Button></DemoAction><DemoAction><Button variant="outline" disabled={busy} onClick={() => setBackupMode("import")}><FileUp />导入备份</Button></DemoAction><Button asChild variant="ghost"><Link to="/settings">配置 WebDAV</Link></Button></div><Input aria-label="云端备份密码" type="password" autoComplete="new-password" placeholder="输入至少 8 位的云端备份密码" value={passphrase} disabled={busy || api.isDemoMode()} onChange={event => { setPassphrase(event.target.value); setPreview(null); }} /><div className="flex flex-wrap gap-2"><DemoAction><Button variant="outline" disabled={busy || Array.from(passphrase).length < 8} onClick={() => setConfirmBackup(true)}>备份到云端</Button></DemoAction><DemoAction><Button variant="outline" disabled={busy || Array.from(passphrase).length < 8} onClick={() => void cloud("cloud_sync")}>合并同步</Button></DemoAction><DemoAction><Button variant="outline" disabled={busy || Array.from(passphrase).length < 8} onClick={() => void cloud("cloud_preview")}>预览云端备份</Button></DemoAction></div></CardContent></Card>
    <Card><CardHeader className="flex flex-wrap items-start justify-between gap-4"><div><CardTitle className="flex items-center gap-2"><History className="size-4" />执行记录</CardTitle><CardDescription className="mt-2">仅执行已保存且启用的任务，记录最近 200 次结果。</CardDescription></div><DemoAction><Button variant="outline" size="sm" disabled={busy || dirty || !current || !(current.refreshEnabled || current.alertsEnabled || current.rotateEnabled)} onClick={() => void work(async () => { await api.traeAction("automation_run", { variant }); setReload(value => value + 1); toast.success("已检查当前区域的启用任务"); })}>{busy ? <Loader2 className="animate-spin" /> : <RefreshCw />}立即检查</Button></DemoAction></CardHeader><CardContent className="space-y-3">{logs.filter(log => log.region === variant).length ? logs.filter(log => log.region === variant).map((log, index) => <div key={index} className="flex flex-wrap justify-between gap-2 border-b pb-3 text-sm last:border-0"><span className="text-muted-foreground">{new Date(log.at * 1000).toLocaleString("zh-CN", { hour12: false })}</span><span>{log.action === "rotate" ? "账号轮换" : "额度查询"}</span><span>{outcomes[log.outcome] ?? log.outcome}</span></div>) : <p className="py-6 text-center text-sm text-muted-foreground">暂无执行记录</p>}</CardContent></Card>
    <TraeBackupDialog mode={backupMode} onClose={() => setBackupMode(null)} />
    <Dialog open={confirmBackup} onOpenChange={open => { if (!busy) setConfirmBackup(open); }}><DialogContent><DialogHeader><DialogTitle>更新云端 TRAE 备份？</DialogTitle><DialogDescription>将使用本机账号更新云端 TRAE 文件。需要保留两端账号时请选择合并同步。</DialogDescription></DialogHeader><DialogFooter><Button variant="outline" disabled={busy} onClick={() => setConfirmBackup(false)}>取消</Button><Button disabled={busy} onClick={() => void cloud("cloud_backup")}>确认备份</Button></DialogFooter></DialogContent></Dialog>
    <Dialog open={preview !== null} onOpenChange={open => { if (!open && !busy) setPreview(null); }}><DialogContent><DialogHeader><DialogTitle>恢复 TRAE 账号</DialogTitle><DialogDescription>确认添加以下缺失账号，已有账号会保留。</DialogDescription></DialogHeader><div className="max-h-64 space-y-2 overflow-auto text-sm">{preview?.accounts.map((a, i) => <div key={i} className="flex justify-between gap-3"><span>{a.name} · {a.variant === "cn" ? "国内版" : "国际版"}</span><span className="text-muted-foreground">{a.exists ? "已有，保留" : "将添加"}</span></div>)}</div><DialogFooter><Button variant="outline" disabled={busy} onClick={() => setPreview(null)}>取消</Button><Button disabled={busy || !preview?.accounts.some(a => !a.exists)} onClick={() => void cloud("cloud_restore")}>确认恢复</Button></DialogFooter></DialogContent></Dialog>
  </div>;
}
