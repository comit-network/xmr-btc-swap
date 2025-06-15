import { Box, Chip, Paper, Tooltip, Typography } from "@mui/material";
import { VerifiedUser } from "@mui/icons-material";
import { ExtendedMakerStatus } from "models/apiModel";
import TruncatedText from "renderer/components/other/TruncatedText";
import {
  MoneroBitcoinExchangeRate,
  SatsAmount,
} from "renderer/components/other/Units";
import { getMarkup, satsToBtc, secondsToDays } from "utils/conversionUtils";
import { isMakerOutdated, isMakerVersionOutdated } from "utils/multiAddrUtils";
import WarningIcon from "@mui/icons-material/Warning";
import { useAppSelector } from "store/hooks";
import IdentIcon from "renderer/components/icons/IdentIcon";

/**
 * A chip that displays the markup of the maker's exchange rate compared to the market rate.
 */
function MakerMarkupChip({ maker }: { maker: ExtendedMakerStatus }) {
  const marketExchangeRate = useAppSelector((s) => s.rates?.xmrBtcRate);
  if (marketExchangeRate == null) return null;

  const makerExchangeRate = satsToBtc(maker.price);
  /** The markup of the exchange rate compared to the market rate in percent */
  const markup = getMarkup(makerExchangeRate, marketExchangeRate);

  return (
    <Tooltip title="The markup this maker charges compared to centralized markets. A lower markup means that you get more Monero for your Bitcoin.">
      <Chip label={`Markup ${markup.toFixed(2)}%`} />
    </Tooltip>
  );
}

export default function MakerInfo({ maker }: { maker: ExtendedMakerStatus }) {
  const isOutdated = isMakerOutdated(maker);

  return (
    <Box
      sx={{
        flex: 1,
        "& *": {
          lineBreak: "anywhere",
        },
        display: "flex",
        flexDirection: "column",
        gap: 1,
      }}
    >
      <Box sx={{ display: "flex", alignItems: "center", gap: 1 }}>
        <Tooltip
          title={
            "This avatar is deterministically derived from the public key of the maker"
          }
          arrow
        >
          <Box sx={{ display: "flex", alignItems: "center", gap: 1 }}>
            <IdentIcon value={maker.peerId} size={"3rem"} />
          </Box>
        </Tooltip>
        <Box>
          <Typography variant="subtitle1">
            <TruncatedText limit={16} truncateMiddle>
              {maker.peerId}
            </TruncatedText>
          </Typography>
          <Typography color="textSecondary" variant="body2">
            {maker.multiAddr}
          </Typography>
        </Box>
      </Box>
      <Box sx={{ display: "flex", flexDirection: "column" }}>
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
      <Box sx={{ display: "flex", flexWrap: "wrap", gap: 0.5 }}>
        {maker.testnet && <Chip label="Testnet" />}
        {maker.uptime && (
          <Tooltip title="A high uptime (>90%) indicates reliability. Makers with very low uptime may be unreliable and cause swaps to take longer to complete or fail entirely.">
            <Chip label={`${Math.round(maker.uptime * 100)}% uptime`} />
          </Tooltip>
        )}
        {maker.age && (
          <Chip
            label={`Went online ${Math.round(secondsToDays(maker.age))} ${
              maker.age === 1 ? "day" : "days"
            } ago`}
          />
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
        {maker.version && (
          <Tooltip title="The version of the maker's software">
            <Chip label={`v${maker.version}`} />
          </Tooltip>
        )}
        <MakerMarkupChip maker={maker} />
      </Box>
    </Box>
  );
}
