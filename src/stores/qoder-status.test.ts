import { beforeEach, describe, expect, it, vi } from "vitest";
import type { QoderStatus } from "@/lib/types";

vi.mock("@/lib/api", () => ({
  getQoderStatus: vi.fn(),
  asError: (error: unknown) => String(error),
}));

import * as api from "@/lib/api";
import { qoderDesktopRunning, useQoderStatus } from "./qoder-status";

function sample(): QoderStatus {
  const base = { label: "test", installed: true, loggedIn: true, currentId: null, currentLabel: null, echoName: null };
  return {
    desktop: { ...base, target: "desktop", running: true },
    cli: { ...base, target: "cli", running: false },
    globalDesktop: { ...base, target: "global_desktop", running: false },
    globalCli: { ...base, target: "global_cli", running: true },
    bundles: [],
  };
}

describe("Qoder rail status", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    useQoderStatus.setState({ status: null, error: null });
  });

  it("uses the selected desktop edition and reports unknown on failed or missing data", () => {
    expect(qoderDesktopRunning(sample(), null, "cn")).toBe(true);
    expect(qoderDesktopRunning(sample(), null, "ai")).toBe(false);
    expect(qoderDesktopRunning(null, null, "cn")).toBeUndefined();
    expect(qoderDesktopRunning(sample(), "failed", "cn")).toBeUndefined();
  });

  it("does not let an older response overwrite a refresh after an account action", async () => {
    let resolveOlder!: (status: QoderStatus) => void;
    vi.mocked(api.getQoderStatus)
      .mockReturnValueOnce(new Promise((resolve) => { resolveOlder = resolve; }))
      .mockResolvedValueOnce({ ...sample(), desktop: { ...sample().desktop, running: false } });
    const older = useQoderStatus.getState().refresh();
    await useQoderStatus.getState().refresh();
    resolveOlder(sample());
    await older;
    expect(useQoderStatus.getState().status?.desktop.running).toBe(false);
  });
});
