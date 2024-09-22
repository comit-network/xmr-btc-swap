import { ExtendedProviderStatus } from "models/apiModel";
import { splitPeerIdFromMultiAddress } from "utils/parseUtils";

export const isTestnet = () => true;

export const isDevelopment = true;

export function getStubTestnetProvider(): ExtendedProviderStatus | null {
  const stubProviderAddress = process.env.TESTNET_STUB_PROVIDER_ADDRESS;

  if(stubProviderAddress != null) {
    try {
      const [multiAddr, peerId] = splitPeerIdFromMultiAddress(stubProviderAddress);

      return {
        multiAddr,
        testnet: true,
        peerId,
        maxSwapAmount: 0,
        minSwapAmount: 0,
        price: 0,
      };
    }catch {
      return null;
    }
  }

  return null;
}
