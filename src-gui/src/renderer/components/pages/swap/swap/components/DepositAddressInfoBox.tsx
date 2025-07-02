import { Box } from "@mui/material";
import { ReactNode } from "react";
import ActionableMonospaceTextBox from "renderer/components/other/ActionableMonospaceTextBox";
import InfoBox from "./InfoBox";
import { Alert } from "@mui/material";

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
      mainContent={
        <ActionableMonospaceTextBox
          content={address}
          displayCopyIcon={true}
          enableQrCode={true}
        />
      }
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
        </Box>
      }
      icon={icon}
      loading={false}
    />
  );
}
