import { describe, expect, it } from "vitest";
import { qoderTargets, useQoderView } from "./qoder-view";

describe("Qoder version selection", () => {
  it("keeps desktop and CLI targets within their own version", () => {
    expect(qoderTargets("cn")).toEqual(["desktop", "cli"]);
    expect(qoderTargets("ai")).toEqual(["global_desktop", "global_cli"]);
  });

  it("keeps the selection when the page is remounted", () => {
    useQoderView.getState().setVariant("ai");
    expect(useQoderView.getState().variant).toBe("ai");
    useQoderView.getState().setVariant("cn");
  });
});
