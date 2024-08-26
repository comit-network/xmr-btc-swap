import { Box, Chip, makeStyles, Tooltip, Typography } from "@material-ui/core";
import { VerifiedUser } from "@material-ui/icons";
import { ExtendedProviderStatus } from "models/apiModel";
import {
  MoneroBitcoinExchangeRate,
  SatsAmount,
} from "renderer/components/other/Units";
import { satsToBtc, secondsToDays } from "utils/conversionUtils";

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

export default function ProviderInfo({
  provider,
}: {
  provider: ExtendedProviderStatus;
}) {
  const classes = useStyles();

  return (
    <Box className={classes.content}>
      <Typography color="textSecondary" gutterBottom>
        Swap Provider
      </Typography>
      <Typography variant="h5" component="h2">
        {provider.multiAddr}
      </Typography>
      <Typography color="textSecondary" gutterBottom>
        {provider.peerId.substring(0, 8)}...{provider.peerId.slice(-8)}
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
      </Box>
    </Box>
  );
}
