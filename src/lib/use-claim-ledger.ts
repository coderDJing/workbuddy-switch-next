import { useCallback, useEffect, useRef, useState } from "react";

import * as api from "@/lib/api";
import { useVisibleInterval } from "@/lib/use-visible-interval";

/** 记忆刷新间隔：领取是分钟级动作，一分钟足够反映自动周期写入的结果。 */
const CLAIM_LEDGER_REFRESH_MS = 60_000;

/**
 * 读取今日领取记忆。只在主窗口可见时轮询（领取本身由后端完成，
 * 这里不需要高频），窗口重新获得焦点时立即刷新一次。
 *
 * 读取失败按「今天没有记忆」处理：按钮回到普通态，绝不因为记忆文件损坏
 * 挡住用户手动领取。
 */
export function useClaimLedger(enabled = true): {
  ledger: api.ClaimLedger | null;
  reload: () => Promise<void>;
} {
  const [ledger, setLedger] = useState<api.ClaimLedger | null>(null);
  const alive = useRef(true);

  const reload = useCallback(async () => {
    if (!enabled) return;
    try {
      const next = await api.getClaimLedger();
      if (alive.current) setLedger(next);
    } catch {
      /* 记忆读取失败：保持原值，按钮按未知处理 */
    }
  }, [enabled]);

  useEffect(() => {
    alive.current = true;
    return () => {
      alive.current = false;
    };
  }, []);

  useEffect(() => {
    const onFocus = () => void reload();
    window.addEventListener("focus", onFocus);
    return () => window.removeEventListener("focus", onFocus);
  }, [reload]);

  useVisibleInterval(() => {
    void reload();
  }, CLAIM_LEDGER_REFRESH_MS, enabled);

  return { ledger, reload };
}
