import { ExtendedMakerStatus } from "models/apiModel";
import { isMakerOnCorrectNetwork, isMakerOutdated } from "./multiAddrUtils";
import _ from "lodash";

export function sortMakerList(list: ExtendedMakerStatus[]) {
  return (
    _(list)
      // Filter out makers that are on the wrong network (testnet / mainnet)
      .filter(isMakerOnCorrectNetwork)
      // Sort by criteria
      .orderBy(
        [
          // Prefer makers that have a 'version' attribute
          // If we don't have a version, we cannot clarify if it's outdated or not
          (m) => (m.version ? 0 : 1),
          // Prefer makers that are not outdated
          (m) => (isMakerOutdated(m) ? 1 : 0),
          // Prefer makers that have a relevancy score
          (m) => (m.relevancy == null ? 1 : 0),
          // Prefer makers with a higher relevancy score
          (m) => -(m.relevancy ?? 0),
          // Prefer makers with a minimum quantity > 0
          (m) => ((m.minSwapAmount ?? 0) > 0 ? 0 : 1),
          // Prefer makers with a lower price
          (m) => m.price,
        ],
        ["asc", "asc", "asc", "asc", "asc"],
      )
      // Remove duplicate makers
      .uniqBy((m) => m.peerId)
      .value()
  );
}
