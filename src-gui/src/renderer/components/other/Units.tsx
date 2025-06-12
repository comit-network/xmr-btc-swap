import { Tooltip } from "@mui/material";
import { useAppSelector, useSettings } from "store/hooks";
import { getMarkup, piconerosToXmr, satsToBtc } from "utils/conversionUtils";

type Amount = number | null | undefined;

export function AmountWithUnit({
  amount,
  unit,
  fixedPrecision,
  exchangeRate,
  parenthesisText = null,
}: {
  amount: Amount;
  unit: string;
  fixedPrecision: number;
  exchangeRate?: Amount;
  parenthesisText?: string;
}) {
  const [fetchFiatPrices, fiatCurrency] = useSettings((settings) => [
    settings.fetchFiatPrices,
    settings.fiatCurrency,
  ]);
  const title =
    fetchFiatPrices &&
    exchangeRate != null &&
    amount != null &&
    fiatCurrency != null
      ? `≈ ${(exchangeRate * amount).toFixed(2)} ${fiatCurrency}`
      : "";

  return (
    <Tooltip arrow title={title}>
      <span>
        {amount != null
          ? Number.parseFloat(amount.toFixed(fixedPrecision))
          : "?"}{" "}
        {unit}
        {parenthesisText != null ? ` (${parenthesisText})` : null}
      </span>
    </Tooltip>
  );
}

AmountWithUnit.defaultProps = {
  exchangeRate: null,
};

export function BitcoinAmount({ amount }: { amount: Amount }) {
  const btcRate = useAppSelector((state) => state.rates.btcPrice);

  return (
    <AmountWithUnit
      amount={amount}
      unit="BTC"
      fixedPrecision={6}
      exchangeRate={btcRate}
    />
  );
}

export function MoneroAmount({ amount }: { amount: Amount }) {
  const xmrRate = useAppSelector((state) => state.rates.xmrPrice);

  return (
    <AmountWithUnit
      amount={amount}
      unit="XMR"
      fixedPrecision={4}
      exchangeRate={xmrRate}
    />
  );
}

export function MoneroBitcoinExchangeRate({
  rate,
  displayMarkup = false,
}: {
  rate: Amount;
  displayMarkup?: boolean;
}) {
  const marketRate = useAppSelector((state) => state.rates?.xmrBtcRate);
  const markup =
    displayMarkup && marketRate != null
      ? `${getMarkup(rate, marketRate).toFixed(2)}% markup`
      : null;

  return (
    <AmountWithUnit
      amount={rate}
      unit="BTC/XMR"
      fixedPrecision={8}
      parenthesisText={markup}
    />
  );
}

export function MoneroBitcoinExchangeRateFromAmounts({
  satsAmount,
  piconeroAmount,
  displayMarkup = false,
}: {
  satsAmount: number;
  piconeroAmount: number;
  displayMarkup?: boolean;
}) {
  const rate = satsToBtc(satsAmount) / piconerosToXmr(piconeroAmount);

  return (
    <MoneroBitcoinExchangeRate rate={rate} displayMarkup={displayMarkup} />
  );
}

export function MoneroSatsExchangeRate({
  rate,
  displayMarkup = false,
}: {
  rate: Amount;
  displayMarkup?: boolean;
}) {
  const btc = satsToBtc(rate);

  return <MoneroBitcoinExchangeRate rate={btc} displayMarkup={displayMarkup} />;
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
