import { ReactNode } from "react";
import MoneroIcon from "renderer/components/icons/MoneroIcon";
import { isTestnet } from "store/config";
import { getMoneroTxExplorerUrl } from "utils/conversionUtils";
import TransactionInfoBox from "./TransactionInfoBox";

type Props = {
  title: string;
  txId: string;
  additionalContent: ReactNode;
  loading: boolean;
};

export default function MoneroTransactionInfoBox({ txId, ...props }: Props) {
  const explorerUrl = getMoneroTxExplorerUrl(txId, isTestnet());

  return (
    <TransactionInfoBox
      txId={txId}
      explorerUrl={explorerUrl}
      icon={<MoneroIcon />}
      {...props}
    />
  );
}
