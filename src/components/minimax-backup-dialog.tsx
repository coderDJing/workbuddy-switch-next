import { useEffect, useState } from "react";
import { Download, FileUp, Loader2 } from "lucide-react";
import { toast } from "sonner";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Dialog, DialogContent, DialogDescription, DialogFooter, DialogHeader, DialogTitle } from "@/components/ui/dialog";
import * as api from "@/lib/api";
import { useMinimaxStatus } from "@/stores/minimax-status";

export function MinimaxBackupDialog({ mode, onClose }: { mode: "export" | "import" | null; onClose: () => void }) {
  const [passphrase, setPassphrase] = useState("");
  const [data, setData] = useState("");
  const [preview, setPreview] = useState<api.MinimaxBackupPreview | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState("");
  const [savedPath, setSavedPath] = useState("");
  useEffect(() => { setPassphrase(""); setData(""); setPreview(null); setError(""); setSavedPath(""); }, [mode]);
  async function submit() {
    if (busy) return;
    setBusy(true); setError("");
    try {
      if (mode === "export") {
        const filename = `wb-switch-minimax-${Date.now()}.wbminimax`;
        if (api.isDesktop()) {
          const { save } = await import("@tauri-apps/plugin-dialog");
          const path = await save({ title: "导出 MiniMax Code 加密备份", defaultPath: filename, filters: [{ name: "MiniMax Code 加密备份", extensions: ["wbminimax"] }] });
          if (!path) return;
          await api.minimaxAction("backup_export", { passphrase, path }); setSavedPath(path);
        } else {
          const result = await api.minimaxAction("backup_export", { passphrase }) as { data: string };
          const url = URL.createObjectURL(new Blob([result.data], { type: "application/octet-stream" }));
          const link = document.createElement("a"); link.href = url; link.download = filename; link.click(); window.setTimeout(() => URL.revokeObjectURL(url), 1000); onClose();
        }
        setPassphrase(""); toast.success("MiniMax Code 加密备份已导出");
      } else if (!preview) {
        setPreview(await api.minimaxAction("backup_preview", { data, passphrase }) as api.MinimaxBackupPreview);
      } else {
        const result = await api.minimaxAction("backup_import", { data, passphrase, snapshotId: preview.snapshotId }) as { imported: number; skipped: number };
        await useMinimaxStatus.getState().refresh(true); toast.success(`已导入 ${result.imported} 个账号，保留 ${result.skipped} 个已有账号`); onClose();
      }
    } catch (error) { setError(api.asError(error)); }
    finally { setBusy(false); }
  }
  return <Dialog open={mode !== null} onOpenChange={open => { if (!open && !busy) onClose(); }}><DialogContent><DialogHeader><DialogTitle>{mode === "export" ? "导出 MiniMax Code 账号" : "导入 MiniMax Code 账号"}</DialogTitle><DialogDescription>使用独立密码加密，可在另一台电脑恢复账号。请妥善保存密码；导入保留已有账号，当前客户端登录不受影响。</DialogDescription></DialogHeader>
    {mode === "import" && <Input aria-label="MiniMax Code 备份文件" type="file" accept=".wbminimax,.wbs" disabled={busy} onChange={async event => { setPreview(null); setData(""); setError(""); const file = event.target.files?.[0]; if (file) { if (file.size > 16 * 1024 * 1024) { setError("备份文件过大"); return; } try { setData(await file.text()); } catch { setError("无法读取备份文件"); } } }} />}
    {!savedPath && <Input aria-label="备份加密密码" type="password" autoComplete="new-password" placeholder={mode === "export" ? "设置至少 8 位的备份密码" : "输入导出时设置的密码"} disabled={busy} value={passphrase} onChange={event => { setPassphrase(event.target.value); setPreview(null); }} />}
    {preview && <div className="max-h-60 space-y-2 overflow-y-auto rounded-xl border p-3 text-sm">{preview.accounts.map((a, i) => <div key={i} className="flex justify-between gap-3"><span>{a.name} · {a.region === "cn" ? "国内版" : "国际版"}</span><span className="text-muted-foreground">{a.exists ? "已有，保留" : "将添加"}</span></div>)}</div>}
    {savedPath && <p className="break-all rounded-lg bg-muted p-3 text-sm">已保存到 {savedPath}</p>}
    {error && <p className="text-sm text-destructive" role="alert">{error}</p>}
    <DialogFooter><Button variant="outline" disabled={busy} onClick={onClose}>{savedPath ? "完成" : "取消"}</Button>{!savedPath && <Button disabled={busy || Array.from(passphrase).length < 8 || (mode === "import" && !data)} onClick={() => void submit()}>{busy ? <Loader2 className="animate-spin" /> : mode === "export" ? <Download /> : <FileUp />}{mode === "export" ? "导出加密备份" : preview ? "确认导入" : "预览备份"}</Button>}</DialogFooter>
  </DialogContent></Dialog>;
}
