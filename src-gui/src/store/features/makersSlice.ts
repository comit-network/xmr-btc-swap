import { createSlice, PayloadAction } from "@reduxjs/toolkit";
import { ExtendedMakerStatus, MakerStatus } from "models/apiModel";
import { SellerStatus } from "models/tauriModel";
import { getStubTestnetMaker } from "store/config";
import { rendezvousSellerToMakerStatus } from "utils/conversionUtils";
import { isMakerOutdated } from "utils/multiAddrUtils";
import { sortMakerList } from "utils/sortUtils";

const stubTestnetMaker = getStubTestnetMaker();

export interface MakersSlice {
  rendezvous: {
    makers: (ExtendedMakerStatus | MakerStatus)[];
  };
  registry: {
    makers: ExtendedMakerStatus[] | null;
    // This counts how many failed connections attempts we have counted since the last successful connection
    connectionFailsCount: number;
  };
  selectedMaker: ExtendedMakerStatus | null;
}

const initialState: MakersSlice = {
  rendezvous: {
    makers: [],
  },
  registry: {
    makers: stubTestnetMaker ? [stubTestnetMaker] : null,
    connectionFailsCount: 0,
  },
  selectedMaker: null,
};

function selectNewSelectedMaker(
  slice: MakersSlice,
  peerId?: string,
): MakerStatus {
  const selectedPeerId = peerId || slice.selectedMaker?.peerId;

  // Check if we still have a record of the currently selected provider
  const currentMaker =
    slice.registry.makers?.find((prov) => prov.peerId === selectedPeerId) ||
    slice.rendezvous.makers.find((prov) => prov.peerId === selectedPeerId);

  // If the currently selected provider is not outdated, keep it
  if (currentMaker != null && !isMakerOutdated(currentMaker)) {
    return currentMaker;
  }

  // Otherwise we'd prefer to switch to a provider that has the newest version
  const providers = sortMakerList([
    ...(slice.registry.makers ?? []),
    ...(slice.rendezvous.makers ?? []),
  ]);

  return providers.at(0) || null;
}

export const makersSlice = createSlice({
  name: "providers",
  initialState,
  reducers: {
    discoveredMakersByRendezvous(slice, action: PayloadAction<SellerStatus[]>) {
      action.payload.forEach((discoveredSeller) => {
        const discoveredMakerStatus =
          rendezvousSellerToMakerStatus(discoveredSeller);

        // If the seller has a status of "Unreachable" the provider is not added to the list
        if (discoveredMakerStatus === null) {
          return;
        }

        // If the provider was already discovered via the public registry, don't add it again
        const indexOfExistingMaker = slice.rendezvous.makers.findIndex(
          (prov) =>
            prov.peerId === discoveredMakerStatus.peerId &&
            prov.multiAddr === discoveredMakerStatus.multiAddr,
        );

        // Avoid duplicate entries, replace them instead
        if (indexOfExistingMaker !== -1) {
          slice.rendezvous.makers[indexOfExistingMaker] = discoveredMakerStatus;
        } else {
          slice.rendezvous.makers.push(discoveredMakerStatus);
        }
      });

      // Sort the provider list and select a new provider if needed
      slice.rendezvous.makers = sortMakerList(slice.rendezvous.makers);
      slice.selectedMaker = selectNewSelectedMaker(slice);
    },
    setRegistryMakers(slice, action: PayloadAction<ExtendedMakerStatus[]>) {
      if (stubTestnetMaker) {
        action.payload.push(stubTestnetMaker);
      }

      // Sort the provider list and select a new provider if needed
      slice.registry.makers = sortMakerList(action.payload);
      slice.selectedMaker = selectNewSelectedMaker(slice);
    },
    registryConnectionFailed(slice) {
      slice.registry.connectionFailsCount += 1;
    },
    setSelectedMaker(
      slice,
      action: PayloadAction<{
        peerId: string;
      }>,
    ) {
      slice.selectedMaker = selectNewSelectedMaker(
        slice,
        action.payload.peerId,
      );
    },
  },
});

export const {
  discoveredMakersByRendezvous,
  setRegistryMakers,
  registryConnectionFailed,
  setSelectedMaker,
} = makersSlice.actions;

export default makersSlice.reducer;
