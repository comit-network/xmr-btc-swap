import { createSlice, PayloadAction } from "@reduxjs/toolkit";
import { ExtendedProviderStatus, ProviderStatus } from "models/apiModel";
import { getStubTestnetProvider } from "store/config";
import { isProviderCompatible } from "utils/multiAddrUtils";
import { sortProviderList } from "utils/sortUtils";

const stubTestnetProvider = getStubTestnetProvider();

export interface ProvidersSlice {
  rendezvous: {
    providers: (ExtendedProviderStatus | ProviderStatus)[];
  };
  registry: {
    providers: ExtendedProviderStatus[] | null;
    failedReconnectAttemptsSinceLastSuccess: number;
  };
  selectedProvider: ExtendedProviderStatus | null;
}

const initialState: ProvidersSlice = {
  rendezvous: {
    providers: [],
  },
  registry: {
    providers: stubTestnetProvider ? [stubTestnetProvider] : null,
    failedReconnectAttemptsSinceLastSuccess: 0,
  },
  selectedProvider: null,
};

function selectNewSelectedProvider(
  slice: ProvidersSlice,
  peerId?: string,
): ProviderStatus {
  const selectedPeerId = peerId || slice.selectedProvider?.peerId;

  return (
    slice.registry.providers?.find((prov) => prov.peerId === selectedPeerId) ||
    slice.rendezvous.providers.find((prov) => prov.peerId === selectedPeerId) ||
    slice.registry.providers?.at(0) ||
    slice.rendezvous.providers[0] ||
    null
  );
}

export const providersSlice = createSlice({
  name: "providers",
  initialState,
  reducers: {
    discoveredProvidersByRendezvous(
      slice,
      action: PayloadAction<ProviderStatus[]>,
    ) {
      action.payload.forEach((discoveredProvider) => {
        if (
          !slice.registry.providers?.some(
            (prov) =>
              prov.peerId === discoveredProvider.peerId &&
              prov.multiAddr === discoveredProvider.multiAddr,
          )
        ) {
          const indexOfExistingProvider = slice.rendezvous.providers.findIndex(
            (prov) =>
              prov.peerId === discoveredProvider.peerId &&
              prov.multiAddr === discoveredProvider.multiAddr,
          );

          // Avoid duplicates, replace instead
          if (indexOfExistingProvider !== -1) {
            slice.rendezvous.providers[indexOfExistingProvider] =
              discoveredProvider;
          } else {
            slice.rendezvous.providers.push(discoveredProvider);
          }
        }
      });

      slice.rendezvous.providers = sortProviderList(slice.rendezvous.providers);
    },
    setRegistryProviders(
      slice,
      action: PayloadAction<ExtendedProviderStatus[]>,
    ) {
      if (stubTestnetProvider) {
        action.payload.push(stubTestnetProvider);
      }

      slice.registry.providers = sortProviderList(action.payload).filter(
        isProviderCompatible,
      );
      slice.selectedProvider = selectNewSelectedProvider(slice);
    },
    increaseFailedRegistryReconnectAttemptsSinceLastSuccess(slice) {
      slice.registry.failedReconnectAttemptsSinceLastSuccess += 1;
    },
    setSelectedProvider(
      slice,
      action: PayloadAction<{
        peerId: string;
      }>,
    ) {
      slice.selectedProvider = selectNewSelectedProvider(
        slice,
        action.payload.peerId,
      );
    },
  },
});

export const {
  discoveredProvidersByRendezvous,
  setRegistryProviders,
  increaseFailedRegistryReconnectAttemptsSinceLastSuccess,
  setSelectedProvider,
} = providersSlice.actions;

export default providersSlice.reducer;
