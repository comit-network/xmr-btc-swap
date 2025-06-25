import { createSlice, PayloadAction } from "@reduxjs/toolkit";
import { Theme } from "renderer/components/theme";

export type DonateToDevelopmentTip = false | 0.0005 | 0.0075;

const DEFAULT_RENDEZVOUS_POINTS = [
  "/dns4/discover.unstoppableswap.net/tcp/8888/p2p/12D3KooWA6cnqJpVnreBVnoro8midDL9Lpzmg8oJPoAGi7YYaamE",
  "/dns4/discover2.unstoppableswap.net/tcp/8888/p2p/12D3KooWGRvf7qVQDrNR5nfYD6rKrbgeTi9x8RrbdxbmsPvxL4mw",
  "/dns4/darkness.su/tcp/8888/p2p/12D3KooWFQAgVVS9t9UgL6v1sLprJVM7am5hFK7vy9iBCCoCBYmU",
];

export interface SettingsState {
  /// This is an ordered list of node urls for each network and blockchain
  nodes: Record<Network, Record<Blockchain, string[]>>;
  /// Which theme to use
  theme: Theme;
  /// Whether to fetch fiat prices from the internet
  fetchFiatPrices: boolean;
  fiatCurrency: FiatCurrency;
  /// Whether to enable Tor for p2p connections
  enableTor: boolean;
  /// Whether to use the Monero RPC pool for load balancing (true) or custom nodes (false)
  useMoneroRpcPool: boolean;
  userHasSeenIntroduction: boolean;
  /// List of rendezvous points
  rendezvousPoints: string[];
  /// Does the user want to donate parts of his swaps to funding the development
  /// of the project?
  donateToDevelopment: DonateToDevelopmentTip;
}

export enum FiatCurrency {
  Usd = "USD",
  Eur = "EUR",
  Gbp = "GBP",
  Chf = "CHF",
  Jpy = "JPY",
  // the following are copied from the coin gecko API and claude, not sure if they all work
  Aed = "AED",
  Ars = "ARS",
  Aud = "AUD",
  Bdt = "BDT",
  Bhd = "BHD",
  Bmd = "BMD",
  Brl = "BRL",
  Cad = "CAD",
  Clp = "CLP",
  Cny = "CNY",
  Czk = "CZK",
  Dkk = "DKK",
  Gel = "GEL",
  Hkd = "HKD",
  Huf = "HUF",
  Idr = "IDR",
  Ils = "ILS",
  Inr = "INR",
  Krw = "KRW",
  Kwd = "KWD",
  Lkr = "LKR",
  Mmk = "MMK",
  Mxn = "MXN",
  Myr = "MYR",
  Ngn = "NGN",
  Nok = "NOK",
  Nzd = "NZD",
  Php = "PHP",
  Pkr = "PKR",
  Pln = "PLN",
  Rub = "RUB",
  Sar = "SAR",
  Sek = "SEK",
  Sgd = "SGD",
  Thb = "THB",
  Try = "TRY",
  Twd = "TWD",
  Uah = "UAH",
  Ves = "VES",
  Vnd = "VND",
  Zar = "ZAR",
}

export enum Network {
  Testnet = "testnet",
  Mainnet = "mainnet",
}

export enum Blockchain {
  Bitcoin = "bitcoin",
  Monero = "monero",
}

const initialState: SettingsState = {
  nodes: {
    [Network.Testnet]: {
      [Blockchain.Bitcoin]: [
        "ssl://ax101.blockeng.ch:60002",
        "ssl://blackie.c3-soft.com:57006",
        "ssl://v22019051929289916.bestsrv.de:50002",
        "tcp://v22019051929289916.bestsrv.de:50001",
        "tcp://electrum.blockstream.info:60001",
        "ssl://electrum.blockstream.info:60002",
        "ssl://blockstream.info:993",
        "tcp://blockstream.info:143",
        "ssl://testnet.qtornado.com:51002",
        "tcp://testnet.qtornado.com:51001",
        "tcp://testnet.aranguren.org:51001",
        "ssl://testnet.aranguren.org:51002",
        "ssl://testnet.qtornado.com:50002",
        "ssl://bitcoin.devmole.eu:5010",
        "tcp://bitcoin.devmole.eu:5000",
      ],
      [Blockchain.Monero]: [],
    },
    [Network.Mainnet]: {
      [Blockchain.Bitcoin]: [
        "ssl://electrum.blockstream.info:50002",
        "tcp://electrum.blockstream.info:50001",
        "ssl://bitcoin.stackwallet.com:50002",
        "ssl://b.1209k.com:50002",
        "tcp://electrum.coinucopia.io:50001",
      ],
      [Blockchain.Monero]: [],
    },
  },
  theme: Theme.Dark,
  fetchFiatPrices: false,
  fiatCurrency: FiatCurrency.Usd,
  enableTor: true,
  useMoneroRpcPool: true, // Default to using RPC pool
  userHasSeenIntroduction: false,
  rendezvousPoints: DEFAULT_RENDEZVOUS_POINTS,
  donateToDevelopment: false, // Default to no donation
};

const alertsSlice = createSlice({
  name: "settings",
  initialState,
  reducers: {
    moveUpNode(
      slice,
      action: PayloadAction<{
        network: Network;
        type: Blockchain;
        node: string;
      }>,
    ) {
      const index = slice.nodes[action.payload.network][
        action.payload.type
      ].indexOf(action.payload.node);
      if (index > 0) {
        const temp =
          slice.nodes[action.payload.network][action.payload.type][index];
        slice.nodes[action.payload.network][action.payload.type][index] =
          slice.nodes[action.payload.network][action.payload.type][index - 1];
        slice.nodes[action.payload.network][action.payload.type][index - 1] =
          temp;
      }
    },
    setTheme(slice, action: PayloadAction<Theme>) {
      slice.theme = action.payload;
    },
    setFetchFiatPrices(slice, action: PayloadAction<boolean>) {
      slice.fetchFiatPrices = action.payload;
    },
    setFiatCurrency(slice, action: PayloadAction<FiatCurrency>) {
      slice.fiatCurrency = action.payload;
    },
    addRendezvousPoint(slice, action: PayloadAction<string>) {
      slice.rendezvousPoints.push(action.payload);
    },
    removeRendezvousPoint(slice, action: PayloadAction<string>) {
      slice.rendezvousPoints = slice.rendezvousPoints.filter(
        (point) => point !== action.payload,
      );
    },
    addNode(
      slice,
      action: PayloadAction<{
        network: Network;
        type: Blockchain;
        node: string;
      }>,
    ) {
      // Make sure the node is not already in the list
      if (
        slice.nodes[action.payload.network][action.payload.type].includes(
          action.payload.node,
        )
      ) {
        return;
      }
      // Add the node to the list
      slice.nodes[action.payload.network][action.payload.type].push(
        action.payload.node,
      );
    },
    removeNode(
      slice,
      action: PayloadAction<{
        network: Network;
        type: Blockchain;
        node: string;
      }>,
    ) {
      slice.nodes[action.payload.network][action.payload.type] = slice.nodes[
        action.payload.network
      ][action.payload.type].filter((node) => node !== action.payload.node);
    },
    setUserHasSeenIntroduction(slice, action: PayloadAction<boolean>) {
      slice.userHasSeenIntroduction = action.payload;
    },
    resetSettings(_) {
      return initialState;
    },
    setTorEnabled(slice, action: PayloadAction<boolean>) {
      slice.enableTor = action.payload;
    },
    setUseMoneroRpcPool(slice, action: PayloadAction<boolean>) {
      slice.useMoneroRpcPool = action.payload;
    },
    setDonateToDevelopment(
      slice,
      action: PayloadAction<DonateToDevelopmentTip>,
    ) {
      slice.donateToDevelopment = action.payload;
    },
  },
});

export const {
  moveUpNode,
  setTheme,
  addNode,
  removeNode,
  resetSettings,
  setFetchFiatPrices,
  setFiatCurrency,
  setTorEnabled,
  setUseMoneroRpcPool,
  setUserHasSeenIntroduction,
  addRendezvousPoint,
  removeRendezvousPoint,
  setDonateToDevelopment,
} = alertsSlice.actions;

export default alertsSlice.reducer;
