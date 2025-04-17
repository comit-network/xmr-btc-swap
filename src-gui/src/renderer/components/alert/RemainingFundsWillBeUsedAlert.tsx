import { Box, makeStyles } from "@material-ui/core";
import { Alert } from "@material-ui/lab";
import { useAppSelector } from "store/hooks";
import { SatsAmount } from "../other/Units";
import WalletRefreshButton from "../pages/wallet/WalletRefreshButton";

const useStyles = makeStyles((theme) => ({
    outer: {
        paddingBottom: theme.spacing(1),
    },
}));

export default function RemainingFundsWillBeUsedAlert() {
    const classes = useStyles();
    const balance = useAppSelector((s) => s.rpc.state.balance);

    if (balance == null || balance <= 0) {
        return <></>;
    }

    return (
        <Box className={classes.outer}>
            <Alert
                severity="warning"
                action={<WalletRefreshButton />}
                variant="filled"
            >
                The remaining funds of <SatsAmount amount={balance} /> in the wallet
                will be used for the next swap
            </Alert>
        </Box>
    );
}
