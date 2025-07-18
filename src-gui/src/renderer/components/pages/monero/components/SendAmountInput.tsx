import { Box, Button, Card, Grow, Typography } from "@mui/material";
import NumberInput from "renderer/components/inputs/NumberInput";
import SwapVertIcon from "@mui/icons-material/SwapVert";
import { useTheme } from "@mui/material/styles";
import { piconerosToXmr } from "../../../../../utils/conversionUtils";
import { MoneroAmount } from "renderer/components/other/Units";

interface SendAmountInputProps {
  balance: {
    unlocked_balance: string;
  };
  amount: string;
  onAmountChange: (amount: string) => void;
  onMaxClicked?: () => void;
  onMaxToggled?: () => void;
  currency: string;
  onCurrencyChange: (currency: string) => void;
  fiatCurrency: string;
  xmrPrice: number;
  showFiatRate: boolean;
  disabled?: boolean;
}

export default function SendAmountInput({
  balance,
  amount,
  currency,
  onCurrencyChange,
  onAmountChange,
  onMaxClicked,
  onMaxToggled,
  fiatCurrency,
  xmrPrice,
  showFiatRate,
  disabled = false,
}: SendAmountInputProps) {
  const theme = useTheme();

  const isMaxSelected = amount === "<MAX>";

  // Calculate secondary amount for display
  const secondaryAmount = (() => {
    if (isMaxSelected) {
      return "All available funds";
    }

    if (!amount || amount === "" || isNaN(parseFloat(amount))) {
      return "0.00";
    }

    const primaryValue = parseFloat(amount);
    if (currency === "XMR") {
      // Primary is XMR, secondary is USD
      return (primaryValue * xmrPrice).toFixed(2);
    } else {
      // Primary is USD, secondary is XMR
      return (primaryValue / xmrPrice).toFixed(3);
    }
  })();

  const handleMaxAmount = () => {
    if (disabled) return;

    if (onMaxToggled) {
      onMaxToggled();
    } else if (onMaxClicked) {
      onMaxClicked();
    } else {
      // Fallback to old behavior if no callback provided
      if (
        balance?.unlocked_balance !== undefined &&
        balance?.unlocked_balance !== null
      ) {
        // TODO: We need to use a real fee here and call sweep(...) instead of just subtracting a fixed amount
        const unlocked = parseFloat(balance.unlocked_balance);
        const maxAmountXmr = piconerosToXmr(unlocked - 10000000000); // Subtract ~0.01 XMR for fees

        if (currency === "XMR") {
          onAmountChange(Math.max(0, maxAmountXmr).toString());
        } else {
          // Convert to USD for display
          const maxAmountUsd = maxAmountXmr * xmrPrice;
          onAmountChange(Math.max(0, maxAmountUsd).toString());
        }
      }
    }
  };

  const handleMaxTextClick = () => {
    if (disabled) return;
    if (isMaxSelected && onMaxToggled) {
      onMaxToggled();
    }
  };

  const handleCurrencySwap = () => {
    if (!isMaxSelected && !disabled) {
      onCurrencyChange(currency === "XMR" ? fiatCurrency : "XMR");
    }
  };

  const isAmountTooHigh =
    !isMaxSelected &&
    (currency === "XMR"
      ? parseFloat(amount) >
        piconerosToXmr(parseFloat(balance.unlocked_balance))
      : parseFloat(amount) / xmrPrice >
        piconerosToXmr(parseFloat(balance.unlocked_balance)));

  return (
    <Card
      elevation={0}
      tabIndex={0}
      sx={{
        position: "relative",
        display: "flex",
        flexDirection: "column",
        alignItems: "center",
        justifyContent: "center",
        border: `1px solid ${theme.palette.grey[800]}`,
        width: "100%",
        height: 250,
        opacity: disabled ? 0.6 : 1,
        pointerEvents: disabled ? "none" : "auto",
      }}
    >
      <Box
        sx={{ display: "flex", flexDirection: "column", alignItems: "center" }}
      >
        {isAmountTooHigh && (
          <Grow
            in
            style={{ transitionDelay: isAmountTooHigh ? "100ms" : "0ms" }}
          >
            <Typography variant="caption" align="center" color="error">
              You don't have enough
              <br /> unlocked balance to send this amount.
            </Typography>
          </Grow>
        )}
        <Box sx={{ display: "flex", alignItems: "baseline", gap: 1 }}>
          {isMaxSelected ? (
            <Typography
              variant="h3"
              onClick={handleMaxTextClick}
              sx={{
                fontWeight: 600,
                color: "primary.main",
                cursor: disabled ? "default" : "pointer",
                userSelect: "none",
                "&:hover": {
                  opacity: disabled ? 1 : 0.8,
                },
              }}
              title={disabled ? "" : "Click to edit amount"}
            >
              &lt;MAX&gt;
            </Typography>
          ) : (
            <>
              <NumberInput
                value={amount}
                onChange={disabled ? () => {} : onAmountChange}
                placeholder={currency === "XMR" ? "0.000" : "0.00"}
                fontSize="3em"
                fontWeight={600}
                minWidth={60}
                step={currency === "XMR" ? 0.001 : 0.01}
                largeStep={currency === "XMR" ? 0.1 : 10}
              />
              <Typography variant="h4" color="text.secondary">
                {currency}
              </Typography>
            </>
          )}
        </Box>
        {showFiatRate && (
          <Box sx={{ display: "flex", alignItems: "center", gap: 0.5 }}>
            <SwapVertIcon
              onClick={handleCurrencySwap}
              sx={{
                cursor: isMaxSelected || disabled ? "default" : "pointer",
                opacity: isMaxSelected || disabled ? 0.5 : 1,
              }}
            />
            <Typography color="text.secondary">
              {secondaryAmount}{" "}
              {isMaxSelected ? "" : currency === "XMR" ? fiatCurrency : "XMR"}
            </Typography>
          </Box>
        )}
      </Box>

      <Box
        sx={{
          display: "flex",
          alignItems: "center",
          width: "100%",
          justifyContent: "center",
          gap: 1.5,
          position: "absolute",
          bottom: 12,
          left: 0,
        }}
      >
        <Typography color="text.secondary">Available</Typography>
        <Box sx={{ display: "flex", alignItems: "baseline", gap: 0.5 }}>
          <Typography color="text.primary">
            <MoneroAmount
              amount={piconerosToXmr(parseFloat(balance.unlocked_balance))}
            />
          </Typography>
          <Typography color="text.secondary">XMR</Typography>
        </Box>
        <Button
          variant={isMaxSelected ? "contained" : "secondary"}
          size="tiny"
          onClick={handleMaxAmount}
          disabled={disabled}
        >
          Max
        </Button>
      </Box>
    </Card>
  );
}
