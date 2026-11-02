import { QoderAccountsView } from "@/components/qoder-accounts-view";
import { useState } from "react";

/** Qoder 平台的「账号管理」：与 WorkBuddy / CodeBuddy 账号页平级。卡片疏密沿用账号页的紧凑偏好。 */
export default function QoderPage() {
  const [compact, setCompact] = useState(() => {
    try { return localStorage.getItem("wb-switch.compact") !== "0"; }
    catch { return true; }
  });
  function toggleCompact() {
    setCompact((value) => {
      const next = !value;
      try { localStorage.setItem("wb-switch.compact", next ? "1" : "0"); } catch { /* 存储不可用时只影响本次 */ }
      return next;
    });
  }
  return <QoderAccountsView compact={compact} onToggleCompact={toggleCompact} />;
}
