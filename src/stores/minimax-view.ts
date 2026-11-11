import { create } from "zustand";
import type { MinimaxRegion } from "@/lib/api";

/** 区域选择与 TRAE 的国内 / 国际版同构：只影响本页展示，不会写回后端。 */
function initialRegion(): MinimaxRegion {
  try { return localStorage.getItem("wb-switch.minimax.region") === "en" ? "en" : "cn"; }
  catch { return "cn"; }
}

export const useMinimaxView = create<{ region: MinimaxRegion; setRegion: (region: MinimaxRegion) => void }>((set) => ({
  region: initialRegion(),
  setRegion: (region) => {
    try { localStorage.setItem("wb-switch.minimax.region", region); } catch { /* 本次选择仍然有效。 */ }
    set({ region });
  },
}));
