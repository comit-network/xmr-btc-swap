export interface ExtendedMakerStatus extends MakerStatus {
  uptime?: number;
  age?: number;
  relevancy?: number;
  version?: string;
  recommended?: boolean;
}

export interface MakerStatus extends MakerQuote, Maker { }

export interface MakerQuote {
  price: number;
  minSwapAmount: number;
  maxSwapAmount: number;
}

export interface Maker {
  multiAddr: string;
  testnet: boolean;
  peerId: string;
}

export interface Alert {
  id: number;
  title: string;
  body: string;
  severity: "info" | "warning" | "error";
}
