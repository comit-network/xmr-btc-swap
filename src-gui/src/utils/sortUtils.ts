import {
  PendingSelectMakerApprovalRequest,
  SortableQuoteWithAddress,
} from "models/tauriModelExt";
import { QuoteWithAddress } from "models/tauriModel";
import { isMakerVersionOutdated } from "./multiAddrUtils";
import _ from "lodash";

export function sortApprovalsAndKnownQuotes(
  pendingSelectMakerApprovals: PendingSelectMakerApprovalRequest[],
  known_quotes: QuoteWithAddress[],
) {
  const sortableQuotes = pendingSelectMakerApprovals.map((approval) => {
    return {
      ...approval.request.content.maker,
      expiration_ts:
        approval.request_status.state === "Pending"
          ? approval.request_status.content.expiration_ts
          : undefined,
      request_id: approval.request_id,
    } as SortableQuoteWithAddress;
  });

  sortableQuotes.push(
    ...known_quotes.map((quote) => ({
      ...quote,
      request_id: null,
    })),
  );

  return sortMakerApprovals(sortableQuotes);
}

export function sortMakerApprovals(list: SortableQuoteWithAddress[]) {
  return (
    _(list)
      .orderBy(
        [
          // Prefer makers that have a 'version' attribute
          // If we don't have a version, we cannot clarify if it's outdated or not
          (m) => (m.version ? 0 : 1),
          // Prefer makers that are not outdated
          (m) => (isMakerVersionOutdated(m.version) ? 1 : 0),
          // Prefer makers with a minimum quantity > 0
          (m) => ((m.quote.min_quantity ?? 0) > 0 ? 0 : 1),
          // Prefer approvals over actual quotes
          (m) => (m.request_id ? 0 : 1),
          // Prefer makers with a lower price
          (m) => m.quote.price,
        ],
        ["asc", "asc", "asc", "asc", "asc"],
      )
      // Remove duplicate makers
      .uniqBy((m) => m.peer_id)
      .value()
  );
}
