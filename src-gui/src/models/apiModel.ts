export interface ExtendedProviderStatus extends ProviderStatus {
  uptime?: number;
  age?: number;
  relevancy?: number;
  version?: string;
  recommended?: boolean;
}

export interface ProviderStatus extends ProviderQuote, Provider {}

export interface ProviderQuote {
  price: number;
  minSwapAmount: number;
  maxSwapAmount: number;
}

export interface Provider {
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
