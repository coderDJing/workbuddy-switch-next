import { create } from "zustand";
import type { QoderTarget, WbVariant } from "@/lib/types";

/** Qoder 的选择与 WorkBuddy 独立，页面切换时保留本次选择。 */
export const useQoderView = create<{ variant: WbVariant; setVariant: (variant: WbVariant) => void }>((set) => ({
  variant: "cn",
  setVariant: (variant) => set({ variant }),
}));

const TARGETS: Record<WbVariant, readonly QoderTarget[]> = {
  cn: ["desktop", "cli"],
  ai: ["global_desktop", "global_cli"],
};

export function qoderTargets(variant: WbVariant): readonly QoderTarget[] {
  return TARGETS[variant];
}
