import { useState } from 'react';
import { Box, makeStyles, TextField, Typography } from '@material-ui/core';
import { SwapStateWaitingForBtcDeposit } from 'models/storeModel';
import { useAppSelector } from 'store/hooks';
import { satsToBtc } from 'utils/conversionUtils';
import { MoneroAmount } from '../../../../other/Units';

const MONERO_FEE = 0.000016;

const useStyles = makeStyles((theme) => ({
  outer: {
    display: 'flex',
    alignItems: 'center',
    gap: theme.spacing(1),
  },
  textField: {
    '& input::-webkit-outer-spin-button, & input::-webkit-inner-spin-button': {
      display: 'none',
    },
    '& input[type=number]': {
      MozAppearance: 'textfield',
    },
    maxWidth: theme.spacing(16),
  },
}));

function calcBtcAmountWithoutFees(amount: number, fees: number) {
  return amount - fees;
}

export default function DepositAmountHelper({
  state,
}: {
  state: SwapStateWaitingForBtcDeposit;
}) {
  const classes = useStyles();
  const [amount, setAmount] = useState(state.minDeposit);
  const bitcoinBalance = useAppSelector((s) => s.rpc.state.balance) || 0;

  function getTotalAmountAfterDeposit() {
    return amount + satsToBtc(bitcoinBalance);
  }

  function hasError() {
    return (
      amount < state.minDeposit ||
      getTotalAmountAfterDeposit() > state.maximumAmount
    );
  }

  function calcXMRAmount(): number | null {
    if (Number.isNaN(amount)) return null;
    if (hasError()) return null;
    if (state.price == null) return null;

    console.log(
      `Calculating calcBtcAmountWithoutFees(${getTotalAmountAfterDeposit()}, ${
        state.minBitcoinLockTxFee
      }) / ${state.price} - ${MONERO_FEE}`,
    );

    return (
      calcBtcAmountWithoutFees(
        getTotalAmountAfterDeposit(),
        state.minBitcoinLockTxFee,
      ) /
        state.price -
      MONERO_FEE
    );
  }

  return (
    <Box className={classes.outer}>
      <Typography variant="subtitle2">
        Depositing {bitcoinBalance > 0 && <>another</>}
      </Typography>
      <TextField
        error={hasError()}
        value={amount}
        onChange={(e) => setAmount(parseFloat(e.target.value))}
        size="small"
        type="number"
        className={classes.textField}
      />
      <Typography variant="subtitle2">
        BTC will give you approximately{' '}
        <MoneroAmount amount={calcXMRAmount()} />.
      </Typography>
    </Box>
  );
}
