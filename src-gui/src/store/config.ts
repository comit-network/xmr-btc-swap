import { ExtendedProviderStatus } from "models/apiModel";

export const isTestnet = () => true;

export const isDevelopment = true;

export function getStubTestnetProvider(): ExtendedProviderStatus | null {
  return null;
}
