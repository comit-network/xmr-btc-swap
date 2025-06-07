import {
  Table,
  TableBody,
  TableCell,
  TableContainer,
  TableRow,
  Typography,
  IconButton,
  Box,
  Tooltip,
  Select,
  MenuItem,
  TableHead,
  Paper,
  Button,
  Dialog,
  DialogContent,
  DialogActions,
  DialogTitle,
  useTheme,
  Switch,
  SelectChangeEvent,
} from "@mui/material";
import {
  removeNode,
  resetSettings,
  setFetchFiatPrices,
  setFiatCurrency,
} from "store/features/settingsSlice";
import {
  addNode,
  Blockchain,
  FiatCurrency,
  moveUpNode,
  Network,
  setTheme,
} from "store/features/settingsSlice";
import {
  useAppDispatch,
  useNodes,
  useSettings,
} from "store/hooks";
import ValidatedTextField from "renderer/components/other/ValidatedTextField";
import HelpIcon from "@mui/icons-material/HelpOutline";
import { ReactNode, useState } from "react";
import { Theme } from "renderer/components/theme";
import {
  Add,
  ArrowUpward,
  Delete,
  Edit,
  HourglassEmpty,
} from "@mui/icons-material";
import { getNetwork } from "store/config";
import { currencySymbol } from "utils/formatUtils";
import { setTorEnabled } from "store/features/settingsSlice";
import InfoBox from "renderer/components/modal/swap/InfoBox";

const PLACEHOLDER_ELECTRUM_RPC_URL = "ssl://blockstream.info:700";
const PLACEHOLDER_MONERO_NODE_URL = "http://xmr-node.cakewallet.com:18081";

/**
 * The settings box, containing the settings for the GUI.
 */
export default function SettingsBox() {
  const theme = useTheme();

  return (
    <InfoBox
      title={
        <Box sx={{ display: "flex", alignItems: "center", gap: 1 }}>
          Settings
        </Box>
      }
      mainContent={
        <Typography variant="subtitle2">
          Customize the settings of the GUI. Some of these require a restart to
          take effect.
        </Typography>
      }
      additionalContent={
        <>
          {/* Table containing the settings */}
          <TableContainer>
            <Table>
              <TableBody>
                <TorSettings />
                <ElectrumRpcUrlSetting />
                <MoneroNodeUrlSetting />
                <FetchFiatPricesSetting />
                <ThemeSetting />
              </TableBody>
            </Table>
          </TableContainer>
          {/* Reset button with a bit of spacing */}
          <Box
            sx={(theme) => ({
              mt: theme.spacing(2),
            })}
          />
          <ResetButton />
        </>
      }
      icon={null}
      loading={false}
    />
  );
}

/**
 * A button that allows you to reset the settings.
 * Opens a modal that asks for confirmation first.
 */
function ResetButton() {
  const dispatch = useAppDispatch();
  const [modalOpen, setModalOpen] = useState(false);

  const onReset = () => {
    dispatch(resetSettings());
    setModalOpen(false);
  };

  return (
    <>
      <Button variant="outlined" onClick={() => setModalOpen(true)}>
        Reset Settings
      </Button>
      <Dialog open={modalOpen} onClose={() => setModalOpen(false)}>
        <DialogTitle>Reset Settings</DialogTitle>
        <DialogContent>
          Are you sure you want to reset the settings?
        </DialogContent>
        <DialogActions>
          <Button onClick={() => setModalOpen(false)}>Cancel</Button>
          <Button color="primary" onClick={onReset}>
            Reset
          </Button>
        </DialogActions>
      </Dialog>
    </>
  );
}

/**
 * A setting that allows you to enable or disable the fetching of fiat prices.
 */
function FetchFiatPricesSetting() {
  const fetchFiatPrices = useSettings((s) => s.fetchFiatPrices);
  const dispatch = useAppDispatch();

  return (
    <>
      <TableRow>
        <TableCell>
          <SettingLabel
            label="Query fiat prices"
            tooltip="Whether to fetch fiat prices via the clearnet. This is required for the price display to work. If you require total anonymity and don't use a VPN, you should disable this."
          />
        </TableCell>
        <TableCell>
          <Switch
            color="primary"
            checked={fetchFiatPrices}
            onChange={(event) =>
              dispatch(setFetchFiatPrices(event.currentTarget.checked))
            }
          />
        </TableCell>
      </TableRow>
      {fetchFiatPrices ? <FiatCurrencySetting /> : <></>}
    </>
  );
}

/**
 * A setting that allows you to select the fiat currency to display prices in.
 */
function FiatCurrencySetting() {
  const fiatCurrency = useSettings((s) => s.fiatCurrency);
  const dispatch = useAppDispatch();
  const onChange = (e: SelectChangeEvent<FiatCurrency>) =>
    dispatch(setFiatCurrency(e.target.value as FiatCurrency));

  return (
    <TableRow>
      <TableCell>
        <SettingLabel
          label="Fiat currency"
          tooltip="This is the currency that the price display will show prices in."
        />
      </TableCell>
      <TableCell>
        <Select
          value={fiatCurrency}
          onChange={onChange}
          variant="outlined"
          fullWidth
        >
          {Object.values(FiatCurrency).map((currency) => (
            <MenuItem key={currency} value={currency}>
              <Box
                sx={{
                  display: "flex",
                  justifyContent: "space-between",
                  width: "100%",
                }}
              >
                <Box>{currency}</Box>
                <Box>{currencySymbol(currency)}</Box>
              </Box>
            </MenuItem>
          ))}
        </Select>
      </TableCell>
    </TableRow>
  );
}

/**
 * URL validation function, forces the URL to be in the format of "protocol://host:port/"
 */
function isValidUrl(url: string, allowedProtocols: string[]): boolean {
  const urlPattern = new RegExp(
    `^(${allowedProtocols.join("|")})://[^\\s]+:\\d+/?$`,
  );
  return urlPattern.test(url);
}

/**
 * A setting that allows you to select the Electrum RPC URL to use.
 */
function ElectrumRpcUrlSetting() {
  const [tableVisible, setTableVisible] = useState(false);
  const network = getNetwork();

  const isValid = (url: string) => isValidUrl(url, ["ssl", "tcp"]);

  return (
    <TableRow>
      <TableCell>
        <SettingLabel
          label="Custom Electrum RPC URL"
          tooltip="This is the URL of the Electrum server that the GUI will connect to. It is used to sync Bitcoin transactions. If you leave this field empty, the GUI will choose from a list of known servers at random."
        />
      </TableCell>
      <TableCell>
        <IconButton onClick={() => setTableVisible(true)} size="large">
          {<Edit />}
        </IconButton>
        {tableVisible ? (
          <NodeTableModal
            open={tableVisible}
            onClose={() => setTableVisible(false)}
            network={network}
            blockchain={Blockchain.Bitcoin}
            isValid={isValid}
            placeholder={PLACEHOLDER_ELECTRUM_RPC_URL}
          />
        ) : (
          <></>
        )}
      </TableCell>
    </TableRow>
  );
}

/**
 * A label for a setting, with a tooltip icon.
 */
function SettingLabel({
  label,
  tooltip,
}: {
  label: ReactNode;
  tooltip: string | null;
}) {
  return (
    <Box style={{ display: "flex", alignItems: "center", gap: "0.5rem" }}>
      <Box>{label}</Box>
      <Tooltip title={tooltip}>
        <IconButton size="small">
          <HelpIcon />
        </IconButton>
      </Tooltip>
    </Box>
  );
}

/**
 * A setting that allows you to select the Monero Node URL to use.
 */
function MoneroNodeUrlSetting() {
  const network = getNetwork();
  const [tableVisible, setTableVisible] = useState(false);

  const isValid = (url: string) => isValidUrl(url, ["http"]);

  return (
    <TableRow>
      <TableCell>
        <SettingLabel
          label="Custom Monero Node URL"
          tooltip="This is the URL of the Monero node that the GUI will connect to. Ensure the node is listening for RPC connections over HTTP. If you leave this field empty, the GUI will choose from a list of known nodes at random."
        />
      </TableCell>
      <TableCell>
        <IconButton onClick={() => setTableVisible(!tableVisible)} size="large">
          <Edit />
        </IconButton>
        {tableVisible ? (
          <NodeTableModal
            open={tableVisible}
            onClose={() => setTableVisible(false)}
            network={network}
            blockchain={Blockchain.Monero}
            isValid={isValid}
            placeholder={PLACEHOLDER_MONERO_NODE_URL}
          />
        ) : (
          <></>
        )}
      </TableCell>
    </TableRow>
  );
}

/**
 * A setting that allows you to select the theme of the GUI.
 */
function ThemeSetting() {
  const theme = useSettings((s) => s.theme);
  const dispatch = useAppDispatch();

  return (
    <TableRow>
      <TableCell>
        <SettingLabel label="Theme" tooltip="This is the theme of the GUI." />
      </TableCell>
      <TableCell>
        <Select
          value={theme}
          onChange={(e) => dispatch(setTheme(e.target.value as Theme))}
          variant="outlined"
          fullWidth
        >
          {/** Create an option for each theme variant */}
          {Object.values(Theme).map((themeValue) => (
            <MenuItem key={themeValue} value={themeValue}>
              {themeValue.charAt(0).toUpperCase() + themeValue.slice(1)}
            </MenuItem>
          ))}
        </Select>
      </TableCell>
    </TableRow>
  );
}

/**
 * A modal containing a NodeTable for a given network and blockchain.
 * It allows you to add, remove, and move nodes up the list.
 */
function NodeTableModal({
  open,
  onClose,
  network,
  isValid,
  placeholder,
  blockchain,
}: {
  network: Network;
  blockchain: Blockchain;
  isValid: (url: string) => boolean;
  placeholder: string;
  open: boolean;
  onClose: () => void;
}) {
  return (
    <Dialog open={open} onClose={onClose}>
      <DialogTitle>Available Nodes</DialogTitle>
      <DialogContent>
        <Typography variant="subtitle2">
          When the daemon is started, it will attempt to connect to the first
          available {blockchain} node in this list. If you leave this field
          empty or all nodes are unavailable, it will choose from a list of
          known nodes at random. Requires a restart to take effect.
        </Typography>
        <NodeTable
          network={network}
          blockchain={blockchain}
          isValid={isValid}
          placeholder={placeholder}
        />
      </DialogContent>
      <DialogActions>
        <Button onClick={onClose} size="large">
          Close
        </Button>
      </DialogActions>
    </Dialog>
  );
}

// Create a circle SVG with a given color and radius
function Circle({ color, radius = 6 }: { color: string; radius?: number }) {
  return (
    <span>
      <svg
        width={radius * 2}
        height={radius * 2}
        viewBox={`0 0 ${radius * 2} ${radius * 2}`}
      >
        <circle cx={radius} cy={radius} r={radius} fill={color} />
      </svg>
    </span>
  );
}

/**
 * Displays a status indicator for a node
 */
function NodeStatus({ status }: { status: boolean | undefined }) {
  const theme = useTheme();

  switch (status) {
    case true:
      return (
        <Tooltip
          title={"This node is available and responding to RPC requests"}
        >
          <Circle color={theme.palette.success.dark} />
        </Tooltip>
      );
    case false:
      return (
        <Tooltip
          title={"This node is not available or not responding to RPC requests"}
        >
          <Circle color={theme.palette.error.dark} />
        </Tooltip>
      );
    default:
      return (
        <Tooltip title={"The status of this node is currently unknown"}>
          <HourglassEmpty />
        </Tooltip>
      );
  }
}

/**
 * A table that displays the available nodes for a given network and blockchain.
 * It allows you to add, remove, and move nodes up the list.
 * It fetches the nodes from the store (nodesSlice) and the statuses of all nodes every 15 seconds.
 */
function NodeTable({
  network,
  blockchain,
  isValid,
  placeholder,
}: {
  network: Network;
  blockchain: Blockchain;
  isValid: (url: string) => boolean;
  placeholder: string;
}) {
  const availableNodes = useSettings((s) => s.nodes[network][blockchain]);
  const currentNode = availableNodes[0];
  const nodeStatuses = useNodes((s) => s.nodes);
  const [newNode, setNewNode] = useState("");
  const dispatch = useAppDispatch();

  const onAddNewNode = () => {
    dispatch(addNode({ network, type: blockchain, node: newNode }));
    setNewNode("");
  };

  const onRemoveNode = (node: string) =>
    dispatch(removeNode({ network, type: blockchain, node }));

  const onMoveUpNode = (node: string) =>
    dispatch(moveUpNode({ network, type: blockchain, node }));

  const moveUpButton = (node: string) => {
    if (currentNode === node) return <></>;

    return (
      <Tooltip title={"Move this node to the top of the list"}>
        <IconButton onClick={() => onMoveUpNode(node)} size="large">
          <ArrowUpward />
        </IconButton>
      </Tooltip>
    );
  };

  return (
    <TableContainer
      component={Paper}
      style={{ marginTop: "1rem" }}
      elevation={0}
    >
      <Table size="small">
        {/* Table header row */}
        <TableHead>
          <TableRow>
            <TableCell align="center">Node URL</TableCell>
            <TableCell align="center">Status</TableCell>
            <TableCell align="center">Actions</TableCell>
          </TableRow>
        </TableHead>
        <TableBody>
          {/* Table body rows: one for each node */}
          {availableNodes.map((node, index) => (
            <TableRow key={index}>
              {/* Node URL */}
              <TableCell>
                <Typography variant="overline">{node}</Typography>
              </TableCell>
              {/* Node status icon */}
              <TableCell align="center">
                <NodeStatus status={nodeStatuses[blockchain][node]} />
              </TableCell>
              {/* Remove and move buttons */}
              <TableCell>
                <Box style={{ display: "flex" }}>
                  <Tooltip
                    title={"Remove this node from your list"}
                    children={
                      <IconButton
                        onClick={() => onRemoveNode(node)}
                        children={<Delete />}
                        size="large"
                      />
                    }
                  />
                  {moveUpButton(node)}
                </Box>
              </TableCell>
            </TableRow>
          ))}
          {/* Last row: add a new node */}
          <TableRow key={-1}>
            <TableCell>
              <ValidatedTextField
                label="Add a new node"
                value={newNode}
                onValidatedChange={setNewNode}
                placeholder={placeholder}
                fullWidth
                isValid={isValid}
                variant="outlined"
                noErrorWhenEmpty
              />
            </TableCell>
            <TableCell></TableCell>
            <TableCell>
              <Tooltip title={"Add this node to your list"}>
                <IconButton
                  onClick={onAddNewNode}
                  disabled={
                    availableNodes.includes(newNode) || newNode.length === 0
                  }
                  size="large"
                >
                  <Add />
                </IconButton>
              </Tooltip>
            </TableCell>
          </TableRow>
        </TableBody>
      </Table>
    </TableContainer>
  );
}

export function TorSettings() {
  const dispatch = useAppDispatch();
  const torEnabled = useSettings((settings) => settings.enableTor);
  const handleChange = (event: React.ChangeEvent<HTMLInputElement>) =>
    dispatch(setTorEnabled(event.target.checked));
  const status = (state: boolean) => (state === true ? "enabled" : "disabled");

  return (
    <TableRow>
      <TableCell>
        <SettingLabel
          label="Use Tor"
          tooltip="Tor (The Onion Router) is a decentralized network allowing for anonymous browsing. If enabled, the app will use its internal Tor client to hide your IP address from the maker. Requires a restart to take effect."
        />
      </TableCell>

      <TableCell>
        <Switch checked={torEnabled} onChange={handleChange} color="primary" />
      </TableCell>
    </TableRow>
  );
}
