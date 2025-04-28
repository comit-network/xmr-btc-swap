import { sortBy } from "lodash";
import { BobStateName, GetSwapInfoResponseExt, PendingApprovalRequest, PendingLockBitcoinApprovalRequest } from "models/tauriModelExt";
import { TypedUseSelectorHook, useDispatch, useSelector } from "react-redux";
import type { AppDispatch, RootState } from "renderer/store/storeRenderer";
import { parseDateString } from "utils/parseUtils";
import { useMemo } from "react";
import { isCliLogRelatedToSwap } from "models/cliModel";
import { SettingsState } from "./features/settingsSlice";
import { NodesSlice } from "./features/nodesSlice";
import { RatesState } from "./features/ratesSlice";
import { sortMakerList } from "utils/sortUtils";

export const useAppDispatch = () => useDispatch<AppDispatch>();
export const useAppSelector: TypedUseSelectorHook<RootState> = useSelector;

export function useResumeableSwapsCount(
  additionalFilter?: (s: GetSwapInfoResponseExt) => boolean,
) {
  const saneSwapInfos = useSaneSwapInfos();

  return useAppSelector(
    (state) =>
      saneSwapInfos.filter(
        (swapInfo: GetSwapInfoResponseExt) =>
          !swapInfo.completed && (additionalFilter == null || additionalFilter(swapInfo))
      ).length,
  );
}

/**
 * Counts the number of resumeable swaps excluding:
 * - Punished swaps
 * - Swaps where the sanity check was not passed (e.g. they were aborted)
 */
export function useResumeableSwapsCountExcludingPunished() {
  return useResumeableSwapsCount(
    (s) => s.state_name !== BobStateName.BtcPunished && s.state_name !== BobStateName.SwapSetupCompleted,
  );
}

/// Returns true if we have a swap that is running
export function useIsSwapRunning() {
  return useAppSelector(
    (state) =>
      state.swap.state !== null && state.swap.state.curr.type !== "Released",
  );
}

export function useIsContextAvailable() {
  return useAppSelector((state) => state.rpc.status?.type === "Available");
}

/// We do not use a sanity check here, as opposed to the other useSwapInfo hooks,
/// because we are explicitly asking for a specific swap
export function useSwapInfo(
  swapId: string | null,
): GetSwapInfoResponseExt | null {
  return useAppSelector((state) =>
    swapId ? state.rpc.state.swapInfos[swapId] ?? null : null,
  );
}

export function useActiveSwapId(): string | null {
  return useAppSelector((s) => s.swap.state?.swapId ?? null);
}

export function useActiveSwapInfo(): GetSwapInfoResponseExt | null {
  const swapId = useActiveSwapId();
  return useSwapInfo(swapId);
}

export function useActiveSwapLogs() {
  const swapId = useActiveSwapId();
  const logs = useAppSelector((s) => s.rpc.logs);

  return useMemo(
    () => logs.filter((log) => isCliLogRelatedToSwap(log, swapId)),
    [logs, swapId],
  );
}

export function useAllMakers() {
  return useAppSelector((state) => {
    const registryMakers = state.makers.registry.makers || [];
    const listSellersMakers = state.makers.rendezvous.makers || [];
    const all = [...registryMakers, ...listSellersMakers];

    return sortMakerList(all);
  });
}

/// This hook returns the all swap infos, as an array
/// Excluding those who are in a state where it's better to hide them from the user
export function useSaneSwapInfos() {
  const swapInfos = useAppSelector((state) => state.rpc.state.swapInfos);
  return Object.values(swapInfos).filter((swap) => {
    // We hide swaps that are in the SwapSetupCompleted state
    // This is because they are probably ones where:
    // 1. The user force stopped the swap while we were waiting for their confirmation of the offer
    // 2. We where therefore unable to transition to SafelyAborted
    if (swap.state_name === BobStateName.SwapSetupCompleted) {
      return false;
    }

    // We hide swaps that were safely aborted
    // No funds were locked. Cannot be resumed.
    // Wouldn't be beneficial to show them to the user
    if (swap.state_name === BobStateName.SafelyAborted) {
      return false;
    }

    return true;
  });
}

/// This hook returns the swap infos sorted by date
export function useSwapInfosSortedByDate() {
  const swapInfos = useSaneSwapInfos();

  return sortBy(
    swapInfos,
    (swap) => -parseDateString(swap.start_date),
  );
}

export function useRates<T>(selector: (rates: RatesState) => T): T {
  const rates = useAppSelector((state) => state.rates);
  return selector(rates);
}

export function useSettings<T>(selector: (settings: SettingsState) => T): T {
  const settings = useAppSelector((state) => state.settings);
  return selector(settings);
}

export function useNodes<T>(selector: (nodes: NodesSlice) => T): T {
  const nodes = useAppSelector((state) => state.nodes);
  return selector(nodes);
}

export function usePendingApprovals(): PendingApprovalRequest[] {
  const approvals = useAppSelector((state) => state.rpc.state.approvalRequests);
  return Object.values(approvals).filter((c) => c.state === "Pending");
}

export function usePendingLockBitcoinApproval(): PendingLockBitcoinApprovalRequest[] {
  const approvals = usePendingApprovals();
  return approvals.filter((c) => c.content.details.type === "LockBitcoin");
}

/**
 * Calculates the number of unread messages from staff for a specific feedback conversation.
 * @param feedbackId The ID of the feedback conversation.
 * @returns The number of unread staff messages.
 */
export function useUnreadMessagesCount(feedbackId: string): number {
  const { conversationsMap, seenMessagesSet } = useAppSelector((state) => ({
    conversationsMap: state.conversations.conversations,
    // Convert seenMessages array to a Set for efficient lookup
    seenMessagesSet: new Set(state.conversations.seenMessages),
  }));

  const messages = conversationsMap[feedbackId] || [];

  const unreadStaffMessages = messages.filter(
    (msg) => msg.is_from_staff && !seenMessagesSet.has(msg.id.toString()),
  );

  return unreadStaffMessages.length;
}

/**
 * Calculates the total number of unread messages from staff across all feedback conversations.
 * @returns The total number of unread staff messages.
 */
export function useTotalUnreadMessagesCount(): number {
  const { conversationsMap, seenMessagesSet } = useAppSelector((state) => ({
    conversationsMap: state.conversations.conversations,
    seenMessagesSet: new Set(state.conversations.seenMessages),
  }));

  let totalUnreadCount = 0;
  for (const feedbackId in conversationsMap) {
    const messages = conversationsMap[feedbackId] || [];
    const unreadStaffMessages = messages.filter(
      (msg) => msg.is_from_staff && !seenMessagesSet.has(msg.id.toString()),
    );
    totalUnreadCount += unreadStaffMessages.length;
  }

  return totalUnreadCount;
}