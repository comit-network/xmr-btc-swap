import { createSlice, PayloadAction } from "@reduxjs/toolkit";
import { ExtendedProviderStatus, ProviderStatus } from "models/apiModel";
import { Seller } from "models/tauriModel";
import { getStubTestnetProvider } from "store/config";
import { rendezvousSellerToProviderStatus } from "utils/conversionUtils";
import { isProviderOutdated } from "utils/multiAddrUtils";
import { sortProviderList } from "utils/sortUtils";

const stubTestnetProvider = getStubTestnetProvider();

export interface ProvidersSlice {
  rendezvous: {
    providers: (ExtendedProviderStatus | ProviderStatus)[];
  };
  registry: {
    providers: ExtendedProviderStatus[] | null;
    // This counts how many failed connections attempts we have counted since the last successful connection
    connectionFailsCount: number;
  };
  selectedProvider: ExtendedProviderStatus | null;
}

const initialState: ProvidersSlice = {
  rendezvous: {
    providers: [],
  },
  registry: {
    providers: stubTestnetProvider ? [stubTestnetProvider] : null,
    connectionFailsCount: 0,
  },
  selectedProvider: null,
};

function selectNewSelectedProvider(
  slice: ProvidersSlice,
  peerId?: string,
): ProviderStatus {
  const selectedPeerId = peerId || slice.selectedProvider?.peerId;

  // Check if we still have a record of the currently selected provider
  const currentProvider = slice.registry.providers?.find((prov) => prov.peerId === selectedPeerId) || slice.rendezvous.providers.find((prov) => prov.peerId === selectedPeerId);

  // If the currently selected provider is not outdated, keep it
  if (currentProvider != null && !isProviderOutdated(currentProvider)) {
    return currentProvider;
  }

  // Otherwise we'd prefer to switch to a provider that has the newest version
  const providers = sortProviderList([
    ...(slice.registry.providers ?? []),
    ...(slice.rendezvous.providers ?? []),
  ]);

  return providers.at(0) || null;
}

export const providersSlice = createSlice({
  name: "providers",
  initialState,
  reducers: {
    discoveredProvidersByRendezvous(slice, action: PayloadAction<Seller[]>) {
      action.payload.forEach((discoveredSeller) => {
        const discoveredProviderStatus =
          rendezvousSellerToProviderStatus(discoveredSeller);

        // If the seller has a status of "Unreachable" the provider is not added to the list
        if (discoveredProviderStatus === null) {
          return;
        }

        // If the provider was already discovered via the public registry, don't add it again
        const indexOfExistingProvider = slice.rendezvous.providers.findIndex(
          (prov) =>
            prov.peerId === discoveredProviderStatus.peerId &&
            prov.multiAddr === discoveredProviderStatus.multiAddr,
        );

        // Avoid duplicate entries, replace them instead
        if (indexOfExistingProvider !== -1) {
          slice.rendezvous.providers[indexOfExistingProvider] =
            discoveredProviderStatus;
        } else {
          slice.rendezvous.providers.push(discoveredProviderStatus);
        }
      });

      // Sort the provider list and select a new provider if needed
      slice.rendezvous.providers = sortProviderList(slice.rendezvous.providers);
      slice.selectedProvider = selectNewSelectedProvider(slice);
    },
    setRegistryProviders(
      slice,
      action: PayloadAction<ExtendedProviderStatus[]>,
    ) {
      if (stubTestnetProvider) {
        action.payload.push(stubTestnetProvider);
      }

      // Sort the provider list and select a new provider if needed
      slice.registry.providers = sortProviderList(action.payload);
      slice.selectedProvider = selectNewSelectedProvider(slice);
    },
    registryConnectionFailed(slice) {
      slice.registry.connectionFailsCount += 1;
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
  registryConnectionFailed,
  setSelectedProvider,
} = providersSlice.actions;

export default providersSlice.reducer;
