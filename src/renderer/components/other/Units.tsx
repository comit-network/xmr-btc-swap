import { piconerosToXmr, satsToBtc } from 'utils/conversionUtils';
import { Tooltip } from '@material-ui/core';
import { useAppSelector } from 'store/hooks';

type Amount = number | null | undefined;

export function AmountWithUnit({
  amount,
  unit,
  fixedPrecision,
  dollarRate,
}: {
  amount: Amount;
  unit: string;
  fixedPrecision: number;
  dollarRate?: Amount;
}) {
  return (
    <Tooltip
      arrow
      title={
        dollarRate != null && amount != null
          ? `≈ $${(dollarRate * amount).toFixed(2)}`
          : ''
      }
    >
      <span>
        {amount != null
          ? Number.parseFloat(amount.toFixed(fixedPrecision))
          : '?'}{' '}
        {unit}
      </span>
    </Tooltip>
  );
}

AmountWithUnit.defaultProps = {
  dollarRate: null,
};

export function BitcoinAmount({ amount }: { amount: Amount }) {
  const btcUsdRate = useAppSelector((state) => state.rates.btcPrice);

  return (
    <AmountWithUnit
      amount={amount}
      unit="BTC"
      fixedPrecision={6}
      dollarRate={btcUsdRate}
    />
  );
}

export function MoneroAmount({ amount }: { amount: Amount }) {
  const xmrUsdRate = useAppSelector((state) => state.rates.xmrPrice);

  return (
    <AmountWithUnit
      amount={amount}
      unit="XMR"
      fixedPrecision={4}
      dollarRate={xmrUsdRate}
    />
  );
}

export function MoneroBitcoinExchangeRate({ rate }: { rate: Amount }) {
  return <AmountWithUnit amount={rate} unit="BTC/XMR" fixedPrecision={8} />;
}

export function SatsAmount({ amount }: { amount: Amount }) {
  const btcAmount = amount == null ? null : satsToBtc(amount);
  return <BitcoinAmount amount={btcAmount} />;
}

export function PiconeroAmount({ amount }: { amount: Amount }) {
  return (
    <MoneroAmount amount={amount == null ? null : piconerosToXmr(amount)} />
  );
}
