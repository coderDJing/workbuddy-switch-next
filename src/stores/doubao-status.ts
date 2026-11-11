import { create } from "zustand";
import * as api from "@/lib/api";

let inFlight: Promise<void> | undefined;
export const useDoubaoStatus = create<{
  status: api.DoubaoStatus | null;
  error: string | null;
  refresh: (force?: boolean) => Promise<void>;
}>((set) => ({
  status: null,
  error: null,
  refresh(force = false): Promise<void> {
    // A post-mutation refresh must not reuse a response begun before the write.
    if (inFlight) return force ? inFlight.then(() => useDoubaoStatus.getState().refresh()) : inFlight;
    inFlight = api.getDoubaoStatus().then(status => { set({ status, error: null }); })
      .catch(error => { set({ error: api.asError(error) }); })
      .finally(() => { inFlight = undefined; });
    return inFlight;
  },
}));
