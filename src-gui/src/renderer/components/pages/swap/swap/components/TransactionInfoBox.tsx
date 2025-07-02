import { Box, Link, Typography } from "@mui/material";
import { ReactNode } from "react";
import InfoBox from "./InfoBox";
import TruncatedText from "renderer/components/other/TruncatedText";

export type TransactionInfoBoxProps = {
  title: string;
  txId: string | null;
  explorerUrlCreator: ((txId: string) => string) | null;
  additionalContent: ReactNode;
  loading: boolean;
  icon: ReactNode;
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
          <TruncatedText truncateMiddle limit={40}>
            {txId ?? "Transaction ID not available"}
          </TruncatedText>
        </Typography>
      }
      loading={loading}
      additionalContent={
        <Box sx={{ display: "flex", flexDirection: "column", gap: 1 }}>
          <Typography variant="subtitle2">{additionalContent}</Typography>
          {explorerUrlCreator != null &&
            txId != null && ( // Only show the link if the txId is not null and we have a creator for the explorer URL
              <Typography variant="body1">
                <Link href={explorerUrlCreator(txId)} target="_blank">
                  View on explorer
                </Link>
              </Typography>
            )}
        </Box>
      }
      icon={icon}
    />
  );
}
