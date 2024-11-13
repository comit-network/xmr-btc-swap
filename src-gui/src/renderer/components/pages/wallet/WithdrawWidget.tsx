import { Box, Button, makeStyles, Typography } from "@material-ui/core";
import SendIcon from "@material-ui/icons/Send";
import { useState } from "react";
import { SatsAmount } from "renderer/components/other/Units";
import { useAppSelector } from "store/hooks";
import BitcoinIcon from "../../icons/BitcoinIcon";
import InfoBox from "../../modal/swap/InfoBox";
import WithdrawDialog from "../../modal/wallet/WithdrawDialog";
import WalletRefreshButton from "./WalletRefreshButton";

const useStyles = makeStyles((theme) => ({
  title: {
    alignItems: "center",
    display: "flex",
    gap: theme.spacing(0.5),
  },
}));

export default function WithdrawWidget() {
  const classes = useStyles();
  const walletBalance = useAppSelector((state) => state.rpc.state.balance);
  const [showDialog, setShowDialog] = useState(false);

  function onShowDialog() {
    setShowDialog(true);
  }

  return (
    <>
      <InfoBox
        title={
          <Box className={classes.title}>
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
