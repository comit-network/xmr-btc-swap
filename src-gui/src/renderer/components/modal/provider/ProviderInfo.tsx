import { Box, Chip, makeStyles, Tooltip, Typography } from "@material-ui/core";
import { VerifiedUser } from "@material-ui/icons";
import { ExtendedProviderStatus } from "models/apiModel";
import TruncatedText from "renderer/components/other/TruncatedText";
import {
  MoneroBitcoinExchangeRate,
  SatsAmount,
} from "renderer/components/other/Units";
import { satsToBtc, secondsToDays } from "utils/conversionUtils";
import { isProviderOutdated } from 'utils/multiAddrUtils';
import WarningIcon from '@material-ui/icons/Warning';
import { useAppSelector } from "store/hooks";

const useStyles = makeStyles((theme) => ({
  content: {
    flex: 1,
    "& *": {
      lineBreak: "anywhere",
    },
  },
  chipsOuter: {
    display: "flex",
    marginTop: theme.spacing(1),
    gap: theme.spacing(0.5),
    flexWrap: "wrap",
  },
}));

function ProviderSpreadChip({ provider }: { provider: ExtendedProviderStatus }) {
  const xmrBtcPrice = useAppSelector(s => s.rates?.xmrBtcRate);

  if (xmrBtcPrice === null) {
    return null;
  }

  const providerPrice = satsToBtc(provider.price);
  const spread = ((providerPrice - xmrBtcPrice) / xmrBtcPrice) * 100;

  return (
    <Tooltip title="The spread is the difference between the provider's exchange rate and the market rate. A high spread indicates that the provider is charging more than the market rate.">
      <Chip label={`Spread: ${spread.toFixed(2)} %`} />
    </Tooltip>
  );

}

export default function ProviderInfo({
  provider,
}: {
  provider: ExtendedProviderStatus;
}) {
  const classes = useStyles();
  const isOutdated = isProviderOutdated(provider);

  return (
    <Box className={classes.content}>
      <Typography color="textSecondary" gutterBottom>
        Swap Provider
      </Typography>
      <Typography variant="h5" component="h2">
        {provider.multiAddr}
      </Typography>
      <Typography color="textSecondary" gutterBottom>
        <TruncatedText>{provider.peerId}</TruncatedText>
      </Typography>
      <Typography variant="caption">
        Exchange rate:{" "}
        <MoneroBitcoinExchangeRate rate={satsToBtc(provider.price)} />
        <br />
        Minimum swap amount: <SatsAmount amount={provider.minSwapAmount} />
        <br />
        Maximum swap amount: <SatsAmount amount={provider.maxSwapAmount} />
      </Typography>
      <Box className={classes.chipsOuter}>
        <Chip label={provider.testnet ? "Testnet" : "Mainnet"} />
        {provider.uptime && (
          <Tooltip title="A high uptime indicates reliability. Providers with low uptime may be unreliable and cause swaps to take longer to complete or fail entirely.">
            <Chip label={`${Math.round(provider.uptime * 100)} % uptime`} />
          </Tooltip>
        )}
        {provider.age ? (
          <Chip
            label={`Went online ${Math.round(secondsToDays(provider.age))} ${
              provider.age === 1 ? "day" : "days"
            } ago`}
          />
        ) : (
          <Chip label="Discovered via rendezvous point" />
        )}
        {provider.recommended === true && (
          <Tooltip title="This provider has shown to be exceptionally reliable">
            <Chip label="Recommended" icon={<VerifiedUser />} color="primary" />
          </Tooltip>
        )}
        {isOutdated && (
          <Tooltip title="This provider is running an outdated version of the software. Outdated providers may be unreliable and cause swaps to take longer to complete or fail entirely.">
            <Chip label="Outdated" icon={<WarningIcon />} color="primary" />
          </Tooltip>
        )}
        <ProviderSpreadChip provider={provider} />
      </Box>
    </Box>
  );
}
