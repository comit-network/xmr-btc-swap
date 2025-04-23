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
  target?: string;
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

export function isCliLogRelatedToSwap(
  log: CliLog | string,
  swapId: string,
): boolean {
  // If we only have a string, simply check if the string contains the swap id
  // This provides reasonable backwards compatability
  if (typeof log === "string") {
    return log.includes(swapId);
  }

  // If we have a parsed object as the log, check if
  //  - the log has the swap id as an attribute
  //  - there exists a span which has the swap id as an attribute
  return (
    log.fields["swap_id"] === swapId ||
    (log.spans?.some((span) => span["swap_id"] === swapId) ?? false)
  );
}

export function parseCliLogString(log: string): CliLog | string {
  try {
    const parsed = JSON.parse(log);
    if (isCliLog(parsed)) {
      return parsed as CliLog;
    } else {
      return log;
    }
  } catch {
    return log;
  }
}

