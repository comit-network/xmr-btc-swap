import { CliLog, SwapSpawnType } from "./cliModel";
import { TauriSwapProgressEvent } from "./tauriModel";

export type SwapState = {
  curr: TauriSwapProgressEvent;
  prev: TauriSwapProgressEvent | null;
  swapId: string;
};

export interface SwapSlice {
  state: SwapState | null;
  logs: CliLog[];
  spawnType: SwapSpawnType | null;
}
