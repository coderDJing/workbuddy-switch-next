import { create } from "zustand";
import type { WbVariant } from "@/lib/types";

function initialVariant(): WbVariant {
  try { return localStorage.getItem("wb-switch.trae.variant") === "ai" ? "ai" : "cn"; }
  catch { return "cn"; }
}

export const useTraeView = create<{ variant: WbVariant; setVariant: (variant: WbVariant) => void }>((set) => ({
  variant: initialVariant(),
  setVariant: (variant) => {
    try { localStorage.setItem("wb-switch.trae.variant", variant); } catch { /* 本次选择仍然有效。 */ }
    set({ variant });
  },
}));
