import { Box, Chip, makeStyles, Paper, Tooltip, Typography } from "@material-ui/core";
import { VerifiedUser } from "@material-ui/icons";
import { ExtendedMakerStatus } from "models/apiModel";
import TruncatedText from "renderer/components/other/TruncatedText";
import {
  MoneroBitcoinExchangeRate,
  SatsAmount,
} from "renderer/components/other/Units";
import { getMarkup, satsToBtc, secondsToDays } from "utils/conversionUtils";
import { isMakerOutdated, isMakerVersionOutdated } from 'utils/multiAddrUtils';
import WarningIcon from '@material-ui/icons/Warning';
import { useAppSelector, useMakerVersion } from "store/hooks";
import IdentIcon from "renderer/components/icons/IdentIcon";

const useStyles = makeStyles((theme) => ({
  content: {
    flex: 1,
    "& *": {
      lineBreak: "anywhere",
    },
    display: "flex",
    flexDirection: "column",
    gap: theme.spacing(1),
  },
  chipsOuter: {
    display: "flex",
    flexWrap: "wrap",
    gap: theme.spacing(0.5),
  },
  quoteOuter: {
    display: "flex",
    flexDirection: "column",
  },
  peerIdContainer: {
    display: "flex",
    alignItems: "center",
    gap: theme.spacing(1),
  },
}));

/**
 * A chip that displays the markup of the maker's exchange rate compared to the market rate.
 */
function MakerMarkupChip({ maker }: { maker: ExtendedMakerStatus }) {
  const marketExchangeRate = useAppSelector(s => s.rates?.xmrBtcRate);
  if (marketExchangeRate == null)
    return null;

  const makerExchangeRate = satsToBtc(maker.price);
  /** The markup of the exchange rate compared to the market rate in percent */
  const markup = getMarkup(makerExchangeRate, marketExchangeRate);

  return (
    <Tooltip title="The markup this maker charges compared to centralized markets. A lower markup means that you get more Monero for your Bitcoin.">
      <Chip label={`Markup ${markup.toFixed(2)}%`} />
    </Tooltip>
  );
}

export default function MakerInfo({
  maker,
}: {
  maker: ExtendedMakerStatus;
}) {
  const classes = useStyles();
  const isOutdated = isMakerOutdated(maker);

  return (
    <Box className={classes.content}>
      <Box className={classes.peerIdContainer}>
        <Tooltip title={"This avatar is deterministically derived from the public key of the maker"} arrow>
          <Box className={classes.peerIdContainer}>
            <IdentIcon value={maker.peerId} size={"3rem"} />
          </Box>
        </Tooltip>
        <Box>
          <Typography variant="subtitle1">
            <TruncatedText limit={16} truncateMiddle>{maker.peerId}</TruncatedText>
          </Typography>
          <Typography color="textSecondary" variant="body2">
            {maker.multiAddr}
          </Typography>
        </Box>
      </Box>
      <Box className={classes.quoteOuter}>
        <Typography variant="caption">
          Exchange rate:{" "}
          <MoneroBitcoinExchangeRate rate={satsToBtc(maker.price)} />
        </Typography>
        <Typography variant="caption">
          Minimum amount: <SatsAmount amount={maker.minSwapAmount} />
        </Typography>
        <Typography variant="caption">
          Maximum amount: <SatsAmount amount={maker.maxSwapAmount} />
        </Typography>
      </Box>
      <Box className={classes.chipsOuter}>
        {maker.testnet && <Chip label="Testnet" />}
        {maker.uptime && (
          <Tooltip title="A high uptime (>90%) indicates reliability. Makers with very low uptime may be unreliable and cause swaps to take longer to complete or fail entirely.">
            <Chip label={`${Math.round(maker.uptime * 100)}% uptime`} />
          </Tooltip>
        )}
        {maker.age ? (
          <Chip
            label={`Went online ${Math.round(secondsToDays(maker.age))} ${maker.age === 1 ? "day" : "days"
              } ago`}
          />
        ) : (
          <Chip label="Discovered via rendezvous point" />
        )}
        {maker.recommended === true && (
          <Tooltip title="This maker has shown to be exceptionally reliable">
            <Chip label="Recommended" icon={<VerifiedUser />} color="primary" />
          </Tooltip>
        )}
        {isOutdated && (
          <Tooltip title="This maker is running an older version of the software. Outdated makers may be unreliable and cause swaps to take longer to complete or fail entirely.">
            <Chip label="Outdated" icon={<WarningIcon />} color="primary" />
          </Tooltip>
        )}
        <MakerMarkupChip maker={maker} />
      </Box>
    </Box >
  );
}

