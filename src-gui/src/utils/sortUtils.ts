import { ExtendedProviderStatus } from "models/apiModel";
import { isProviderCompatible } from "./multiAddrUtils";

export function sortProviderList(list: ExtendedProviderStatus[]) {
  return list
    .filter(isProviderCompatible)
    .concat()
    .sort((firstEl, secondEl) => {
      // If neither of them have a relevancy score, sort by max swap amount
      if (firstEl.relevancy === undefined && secondEl.relevancy === undefined) {
        if (firstEl.maxSwapAmount > secondEl.maxSwapAmount) {
          return -1;
        }
      }
      // If only on of the two don't have a relevancy score, prioritize the one that does
      if (firstEl.relevancy === undefined) return 1;
      if (secondEl.relevancy === undefined) return -1;
      if (firstEl.relevancy > secondEl.relevancy) {
        return -1;
      }
      return 1;
    });
}