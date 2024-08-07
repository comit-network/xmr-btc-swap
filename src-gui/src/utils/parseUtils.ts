import { CliLog, isCliLog } from 'models/cliModel';

/*
Extract btc amount from string

E.g: "0.00100000 BTC"
Output: 0.001
 */
export function extractAmountFromUnitString(text: string): number | null {
  if (text != null) {
    const parts = text.split(' ');
    if (parts.length === 2) {
      const amount = Number.parseFloat(parts[0]);
      return amount;
    }
  }
  return null;
}

// E.g 2021-12-29 14:25:59.64082 +00:00:00
export function parseDateString(str: string): number {
  const parts = str.split(' ').slice(0, -1);
  if (parts.length !== 2) {
    throw new Error(
      `Date string does not consist solely of date and time Str: ${str} Parts: ${parts}`,
    );
  }
  const wholeString = parts.join(' ');
  const date = Date.parse(wholeString);
  if (Number.isNaN(date)) {
    throw new Error(
      `Date string could not be parsed Str: ${str} Parts: ${parts}`,
    );
  }
  return date;
}

export function getLinesOfString(data: string): string[] {
  return data
    .toString()
    .replace('\r\n', '\n')
    .replace('\r', '\n')
    .split('\n')
    .filter((l) => l.length > 0);
}

export function getLogsAndStringsFromRawFileString(
  rawFileData: string,
): (CliLog | string)[] {
  return getLinesOfString(rawFileData).map((line) => {
    try {
      return JSON.parse(line);
    } catch (e) {
      return line;
    }
  });
}

export function getLogsFromRawFileString(rawFileData: string): CliLog[] {
  return getLogsAndStringsFromRawFileString(rawFileData).filter(isCliLog);
}

export function logsToRawString(logs: (CliLog | string)[]): string {
  return logs.map((l) => JSON.stringify(l)).join('\n');
}
