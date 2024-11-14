import { ExtendedProviderStatus } from "models/apiModel";
import { isProviderCompatible, isProviderOutdated } from "./multiAddrUtils";

export function sortProviderList(list: ExtendedProviderStatus[]) {
  return list
    .filter(isProviderCompatible)
    .concat()
    .sort((firstEl, secondEl) => {
      // If either provider is outdated, prioritize the one that isn't
      if (isProviderOutdated(firstEl) && !isProviderOutdated(secondEl)) return 1;
      if (!isProviderOutdated(firstEl) && isProviderOutdated(secondEl)) return -1;

      // If neither of them have a relevancy score, sort by price
      if (firstEl.relevancy == null && secondEl.relevancy == null) {
        return firstEl.price - secondEl.price;
      }

      // If only on of the two don't have a relevancy score, prioritize the one that does
      if (firstEl.relevancy == null) return 1;
      if (secondEl.relevancy == null) return -1;

      // Otherwise, sort by relevancy score
      if (firstEl.relevancy > secondEl.relevancy) {
        return -1;
      }
      return 1;
    });
}