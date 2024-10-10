import {
  Table,
  TableBody,
  TableCell,
  TableContainer,
  TableRow,
  Typography,
  IconButton,
  Box,
  makeStyles,
  Tooltip,
  Switch,
} from "@material-ui/core";
import InfoBox from "renderer/components/modal/swap/InfoBox";
import {
  resetSettings,
  setElectrumRpcUrl,
  setMoneroNodeUrl,
} from "store/features/settingsSlice";
import { useAppDispatch, useSettings } from "store/hooks";
import ValidatedTextField from "renderer/components/other/ValidatedTextField";
import RefreshIcon from "@material-ui/icons/Refresh";
import HelpIcon from '@material-ui/icons/HelpOutline';
import { ReactNode } from "react";

const PLACEHOLDER_ELECTRUM_RPC_URL = "ssl://blockstream.info:700";
const PLACEHOLDER_MONERO_NODE_URL = "http://xmr-node.cakewallet.com:18081";

const useStyles = makeStyles((theme) => ({
  title: {
    display: "flex",
    alignItems: "center",
    gap: theme.spacing(1),
  }
}));

export default function SettingsBox() {
  const dispatch = useAppDispatch();
  const classes = useStyles();
  
  return (
    <InfoBox
      title={
        <Box className={classes.title}>
          Settings
          <IconButton
          size="small"
          onClick={() => {
            dispatch(resetSettings());
          }}
        >
          <RefreshIcon />
        </IconButton>
        </Box>
      }
      additionalContent={
        <TableContainer>
          <Table>
            <TableBody>
              <ElectrumRpcUrlSetting />
              <MoneroNodeUrlSetting />
            </TableBody>
          </Table>
        </TableContainer>
      }
      mainContent={
        <Typography variant="subtitle2">
          Some of these settings require a restart to take effect.
        </Typography>
      }
      icon={null}
      loading={false}
    />
  );
}

// URL validation function, forces the URL to be in the format of "protocol://host:port/"
function isValidUrl(url: string, allowedProtocols: string[]): boolean {
  const urlPattern = new RegExp(`^(${allowedProtocols.join("|")})://[^\\s]+:\\d+/?$`);
  return urlPattern.test(url);
}

function ElectrumRpcUrlSetting() {
  const electrumRpcUrl = useSettings((s) => s.electrum_rpc_url);
  const dispatch = useAppDispatch();

  function isValid(url: string): boolean {
    return isValidUrl(url, ["ssl", "tcp"]);
  }

  return (
    <TableRow>
      <TableCell>
        <SettingLabel label="Custom Electrum RPC URL" tooltip="This is the URL of the Electrum server that the GUI will connect to. It is used to sync Bitcoin transactions. If you leave this field empty, the GUI will choose from a list of known servers at random." />
      </TableCell>
      <TableCell>
        <ValidatedTextField
          label="Electrum RPC URL"
          value={electrumRpcUrl}
          isValid={isValid}
          onValidatedChange={(value) => {
            dispatch(setElectrumRpcUrl(value));
          }}
          fullWidth
          placeholder={PLACEHOLDER_ELECTRUM_RPC_URL}
          allowEmpty
        />
      </TableCell>
    </TableRow>
  );
}

function SettingLabel({ label, tooltip }: { label: ReactNode, tooltip: string | null }) {
  return <Box style={{ display: "flex", alignItems: "center", gap: "0.5rem" }}>
    <Box>
      {label}
    </Box>
    <Tooltip title={tooltip}>
      <IconButton size="small">
        <HelpIcon />
      </IconButton>
    </Tooltip>
  </Box>
}

function MoneroNodeUrlSetting() {
  const moneroNodeUrl = useSettings((s) => s.monero_node_url);
  const dispatch = useAppDispatch();

  function isValid(url: string): boolean {
    return isValidUrl(url, ["http"]);
  }

  return (
    <TableRow>
      <TableCell>
       <SettingLabel label="Custom Monero Node URL" tooltip="This is the URL of the Monero node that the GUI will connect to. Ensure the node is listening for RPC connections over HTTP. If you leave this field empty, the GUI will choose from a list of known nodes at random." />
      </TableCell>
      <TableCell>
        <ValidatedTextField
          label="Monero Node URL"
          value={moneroNodeUrl}
          isValid={isValid}
          onValidatedChange={(value) => {
            dispatch(setMoneroNodeUrl(value));
          }}
          fullWidth
          placeholder={PLACEHOLDER_MONERO_NODE_URL}
          allowEmpty
        />
      </TableCell>
    </TableRow>
  );
}