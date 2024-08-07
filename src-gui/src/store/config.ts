import { ExtendedProviderStatus } from 'models/apiModel';

export const isTestnet = () =>
  false

export const isExternalRpc = () =>
  true

export const isDevelopment =
  true

export function getStubTestnetProvider(): ExtendedProviderStatus | null {
  return null;
}

export const getPlatform = () => {
  return 'mac';
};
