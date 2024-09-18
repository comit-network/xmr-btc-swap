import MoneroIcon from "renderer/components/icons/MoneroIcon";
import { isTestnet } from "store/config";
import { getMoneroTxExplorerUrl } from "utils/conversionUtils";
import TransactionInfoBox, {
  TransactionInfoBoxProps,
} from "./TransactionInfoBox";

export default function MoneroTransactionInfoBox({
  txId,
  ...props
}: Omit<TransactionInfoBoxProps, "icon" | "explorerUrlCreator">) {
  return (
    <TransactionInfoBox
      txId={txId}
      explorerUrlCreator={(txid) => getMoneroTxExplorerUrl(txid, isTestnet())}
      icon={<MoneroIcon />}
      {...props}
    />
  );
}
