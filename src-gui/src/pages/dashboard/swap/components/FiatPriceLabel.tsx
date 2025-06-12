import { useSettings } from "store/hooks";
import { currencySymbol } from "utils/formatUtils";
import { Typography } from "@mui/material";
import { useAppSelector } from "store/hooks";

export enum Currency {
    BTC = "BTC",
    XMR = "XMR"
}

interface FiatPriceLabelProps {
    amount: number;
    originalCurrency: Currency;
    gridColumn: string;
    gridRow: string;
  }
  
export default function FiatPriceLabel({ amount, originalCurrency, gridColumn, gridRow }: FiatPriceLabelProps) {
    const btcPrice = useAppSelector((state) => state.rates.btcPrice);
    const xmrPrice = useAppSelector((state) => state.rates.xmrPrice);
    const fiatCurrency = useSettings((s) => s.fiatCurrency);
    const fetchFiatPrices = useSettings((s) => s.fetchFiatPrices);

    if (!(fetchFiatPrices && fiatCurrency)) return null;

    const fiatSymbol = currencySymbol(fiatCurrency) || "";
    const fiatRate = originalCurrency === Currency.BTC ? btcPrice : xmrPrice;
    const fiatAmount = Number((amount * fiatRate).toFixed(2));
  
    return (
      <Typography
        variant="caption"
        sx={{
          gridColumn,
          gridRow,
        }}
      >
        ({fiatAmount.toFixed(2)} {fiatSymbol})
      </Typography>
    );
  }