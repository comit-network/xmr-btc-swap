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
    ? '[57][0-9AB][1-9A-HJ-NP-Za-km-z]{93}'
    : '[48][0-9AB][1-9A-HJ-NP-Za-km-z]{93}';
  return new RegExp(`(?:^${re}$)`).test(address);
}

export function isBtcAddressValid(address: string, testnet: boolean) {
  const re = testnet
    ? '(tb1)[a-zA-HJ-NP-Z0-9]{25,49}'
    : '(bc1)[a-zA-HJ-NP-Z0-9]{25,49}';
  return new RegExp(`(?:^${re}$)`).test(address);
}

export function getBitcoinTxExplorerUrl(txid: string, testnet: boolean) {
  return `https://blockchair.com/bitcoin${
    testnet ? '/testnet' : ''
  }/transaction/${txid}`;
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
