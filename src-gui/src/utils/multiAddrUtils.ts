import { ExtendedProviderStatus, Provider } from "models/apiModel";
import { Multiaddr } from "multiaddr";
import semver from "semver";
import { isTestnet } from "store/config";

const MIN_ASB_VERSION = "0.13.3";

export function providerToConcatenatedMultiAddr(provider: Provider) {
  return new Multiaddr(provider.multiAddr)
    .encapsulate(`/p2p/${provider.peerId}`)
    .toString();
}

export function isProviderCompatible(
  provider: ExtendedProviderStatus,
): boolean {
  return provider.testnet === isTestnet();
}

export function isProviderOutdated(provider: ExtendedProviderStatus): boolean {
  if (provider.version != null) {
    if (semver.satisfies(provider.version, `>=${MIN_ASB_VERSION}`))
      return false;
  } else {
    return false;
  }

  return true;
}