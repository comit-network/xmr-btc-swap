import { CliLog, parseCliLogString } from "models/cliModel";
import { Multiaddr } from "multiaddr";

/*
Extract btc amount from string

E.g: "0.00100000 BTC"
Output: 0.001
 */
export function extractAmountFromUnitString(text: string): number | null {
  if (text != null) {
    const parts = text.split(" ");
    if (parts.length === 2) {
      const amount = Number.parseFloat(parts[0]);
      return amount;
    }
  }
  return null;
}

// E.g: 2024-08-19 6:11:37.475038 +00:00:00
export function parseDateString(str: string): number {
  // Split the string and take only the date and time parts
  const [datePart, timePart] = str.split(" ");

  if (!datePart || !timePart) {
    throw new Error(`Invalid date string format: ${str}`);
  }

  // Parse time part
  const [hours, minutes, seconds] = timePart.split(":");
  const paddedHours = hours.padStart(2, "0"); // Ensure two-digit hours

  // Combine date and time parts, ensuring two-digit hours
  const dateTimeString = `${datePart}T${paddedHours}:${minutes}:${seconds.split(".")[0]}Z`;

  const date = Date.parse(dateTimeString);

  if (Number.isNaN(date)) {
    throw new Error(`Date string could not be parsed: ${str}`);
  }

  return date;
}

export function getLinesOfString(data: string): string[] {
  return data
    .toString()
    .replace("\r\n", "\n")
    .replace("\r", "\n")
    .split("\n")
    .filter((l) => l.length > 0);
}

export function getLogsAndStringsFromRawFileString(
  rawFileData: string,
): (CliLog | string)[] {
  return getLinesOfString(rawFileData).map(parseCliLogString);
}

export function logsToRawString(logs: (CliLog | string)[]): string {
  return logs.map((l) => JSON.stringify(l)).join("\n");
}

// This function checks if a given multi address string is a valid multi address
// and contains a peer ID component.
export function isValidMultiAddressWithPeerId(
  multiAddressStr: string,
): boolean {
  try {
    const multiAddress = new Multiaddr(multiAddressStr);
    const peerId = multiAddress.getPeerId();

    return peerId !== null;
  } catch {
    return false;
  }
}

// This function splits a multi address string into the multi address and peer ID components.
// It throws an error if the multi address string is invalid or does not contain a peer ID component.
export function splitPeerIdFromMultiAddress(
  multiAddressStr: string,
): [multiAddress: string, peerId: string] {
  const multiAddress = new Multiaddr(multiAddressStr);

  // Extract the peer ID
  const peerId = multiAddress.getPeerId();

  if (peerId) {
    // Remove the peer ID component
    const p2pMultiaddr = new Multiaddr("/p2p/" + peerId);
    const multiAddressWithoutPeerId = multiAddress.decapsulate(p2pMultiaddr);
    return [multiAddressWithoutPeerId.toString(), peerId];
  } else {
    throw new Error("No peer id encapsulated in multi address");
  }
}
