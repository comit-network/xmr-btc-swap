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
  ToggleButton,
  ToggleButtonGroup,
} from "@mui/material";
import {
  addNode,
  addRendezvousPoint,
  Blockchain,
  DonateToDevelopmentTip,
  FiatCurrency,
  moveUpNode,
  Network,
  removeNode,
  removeRendezvousPoint,
  resetSettings,
  setFetchFiatPrices,
  setFiatCurrency,
  setTheme,
  setTorEnabled,
  setUseMoneroRpcPool,
  setDonateToDevelopment,
} from "store/features/settingsSlice";
import { useAppDispatch, useNodes, useSettings } from "store/hooks";
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
  Refresh,
} from "@mui/icons-material";

import { getNetwork } from "store/config";
import { currencySymbol } from "utils/formatUtils";
import InfoBox from "renderer/components/modal/swap/InfoBox";
import { isValidMultiAddressWithPeerId } from "utils/parseUtils";
import { getNodeStatus } from "renderer/rpc";
import { setStatus } from "store/features/nodesSlice";

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
                <DonationTipSetting />
                <ElectrumRpcUrlSetting />
                <MoneroRpcPoolSetting />
                <MoneroNodeUrlSetting />
                <FetchFiatPricesSetting />
                <ThemeSetting />
                <RendezvousPointsSetting />
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
  disabled = false,
}: {
  label: ReactNode;
  tooltip: string | null;
  disabled?: boolean;
}) {
  const opacity = disabled ? 0.5 : 1;

  return (
    <Box
      style={{ display: "flex", alignItems: "center", gap: "0.5rem", opacity }}
    >
      <Box>{label}</Box>
      <Tooltip title={tooltip}>
        <IconButton size="small" disabled={disabled}>
          <HelpIcon />
        </IconButton>
      </Tooltip>
    </Box>
  );
}

/**
 * A setting that allows you to toggle between using the Monero RPC Pool and custom nodes.
 */
function MoneroRpcPoolSetting() {
  const useMoneroRpcPool = useSettings((s) => s.useMoneroRpcPool);
  const dispatch = useAppDispatch();

  const handleChange = (
    event: React.MouseEvent<HTMLElement>,
    newValue: string,
  ) => {
    if (newValue !== null) {
      dispatch(setUseMoneroRpcPool(newValue === "pool"));
    }
  };

  return (
    <TableRow>
      <TableCell>
        <SettingLabel
          label="Monero Node Selection"
          tooltip="Choose between using a load-balanced pool of Monero nodes for better reliability, or configure custom Monero nodes."
        />
      </TableCell>
      <TableCell>
        <ToggleButtonGroup
          color="primary"
          value={useMoneroRpcPool ? "pool" : "custom"}
          exclusive
          onChange={handleChange}
          aria-label="Monero node selection"
          size="small"
        >
          <ToggleButton value="pool">Pool (Recommended)</ToggleButton>
          <ToggleButton value="custom">Manual</ToggleButton>
        </ToggleButtonGroup>
      </TableCell>
    </TableRow>
  );
}

/**
 * A setting that allows you to configure a single Monero Node URL.
 * Gets disabled when RPC pool is enabled.
 */
function MoneroNodeUrlSetting() {
  const network = getNetwork();
  const useMoneroRpcPool = useSettings((s) => s.useMoneroRpcPool);
  const moneroNodeUrl = useSettings(
    (s) => s.nodes[network][Blockchain.Monero][0] || "",
  );
  const nodeStatuses = useNodes((s) => s.nodes);
  const dispatch = useAppDispatch();
  const [isRefreshing, setIsRefreshing] = useState(false);

  const currentNodes = useSettings((s) => s.nodes[network][Blockchain.Monero]);

  const handleNodeUrlChange = (newUrl: string) => {
    // Remove existing nodes and add the new one
    currentNodes.forEach((node) => {
      dispatch(removeNode({ network, type: Blockchain.Monero, node }));
    });

    if (newUrl.trim()) {
      dispatch(
        addNode({ network, type: Blockchain.Monero, node: newUrl.trim() }),
      );
    }
  };

  const handleRefreshStatus = async () => {
    // Don't refresh if pool is enabled or no node URL is configured
    if (!moneroNodeUrl || useMoneroRpcPool) return;

    setIsRefreshing(true);
    try {
      const status = await getNodeStatus(
        moneroNodeUrl,
        Blockchain.Monero,
        network,
      );

      // Update the status in the store
      dispatch(
        setStatus({
          node: moneroNodeUrl,
          status,
          blockchain: Blockchain.Monero,
        }),
      );
    } catch (error) {
      console.error("Failed to refresh node status:", error);
    } finally {
      setIsRefreshing(false);
    }
  };

  const isValid = (url: string) => url === "" || isValidUrl(url, ["http"]);
  const nodeStatus = moneroNodeUrl
    ? nodeStatuses[Blockchain.Monero][moneroNodeUrl]
    : null;

  return (
    <TableRow>
      <TableCell>
        <SettingLabel
          label="Custom Monero Node URL"
          tooltip={
            useMoneroRpcPool
              ? "This setting is disabled because Monero RPC pool is enabled. Disable the RPC pool to configure a custom node."
              : "This is the URL of the Monero node that the GUI will connect to. It is used to sync Monero transactions. If you leave this field empty, the GUI will choose from a list of known servers at random."
          }
          disabled={useMoneroRpcPool}
        />
      </TableCell>
      <TableCell>
        <Box sx={{ display: "flex", alignItems: "center", gap: 1 }}>
          <ValidatedTextField
            value={moneroNodeUrl}
            onValidatedChange={handleNodeUrlChange}
            placeholder={PLACEHOLDER_MONERO_NODE_URL}
            disabled={useMoneroRpcPool}
            fullWidth
            isValid={isValid}
            variant="outlined"
            noErrorWhenEmpty
          />
          <>
            <Tooltip
              title={
                useMoneroRpcPool
                  ? "Node status checking is disabled when using the pool"
                  : !moneroNodeUrl
                    ? "Enter a node URL to check status"
                    : "Node status"
              }
            >
              <Box sx={{ display: "flex", alignItems: "center" }}>
                <Circle
                  color={
                    useMoneroRpcPool || !moneroNodeUrl
                      ? "gray"
                      : nodeStatus
                        ? "green"
                        : "red"
                  }
                />
              </Box>
            </Tooltip>
            <Tooltip
              title={
                useMoneroRpcPool
                  ? "Node status refresh is disabled when using the pool"
                  : !moneroNodeUrl
                    ? "Enter a node URL to refresh status"
                    : "Refresh node status"
              }
            >
              <IconButton
                onClick={handleRefreshStatus}
                disabled={isRefreshing || useMoneroRpcPool || !moneroNodeUrl}
                size="small"
              >
                {isRefreshing ? <HourglassEmpty /> : <Refresh />}
              </IconButton>
            </Tooltip>
          </>
        </Box>
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
          known nodes at random.
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
                <Circle
                  color={nodeStatuses[blockchain][node] ? "green" : "red"}
                />
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
          tooltip="Route network traffic through Tor to hide your IP address from the maker."
        />
      </TableCell>

      <TableCell>
        <Switch checked={torEnabled} onChange={handleChange} color="primary" />
      </TableCell>
    </TableRow>
  );
}

/**
 * A setting that allows you to manage rendezvous points for maker discovery
 */
function RendezvousPointsSetting() {
  const [tableVisible, setTableVisible] = useState(false);
  const rendezvousPoints = useSettings((s) => s.rendezvousPoints);
  const dispatch = useAppDispatch();
  const [newPoint, setNewPoint] = useState("");

  const onAddNewPoint = () => {
    dispatch(addRendezvousPoint(newPoint));
    setNewPoint("");
  };

  const onRemovePoint = (point: string) => {
    dispatch(removeRendezvousPoint(point));
  };

  return (
    <TableRow>
      <TableCell>
        <SettingLabel
          label="Rendezvous Points"
          tooltip="These are the points where makers can be discovered. Add custom rendezvous points here to expand your maker discovery options."
        />
      </TableCell>
      <TableCell>
        <IconButton onClick={() => setTableVisible(true)}>
          <Edit />
        </IconButton>
        {tableVisible && (
          <Dialog
            open={true}
            onClose={() => setTableVisible(false)}
            maxWidth="md"
            fullWidth
          >
            <DialogTitle>Rendezvous Points</DialogTitle>
            <DialogContent>
              <Typography variant="subtitle2">
                Add or remove rendezvous points where makers can be discovered.
                These points help you find trading partners in a decentralized
                way.
              </Typography>
              <TableContainer
                component={Paper}
                style={{ marginTop: "1rem" }}
                elevation={0}
              >
                <Table size="small">
                  <TableHead>
                    <TableRow>
                      <TableCell style={{ width: "85%" }}>
                        Rendezvous Point
                      </TableCell>
                      <TableCell style={{ width: "15%" }} align="right">
                        Actions
                      </TableCell>
                    </TableRow>
                  </TableHead>
                  <TableBody>
                    {rendezvousPoints.map((point, index) => (
                      <TableRow key={index}>
                        <TableCell style={{ wordBreak: "break-all" }}>
                          <Typography variant="overline">{point}</Typography>
                        </TableCell>
                        <TableCell align="right">
                          <Tooltip title="Remove this rendezvous point">
                            <IconButton onClick={() => onRemovePoint(point)}>
                              <Delete />
                            </IconButton>
                          </Tooltip>
                        </TableCell>
                      </TableRow>
                    ))}
                    <TableRow>
                      <TableCell>
                        <ValidatedTextField
                          label="Add new rendezvous point"
                          value={newPoint}
                          onValidatedChange={setNewPoint}
                          placeholder="/dns4/discover.unstoppableswap.net/tcp/8888/p2p/12D3KooWA6cnqJpVnreBVnoro8midDL9Lpzmg8oJPoAGi7YYaamE"
                          fullWidth
                          isValid={isValidMultiAddressWithPeerId}
                          variant="outlined"
                          noErrorWhenEmpty
                        />
                      </TableCell>
                      <TableCell align="right">
                        <Tooltip title="Add this rendezvous point">
                          <IconButton
                            onClick={onAddNewPoint}
                            disabled={
                              !isValidMultiAddressWithPeerId(newPoint) ||
                              newPoint.length === 0
                            }
                          >
                            <Add />
                          </IconButton>
                        </Tooltip>
                      </TableCell>
                    </TableRow>
                  </TableBody>
                </Table>
              </TableContainer>
            </DialogContent>
            <DialogActions>
              <Button onClick={() => setTableVisible(false)} size="large">
                Close
              </Button>
            </DialogActions>
          </Dialog>
        )}
      </TableCell>
    </TableRow>
  );
}

/**
 * A setting that allows you to set a development donation tip amount
 */
function DonationTipSetting() {
  const donateToDevelopment = useSettings((s) => s.donateToDevelopment);
  const dispatch = useAppDispatch();

  const handleTipSelect = (tipAmount: DonateToDevelopmentTip) => {
    dispatch(setDonateToDevelopment(tipAmount));
  };

  const formatTipLabel = (tip: DonateToDevelopmentTip) => {
    if (tip === false) return "0%";
    return `${(tip * 100).toFixed(2)}%`;
  };

  const getTipButtonColor = (
    tip: DonateToDevelopmentTip,
    isSelected: boolean,
  ) => {
    // Only show colored if selected and > 0
    if (isSelected && tip !== false) {
      return "#198754"; // Green for any tip > 0
    }
    return "#6c757d"; // Gray for all unselected or no tip
  };

  const getTipButtonSelectedColor = (tip: DonateToDevelopmentTip) => {
    if (tip === false) return "#5c636a"; // Darker gray
    return "#146c43"; // Darker green for any tip > 0
  };

  return (
    <TableRow>
      <TableCell>
        <SettingLabel
          label="Tip to the developers"
          tooltip="Support the development of UnstoppableSwap by donating a small percentage of your swaps. Donations go directly to paying for infrastructure costs and developers"
        />
      </TableCell>
      <TableCell>
        <Box sx={{ display: "flex", flexDirection: "column", gap: 1 }}>
          <ToggleButtonGroup
            value={donateToDevelopment}
            exclusive
            onChange={(event, newValue) => {
              if (newValue !== null) {
                handleTipSelect(newValue);
              }
            }}
            aria-label="Development tip amount"
            size="small"
            sx={{
              width: "100%",
              gap: 1,
              "& .MuiToggleButton-root": {
                flex: 1,
                borderRadius: "8px",
                fontWeight: "600",
                textTransform: "none",
                border: "2px solid",
                "&:not(:first-of-type)": {
                  marginLeft: "8px",
                  borderLeft: "2px solid",
                },
              },
            }}
          >
            {([false, 0.0005, 0.0075] as const).map((tipAmount) => (
              <ToggleButton
                key={String(tipAmount)}
                value={tipAmount}
                sx={{
                  borderColor: `${getTipButtonColor(tipAmount, donateToDevelopment === tipAmount)} !important`,
                  color:
                    donateToDevelopment === tipAmount
                      ? "white"
                      : getTipButtonColor(
                          tipAmount,
                          donateToDevelopment === tipAmount,
                        ),
                  backgroundColor:
                    donateToDevelopment === tipAmount
                      ? getTipButtonColor(
                          tipAmount,
                          donateToDevelopment === tipAmount,
                        )
                      : "transparent",
                  "&:hover": {
                    backgroundColor: `${getTipButtonSelectedColor(tipAmount)} !important`,
                    color: "white !important",
                  },
                  "&.Mui-selected": {
                    backgroundColor: `${getTipButtonColor(tipAmount, true)} !important`,
                    color: "white !important",
                    "&:hover": {
                      backgroundColor: `${getTipButtonSelectedColor(tipAmount)} !important`,
                    },
                  },
                }}
              >
                {formatTipLabel(tipAmount)}
              </ToggleButton>
            ))}
          </ToggleButtonGroup>
          <Typography variant="subtitle2">
            <ul style={{ margin: 0, padding: "0 1.5rem" }}>
              <li>
                Tips go <strong>directly</strong> towards paying for
                infrastructure costs and developers
              </li>
              <li>
                Only ever sent for <strong>successful</strong> swaps
              </li>{" "}
              (refunds are not counted)
              <li>Monero is used for the tips, giving you full anonymity</li>
            </ul>
          </Typography>
        </Box>
      </TableCell>
    </TableRow>
  );
}
