import { Box } from "@material-ui/core";
import { ReactNode } from "react";
import CopyableMonospaceTextBox from "renderer/components/other/CopyableMonospaceTextBox";
import BitcoinQrCode from "./BitcoinQrCode";
import InfoBox from "./InfoBox";

type Props = {
  title: string;
  address: string;
  additionalContent: ReactNode;
  icon: ReactNode;
};

export default function DepositAddressInfoBox({
  title,
  address,
  additionalContent,
  icon,
}: Props) {
  return (
    <InfoBox
      title={title}
      mainContent={<CopyableMonospaceTextBox address={address} />}
      additionalContent={
        <Box
          style={{
            display: "flex",
            flexDirection: "row",
            gap: "0.5rem",
            alignItems: "center",
          }}
        >
          <Box>{additionalContent}</Box>
          <BitcoinQrCode address={address} />
        </Box>
      }
      icon={icon}
      loading={false}
    />
  );
}
