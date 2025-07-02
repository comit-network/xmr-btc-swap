import { Box, Button, Typography } from "@mui/material";
import SendIcon from "@mui/icons-material/Send";
import { useState } from "react";
import { SatsAmount } from "renderer/components/other/Units";
import { useAppSelector } from "store/hooks";
import BitcoinIcon from "../../icons/BitcoinIcon";
import InfoBox from "../swap/swap/components/InfoBox";
import WithdrawDialog from "../../modal/wallet/WithdrawDialog";
import WalletRefreshButton from "./WalletRefreshButton";

export default function WithdrawWidget() {
  const walletBalance = useAppSelector((state) => state.rpc.state.balance);
  const [showDialog, setShowDialog] = useState(false);

  function onShowDialog() {
    setShowDialog(true);
  }

  return (
    <>
      <InfoBox
        title={
          <Box sx={{ alignItems: "center", display: "flex", gap: 0.5 }}>
            Wallet Balance
            <WalletRefreshButton />
          </Box>
        }
        mainContent={
          <Typography variant="h5">
            <SatsAmount amount={walletBalance} />
          </Typography>
        }
        icon={<BitcoinIcon />}
        additionalContent={
          <Button
            variant="contained"
            color="primary"
            endIcon={<SendIcon />}
            size="large"
            onClick={onShowDialog}
            disabled={walletBalance === null || walletBalance <= 0}
          >
            Withdraw
          </Button>
        }
        loading={false}
      />
      <WithdrawDialog open={showDialog} onClose={() => setShowDialog(false)} />
    </>
  );
}
