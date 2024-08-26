import { ExtendedProviderStatus, Provider } from "models/apiModel";
import { Multiaddr } from "multiaddr";
import semver from "semver";
import { isTestnet } from "store/config";

const MIN_ASB_VERSION = "0.12.0";

export function providerToConcatenatedMultiAddr(provider: Provider) {
  return new Multiaddr(provider.multiAddr)
    .encapsulate(`/p2p/${provider.peerId}`)
    .toString();
}

export function isProviderCompatible(
  provider: ExtendedProviderStatus,
): boolean {
  if (provider.version) {
    if (!semver.satisfies(provider.version, `>=${MIN_ASB_VERSION}`))
      return false;
  }
  if (provider.testnet !== isTestnet()) return false;

  return true;
}
