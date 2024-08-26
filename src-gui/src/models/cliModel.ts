export enum SwapSpawnType {
  INIT = "init",
  RESUME = "resume",
  CANCEL_REFUND = "cancel-refund",
}

export type CliLogSpanType = string | "BitcoinWalletSubscription";

export interface CliLog {
  timestamp: string;
  level: "DEBUG" | "INFO" | "WARN" | "ERROR" | "TRACE";
  fields: {
    message: string;
    [index: string]: unknown;
  };
  spans?: {
    name: CliLogSpanType;
    [index: string]: unknown;
  }[];
}
