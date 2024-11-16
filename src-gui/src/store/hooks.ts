import { sortBy } from "lodash";
import { BobStateName, GetSwapInfoResponseExt } from "models/tauriModelExt";
import { TypedUseSelectorHook, useDispatch, useSelector } from "react-redux";
import type { AppDispatch, RootState } from "renderer/store/storeRenderer";
import { parseDateString } from "utils/parseUtils";
import { useMemo } from "react";
import { isCliLogRelatedToSwap } from "models/cliModel";
import { SettingsState } from "./features/settingsSlice";
import { NodesSlice } from "./features/nodesSlice";
import { RatesState } from "./features/ratesSlice";
import { sortProviderList } from "utils/sortUtils";

export const useAppDispatch = () => useDispatch<AppDispatch>();
export const useAppSelector: TypedUseSelectorHook<RootState> = useSelector;

export function useResumeableSwapsCount(
  additionalFilter?: (s: GetSwapInfoResponseExt) => boolean,
) {
  return useAppSelector(
    (state) =>
      Object.values(state.rpc.state.swapInfos).filter(
        (swapInfo: GetSwapInfoResponseExt) =>
          !swapInfo.completed && (additionalFilter == null || additionalFilter(swapInfo))
      ).length,
  );
}


export function useResumeableSwapsCountExcludingPunished() {
  return useResumeableSwapsCount(
    (s) => s.state_name !== BobStateName.BtcPunished,
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
    const all = [...registryProviders, ...listSellersProviders];

    return sortProviderList(all);
  });
}

export function useSwapInfosSortedByDate() {
  const swapInfos = useAppSelector((state) => state.rpc.state.swapInfos);

  return sortBy(
    Object.values(swapInfos),
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
