import { sortBy } from "lodash";
import { GetSwapInfoResponseExt } from "models/tauriModelExt";
import { TypedUseSelectorHook, useDispatch, useSelector } from "react-redux";
import type { AppDispatch, RootState } from "renderer/store/storeRenderer";
import { parseDateString } from "utils/parseUtils";
import { useMemo } from "react";
import { isCliLogRelatedToSwap } from "models/cliModel";
import { TauriSettings } from "models/tauriModel";

export const useAppDispatch = () => useDispatch<AppDispatch>();
export const useAppSelector: TypedUseSelectorHook<RootState> = useSelector;

export function useResumeableSwapsCount() {
  return useAppSelector(
    (state) =>
      Object.values(state.rpc.state.swapInfos).filter(
        (swapInfo) => !swapInfo.completed,
      ).length,
  );
}

export function useIsSwapRunning() {
  return useAppSelector(
    (state) =>
      state.swap.state !== null && state.swap.state.curr.type !== "Released",
  );
}

export function useIsContextAvailable() {
  return useAppSelector((state) => state.rpc.status?.type === "Available");
}

export function useSwapInfo(
  swapId: string | null,
): GetSwapInfoResponseExt | null {
  return useAppSelector((state) =>
    swapId ? state.rpc.state.swapInfos[swapId] ?? null : null,
  );
}

export function useActiveSwapId() {
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

export function useAllProviders() {
  return useAppSelector((state) => {
    const registryProviders = state.providers.registry.providers || [];
    const listSellersProviders = state.providers.rendezvous.providers || [];
    return [...registryProviders, ...listSellersProviders];
  });
}

export function useSwapInfosSortedByDate() {
  const swapInfos = useAppSelector((state) => state.rpc.state.swapInfos);

  return sortBy(
    Object.values(swapInfos),
    (swap) => -parseDateString(swap.start_date),
  );
}

export function useSettings<T>(selector: (settings: TauriSettings) => T): T {
  return useAppSelector((state) => selector(state.settings));
}