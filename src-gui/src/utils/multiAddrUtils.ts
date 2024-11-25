import { ExtendedMakerStatus, Maker } from "models/apiModel";
import { Multiaddr } from "multiaddr";
import semver from "semver";
import { isTestnet } from "store/config";

const MIN_ASB_VERSION = "1.0.0-alpha.1"

export function providerToConcatenatedMultiAddr(provider: Maker) {
  return new Multiaddr(provider.multiAddr)
    .encapsulate(`/p2p/${provider.peerId}`)
    .toString();
}

export function isMakerOnCorrectNetwork(
  provider: ExtendedMakerStatus,
): boolean {
  return provider.testnet === isTestnet();
}

export function isMakerOutdated(provider: ExtendedMakerStatus): boolean {
  if (provider.version != null) {
    if (semver.satisfies(provider.version, `>=${MIN_ASB_VERSION}`))
      return false;
  } else {
    return false;
  }

  return true;
}
