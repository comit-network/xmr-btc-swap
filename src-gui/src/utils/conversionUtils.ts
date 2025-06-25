import { MakerStatus, ExtendedMakerStatus } from "models/apiModel";
import { SellerStatus } from "models/tauriModel";
import { isTestnet } from "store/config";
import { splitPeerIdFromMultiAddress } from "./parseUtils";

export function satsToBtc(sats: number): number {
  return sats / 100000000;
}

export function btcToSats(btc: number): number {
  return btc * 100000000;
}

export function piconerosToXmr(piconeros: number): number {
  return piconeros / 1000000000000;
}

export function isXmrAddressValid(address: string, stagenet: boolean) {
  const re = stagenet
    ? "^(?:[57][0-9A-Za-z]{94}|[57][0-9A-Za-z]{105})$"
    : "^(?:[48][0-9A-Za-z]{94}|[48][0-9A-Za-z]{105})$";
  return new RegExp(`(?:^${re}$)`).test(address);
}

export function isBtcAddressValid(address: string, testnet: boolean) {
  const re = testnet
    ? "(tb1)[a-zA-HJ-NP-Z0-9]{25,49}"
    : "(bc1)[a-zA-HJ-NP-Z0-9]{25,49}";
  return new RegExp(`(?:^${re}$)`).test(address);
}

export function getBitcoinTxExplorerUrl(txid: string, testnet: boolean) {
  return `https://mempool.space/${testnet ? "/testnet" : ""}/tx/${txid}`;
}

export function getMoneroTxExplorerUrl(txid: string, stagenet: boolean) {
  if (stagenet) {
    return `https://stagenet.xmrchain.net/tx/${txid}`;
  }
  return `https://xmrchain.net/tx/${txid}`;
}

export function secondsToDays(seconds: number): number {
  return seconds / 86400;
}

// Convert the "Seller" object returned by the list sellers tauri endpoint to a "MakerStatus" object
// which we use internally to represent the status of a provider. This provides consistency between
// the models returned by the public registry and the models used internally.
export function rendezvousSellerToMakerStatus(
  seller: SellerStatus,
): ExtendedMakerStatus | null {
  if (seller.type === "Unreachable") {
    return null;
  }

  return {
    maxSwapAmount: seller.content.quote.max_quantity,
    minSwapAmount: seller.content.quote.min_quantity,
    price: seller.content.quote.price,
    peerId: seller.content.peer_id,
    multiAddr: seller.content.multiaddr,
    testnet: isTestnet(),
    version: seller.content.version,
  };
}

export function bytesToMb(bytes: number): number {
  return bytes / (1024 * 1024);
}

/// Get the markup of a maker's exchange rate compared to the market rate in percent
export function getMarkup(makerPrice: number, marketPrice: number): number {
  return ((makerPrice - marketPrice) / marketPrice) * 100;
}

// Updated function to parse 9-element tuple and format it
export function formatDateTime(
  dateTime:
    | [number, number, number, number, number, number, number, number, number]
    | null
    | undefined,
): string {
  if (!dateTime || !Array.isArray(dateTime) || dateTime.length !== 9) {
    // Basic validation for null, undefined, or incorrect structure
    return "Invalid Date Input";
  }

  try {
    const [
      year,
      dayOfYear,
      hour,
      minute,
      second,
      nanoseconds,
      offsetH,
      offsetM,
      offsetS,
    ] = dateTime;

    // More robust validation (example)
    if (
      year < 1970 ||
      dayOfYear < 1 ||
      dayOfYear > 366 ||
      hour < 0 ||
      hour > 23 ||
      minute < 0 ||
      minute > 59 ||
      second < 0 ||
      second > 59 ||
      nanoseconds < 0 ||
      nanoseconds > 999999999
    ) {
      return "Invalid Date Components";
    }

    // Calculate total offset in seconds (handle potential non-zero offsets)
    const totalOffsetSeconds = offsetH * 3600 + offsetM * 60 + offsetS;

    // Calculate milliseconds from nanoseconds
    const milliseconds = Math.floor(nanoseconds / 1_000_000);

    // Create Date object for the start of the year *in UTC*
    const date = new Date(Date.UTC(year, 0, 1)); // Month is 0-indexed (January)

    // Add (dayOfYear - 1) days to get the correct date *in UTC*
    date.setUTCDate(date.getUTCDate() + dayOfYear - 1);

    // Set the time components *in UTC*
    date.setUTCHours(hour);
    date.setUTCMinutes(minute);
    date.setUTCSeconds(second);
    date.setUTCMilliseconds(milliseconds);

    // Adjust for the timezone offset to get the correct UTC time
    // Subtract the offset because Date.UTC assumes UTC, but the components might be for a different offset
    date.setTime(date.getTime() - totalOffsetSeconds * 1000);

    // Final validation
    if (isNaN(date.getTime())) {
      return "Invalid Calculated Date";
    }

    // Format to a readable string (e.g., "YYYY-MM-DD HH:MM:SS UTC")
    const yyyy = date.getUTCFullYear();
    const mm = String(date.getUTCMonth() + 1).padStart(2, "0");
    const dd = String(date.getUTCDate()).padStart(2, "0");
    const HH = String(date.getUTCHours()).padStart(2, "0");
    const MM = String(date.getUTCMinutes()).padStart(2, "0");
    const SS = String(date.getUTCSeconds()).padStart(2, "0");

    return `${yyyy}-${mm}-${dd} ${HH}:${MM}:${SS} UTC`;
  } catch (e) {
    return "Invalid Date Format";
  }
}
