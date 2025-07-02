import BitcoinIcon from "renderer/components/icons/BitcoinIcon";
import { isTestnet } from "store/config";
import { getBitcoinTxExplorerUrl } from "utils/conversionUtils";
import TransactionInfoBox, {
  TransactionInfoBoxProps,
} from "./TransactionInfoBox";

export default function BitcoinTransactionInfoBox({
  txId,
  ...props
}: Omit<TransactionInfoBoxProps, "icon" | "explorerUrlCreator">) {
  return (
    <TransactionInfoBox
      txId={txId}
      explorerUrlCreator={(txId) => getBitcoinTxExplorerUrl(txId, isTestnet())}
      icon={<BitcoinIcon />}
      {...props}
    />
  );
}
