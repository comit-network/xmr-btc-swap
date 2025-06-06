import { Link, Typography } from "@mui/material";
import { ReactNode } from "react";
import InfoBox from "./InfoBox";

export type TransactionInfoBoxProps = {
  title: string;
  txId: string | null;
  explorerUrlCreator: ((txId: string) => string) | null;
  additionalContent: ReactNode;
  loading: boolean;
  icon: JSX.Element;
};

export default function TransactionInfoBox({
  title,
  txId,
  additionalContent,
  icon,
  loading,
  explorerUrlCreator,
}: TransactionInfoBoxProps) {
  return (
    <InfoBox
      title={title}
      mainContent={
        <Typography variant="h5">
          {txId ?? "Transaction ID not available"}
        </Typography>
      }
      loading={loading}
      additionalContent={
        <>
          <Typography variant="subtitle2">{additionalContent}</Typography>
          {explorerUrlCreator != null &&
            txId != null && ( // Only show the link if the txId is not null and we have a creator for the explorer URL
              <Typography variant="body1">
                <Link href={explorerUrlCreator(txId)} target="_blank">
                  View on explorer
                </Link>
              </Typography>
            )}
        </>
      }
      icon={icon}
    />
  );
}
