import { QoderAccountsView } from "@/components/qoder-accounts-view";
import { useAccountDensity } from "@/lib/account-layout";
export default function QoderPage(){const {compact,toggleCompact}=useAccountDensity();return <QoderAccountsView compact={compact} onToggleCompact={toggleCompact}/>;}
