import { describe, expect, it } from "vitest";
import { CLI_TARGET_LABELS, cliQueryCommands, cliSkillGuide, type CliTarget } from "./ai-cli-guide";
import operations from "./operations.json";

describe("CLI guides", () => {
  it("discovers every client and keeps platform statistics free of region flags", () => {
    expect(Object.keys(CLI_TARGET_LABELS)).toHaveLength(9);
    for (const target of Object.keys(CLI_TARGET_LABELS) as CliTarget[]) {
      const queries = cliQueryCommands(target, "all");
      expect(queries.some(q => / operations(?: |$)/.test(q.command))).toBe(true);
      for (const query of queries) {
        if (!target.startsWith("qoder-") && !target.startsWith("trae-") && !(target === "minimax-desktop" && query.command.includes(" credit-stats ")) && / (token-stats|credit-stats) /.test(query.command)) expect(query.command).not.toContain("--variant");
        if (target === "vscode" || target === "jetbrains") {
          expect(query.command).not.toContain("--variant ai");
          expect(query.command).not.toContain("--variant all");
        }
      }
      const guide = cliSkillGuide(target, "all");
      expect(guide).toContain("operations");
      expect(guide).toContain("--output-file");
      if (["workbuddy", "codebuddy-ide", "vscode", "jetbrains"].includes(target)) {
        expect(guide).toContain("--close-running --relaunch");
        expect(guide).not.toContain(" automation get ");
      }
    }
  });
  it("shares valid backend routes and typed operation contracts", () => {
    for (const operation of Object.values(operations)) {
      expect(["GET", "POST"]).toContain(operation.method);
      expect(operation.path).toMatch(/^\/api\//);
      expect(operation.parameters).toBeTypeOf("object");
    }
    expect(operations.update_account_display.parameters.patch.required).toBe(true);
    expect(operations.switch_account.parameters.restart.required).toBe(false);
    expect(operations.get_status.parameters.variant.enum).toEqual(["cn", "ai"]);
  });
});
