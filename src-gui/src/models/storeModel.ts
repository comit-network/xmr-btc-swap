import { CliLog, SwapSpawnType } from "./cliModel";
import { TauriSwapProgressEvent } from "./tauriModel";

export interface SwapSlice {
  state: {
    curr: TauriSwapProgressEvent;
    prev: TauriSwapProgressEvent | null;
    swapId: string;
  } | null;
  logs: CliLog[];
  spawnType: SwapSpawnType | null;
}
