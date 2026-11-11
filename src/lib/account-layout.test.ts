import { describe,expect,it } from "vitest";
import { accountGrid,orderBasicAccounts } from "./account-layout";
describe("shared platform account layout",()=>{
 it("sorts current accounts first without mutating source or treating unknown balance as zero",()=>{const accounts=[{id:"a",name:"Alpha",capturedAt:2,active:false},{id:"b",name:"Beta",capturedAt:1,active:false}];expect(orderBasicAccounts(accounts,"active","b").map(a=>a.id)).toEqual(["b","a"]);expect(accounts[0].id).toBe("a");expect(orderBasicAccounts(accounts,"createdAt").map(a=>a.id)).toEqual(["a","b"]);expect(orderBasicAccounts(accounts,"name").map(a=>a.id)).toEqual(["a","b"]);});
 it("keeps compact and spacious grids responsive",()=>{expect(accountGrid(true)).toContain("min(100%,300px)");expect(accountGrid(false)).toContain("min(100%,340px)");});
});
