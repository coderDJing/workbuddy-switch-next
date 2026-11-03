import { useEffect, useRef, useState } from "react";
import { Download, FileUp, FolderOpen, Loader2 } from "lucide-react";

import { Alert, AlertDescription, AlertTitle } from "@/components/ui/alert";
import { Button } from "@/components/ui/button";
import { Dialog, DialogContent, DialogDescription, DialogFooter, DialogHeader, DialogTitle } from "@/components/ui/dialog";
import * as api from "@/lib/api";
import type { QoderAccountGroup } from "@/lib/qoder-accounts";
import { OverlayScrollArea } from "@/components/ui/overlay-scroll-area";

type Props = {
  mode: "export" | "import" | null;
  onClose: () => void;
  groups: QoderAccountGroup[];
  onImported: (result: { imported: number; skipped: number; warnings: string[] }) => void;
};

function exportFileName(): string {
  const date = new Date();
  const pad = (value: number) => String(value).padStart(2, "0");
  return `wb-switch-qoder-${date.getFullYear()}-${pad(date.getMonth() + 1)}-${pad(date.getDate())}.json`;
}

function revealLabel(): string {
  if (navigator.userAgent.includes("Windows")) return "在资源管理器中显示";
  if (navigator.userAgent.includes("Linux")) return "在文件管理器中显示";
  return "在 Finder 中显示";
}

export function QoderBackupDialog({ mode, onClose, groups, onImported }: Props) {
  const [selected, setSelected] = useState<Set<string>>(new Set());
  const [preview, setPreview] = useState<api.QoderBackupPreview[]>([]);
  const [fileText, setFileText] = useState("");
  const [fileName, setFileName] = useState("");
  const [parsing, setParsing] = useState(false);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState("");
  const [savedPath, setSavedPath] = useState<string | null>(null);
  const input = useRef<HTMLInputElement>(null);

  useEffect(() => {
    setSelected(new Set());
    setPreview([]);
    setFileText("");
    setFileName("");
    setParsing(false);
    setBusy(false);
    setError("");
    setSavedPath(null);
  }, [mode]);

  const items = mode === "export"
    ? groups.map((group) => ({ id: group.key, label: group.label, global: Object.values(group.targets).some((bundle) => bundle?.target.startsWith("global_")) }))
    : preview.map((account) => ({ id: String(account.index), label: account.label, global: account.global }));
  const allSelected = items.length > 0 && selected.size === items.length;

  function toggle(id: string) {
    setSelected((previous) => {
      const next = new Set(previous);
      if (next.has(id)) next.delete(id);
      else next.add(id);
      return next;
    });
  }

  async function onFile(file: File | undefined) {
    if (!file) return;
    setParsing(true);
    setError("");
    setPreview([]);
    setSelected(new Set());
    setFileText("");
    setFileName(file.name);
    try {
      if (file.size > 8 * 1024 * 1024) throw new Error("Qoder 备份文件过大");
      const text = await file.text();
      const result = await api.previewQoderBackup(text);
      setFileText(text);
      setPreview(result.accounts);
      setSelected(new Set(result.accounts.map((account) => String(account.index))));
    } catch (error) {
      setError(api.asError(error));
    } finally {
      setParsing(false);
    }
  }

  async function submit() {
    if (!selected.size || busy || parsing) return;
    setBusy(true);
    setError("");
    try {
      if (mode === "import") {
        const result = await api.importQoderBackup(fileText, [...selected].map(Number));
        onImported(result);
        onClose();
      } else {
        const ids = groups.filter((group) => selected.has(group.key)).flatMap((group) => Object.values(group.targets).flatMap((bundle) => bundle ? [bundle.id] : []));
        if (api.isWebui()) {
          const data = await api.exportQoderAccounts(ids);
          const url = URL.createObjectURL(new Blob([JSON.stringify(data, null, 2)], { type: "application/json" }));
          const link = document.createElement("a");
          link.href = url;
          link.download = exportFileName();
          document.body.appendChild(link);
          link.click();
          link.remove();
          URL.revokeObjectURL(url);
          onClose();
        } else {
          const { save } = await import("@tauri-apps/plugin-dialog");
          const path = await save({ title: "导出账号", defaultPath: exportFileName(), filters: [{ name: "JSON", extensions: ["json"] }] });
          if (path) setSavedPath((await api.exportQoderAccountsToPath(ids, path)).path);
        }
      }
    } catch (error) {
      setError(api.asError(error));
    } finally {
      setBusy(false);
    }
  }

  return (
    <Dialog open={mode !== null} onOpenChange={(open) => { if (!open && !busy && !parsing) onClose(); }}>
      <DialogContent className="min-w-0 overflow-x-hidden">
        <DialogHeader>
          <DialogTitle>{mode === "export" ? "导出账号" : "导入账号"}</DialogTitle>
          <DialogDescription>{mode === "export" ? "勾选要导出的账号，导出为 JSON 文件。" : "选择 JSON 文件，勾选要导入的账号。导入不会切换当前客户端。"}</DialogDescription>
        </DialogHeader>

        {mode === "export" && (
          <Alert variant="warning">
            <AlertTitle>安全提示</AlertTitle>
            <AlertDescription>导出文件含登录 token，等同密码，请勿上传网盘或发送给他人。</AlertDescription>
          </Alert>
        )}

        {mode === "import" && (
          <>
            <input ref={input} type="file" accept=".json,application/json" className="hidden" onChange={(event) => { void onFile(event.target.files?.[0]); event.target.value = ""; }} />
            <div className="flex items-center gap-2">
              <Button variant="outline" disabled={busy || parsing} onClick={() => input.current?.click()}><FileUp />选择文件</Button>
              {fileName && <span className="truncate text-xs text-muted-foreground">{fileName}</span>}
            </div>
          </>
        )}

        {parsing && <div className="flex items-center gap-2 py-4 text-sm text-muted-foreground"><Loader2 className="animate-spin" />正在解析…</div>}
        {!parsing && items.length > 0 && (
          <>
            <div className="flex items-center justify-between text-sm">
              <span className="text-muted-foreground">共 {items.length} 个账号，已选 {selected.size} 个</span>
              <button type="button" className="cursor-pointer text-primary hover:underline" onClick={() => setSelected(allSelected ? new Set() : new Set(items.map((item) => item.id)))}>{allSelected ? "取消全选" : "全选"}</button>
            </div>
            <OverlayScrollArea className="max-h-56">
              <div className="space-y-1 pr-1">
              {items.map((item) => (
                <label key={item.id} className="flex cursor-pointer items-center gap-3 rounded-md border px-3 py-2 hover:bg-accent/50">
                  <input type="checkbox" className="size-4 accent-primary" checked={selected.has(item.id)} onChange={() => toggle(item.id)} />
                  <span className="min-w-0 flex-1 truncate text-sm">{item.label}</span>
                  <span className="text-xs text-muted-foreground">{item.global ? "国际版" : "国内版"}</span>
                </label>
              ))}
              </div>
            </OverlayScrollArea>
          </>
        )}
        {mode === "export" && !items.length && <p className="py-4 text-center text-sm text-muted-foreground">暂无账号可导出。</p>}
        {error && <Alert variant="destructive"><AlertDescription>{error}</AlertDescription></Alert>}
        {savedPath && (
          <Alert>
            <AlertTitle>导出成功</AlertTitle>
            <AlertDescription className="space-y-2">
              <span className="block break-all font-mono text-xs">{savedPath}</span>
              <Button variant="outline" size="sm" onClick={() => { void import("@tauri-apps/plugin-opener").then(({ revealItemInDir }) => revealItemInDir(savedPath)); }}><FolderOpen />{revealLabel()}</Button>
            </AlertDescription>
          </Alert>
        )}
        <DialogFooter>
          <Button variant="outline" onClick={onClose} disabled={busy || parsing}>{savedPath ? "完成" : "取消"}</Button>
          {!savedPath && (
            <Button onClick={() => void submit()} disabled={busy || parsing || !selected.size}>
              {mode === "export" && (busy ? <Loader2 className="animate-spin" /> : <Download />)}
              {mode === "export" ? "导出勾选账号" : busy ? "导入中…" : "导入勾选账号"}
            </Button>
          )}
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
