import { describe, expect, it } from "vitest";
import type { MinimaxSnapshot } from "./api";
import { observedMinimaxUsage } from "./minimax-statistics";
const snapshot = (at:number,used:number):MinimaxSnapshot => ({accountId:"a",name:"a",region:"cn",at,credit:{available:true,unit:"credits",unlimited:false,remaining:100-used,total:100,used,checkedAt:at,packs:[{name:"package",remaining:100-used,total:100,used,expiresAt:9999}]}});
describe("MiniMax observed credits",()=>{
  it("compares unchanged packages, independent accounts and monotonic counters",()=>{
    expect(observedMinimaxUsage([snapshot(100,10),snapshot(200,30)],0,300).total).toBe(20);
    expect(observedMinimaxUsage([snapshot(100,30),snapshot(200,10)],0,300).total).toBeNull();
    expect(observedMinimaxUsage([snapshot(100,10),{...snapshot(200,30),accountId:"other"}],0,300).total).toBeNull();
  });
  it("starts a baseline after claims, expiry and incomplete queries",()=>{
    for (const change of [{total:500},{packs:[]},{error:"partial"},{used:null},{available:false}]) {
      const after=snapshot(200,30);Object.assign(after.credit,change);
      expect(observedMinimaxUsage([snapshot(100,10),after],0,300).total).toBeNull();
    }
    expect(observedMinimaxUsage([snapshot(100,10),snapshot(200,30)],150,300).total).toBeNull();
  });
});
