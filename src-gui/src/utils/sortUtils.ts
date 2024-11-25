import { ExtendedMakerStatus } from "models/apiModel";
import { isMakerOnCorrectNetwork, isMakerOutdated } from "./multiAddrUtils";

export function sortMakerList(list: ExtendedMakerStatus[]) {
  return list
    // Filter out makers that are on the wrong network (testnet / mainnet)
    .filter(isMakerOnCorrectNetwork)
    .concat()
    // Sort by criteria
    .sort((firstEl, secondEl) => {
      // If either provider is outdated, prioritize the one that isn't
      if (isMakerOutdated(firstEl) && !isMakerOutdated(secondEl)) return 1;
      if (!isMakerOutdated(firstEl) && isMakerOutdated(secondEl)) return -1;

      // If neither of them have a relevancy score or they are the same, sort by price
      if (firstEl.relevancy == secondEl.relevancy) {
        return firstEl.price - secondEl.price;
      }

      // If only one of the two doesn't have a relevancy score, prioritize the one that does
      if (firstEl.relevancy == null) return 1;
      if (secondEl.relevancy == null) return -1;

      // Otherwise, sort by relevancy score
      return secondEl.relevancy - firstEl.relevancy;
    })
    // Remove duplicate makers
    .filter((provider, index, self) =>
      index === self.findIndex((p) => p.peerId === provider.peerId)
    )
}