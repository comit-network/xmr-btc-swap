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

function isCliLog(log: unknown): log is CliLog {
  return (
    typeof log === "object" &&
    log !== null &&
    "timestamp" in log &&
    "level" in log &&
    "fields" in log
  );
}

export function parseCliLogString(log: string): CliLog | string {
  try {
    const parsed = JSON.parse(log);
    if (isCliLog(parsed)) {
      return parsed;
    } else {
      return log;
    }
  } catch {
    return log;
  }
}
