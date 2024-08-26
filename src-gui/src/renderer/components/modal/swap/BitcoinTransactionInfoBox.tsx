import { ReactNode } from "react";
import BitcoinIcon from "renderer/components/icons/BitcoinIcon";
import { isTestnet } from "store/config";
import { getBitcoinTxExplorerUrl } from "utils/conversionUtils";
import TransactionInfoBox from "./TransactionInfoBox";

type Props = {
  title: string;
  txId: string;
  additionalContent: ReactNode;
  loading: boolean;
};

export default function BitcoinTransactionInfoBox({ txId, ...props }: Props) {
  const explorerUrl = getBitcoinTxExplorerUrl(txId, isTestnet());

  return (
    <TransactionInfoBox
      txId={txId}
      explorerUrl={explorerUrl}
      icon={<BitcoinIcon />}
      {...props}
    />
  );
}
