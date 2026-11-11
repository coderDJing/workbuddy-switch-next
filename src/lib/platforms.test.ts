import { describe,expect,it } from "vitest";
import { PLATFORMS } from "./platforms";
import { SUPPORTED_TOOLS } from "./supported-tools";
describe("platform registration",()=>{
  it("keeps identifiers unique and preserves the existing default platform",()=>{
    expect(new Set(PLATFORMS.map(p=>p.id)).size).toBe(PLATFORMS.length);
    expect(new Set(SUPPORTED_TOOLS.map(t=>t.id)).size).toBe(SUPPORTED_TOOLS.length);
    expect(PLATFORMS[0].id).toBe("codebuddy");
    for(const p of PLATFORMS)for(const tool of p.tools)expect(SUPPORTED_TOOLS.some(t=>t.id===tool)).toBe(true);
    expect(PLATFORMS.find(p=>p.id==="trae")?.pages.map(p=>p.key)).toEqual(["accounts", "credit-stats", "settings"]);
    expect(PLATFORMS.find(p=>p.id==="doubao")?.pages.map(p=>p.key)).toEqual(["accounts", "credit-stats", "settings"]);
    // MiniMax Code 桌面端与 CLI 共用登录态及本机会话用量。
    expect(PLATFORMS.find(p=>p.id==="minimax")?.pages.map(p=>p.key)).toEqual(["accounts", "token-stats", "credit-stats", "settings"]);
  });
});
