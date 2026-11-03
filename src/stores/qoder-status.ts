import { create } from "zustand";
import * as api from "@/lib/api";
import type { QoderStatus, WbVariant } from "@/lib/types";

let generation = 0;

interface QoderStatusStore {
  status: QoderStatus | null;
  error: string | null;
  refresh: () => Promise<void>;
}

/** Page and platform rail share the same status and refresh after account actions. */
export const useQoderStatus = create<QoderStatusStore>((set) => ({
  status: null,
  error: null,
  async refresh() {
    const request = ++generation;
    try {
      const status = await api.getQoderStatus();
      if (request === generation) set({ status, error: null });
    } catch (error) {
      if (request === generation) set({ error: api.asError(error) });
    }
  },
}));

/** The rail describes the selected desktop client, not its separate CLI. */
export function qoderDesktopRunning(status: QoderStatus | null, error: string | null, variant: WbVariant): boolean | undefined {
  if (!status || error) return undefined;
  return (variant === "ai" ? status.globalDesktop : status.desktop).running;
}
