import { createSlice, PayloadAction } from "@reduxjs/toolkit";
import { Blockchain } from "./settingsSlice";

export interface NodesSlice {
  nodes: Record<Blockchain, Record<string, boolean>>;
}

function initialState(): NodesSlice {
  return {
    nodes: {
      [Blockchain.Bitcoin]: {},
      [Blockchain.Monero]: {},
    },
  }
}

const nodesSlice = createSlice({
  name: "nodes",
  initialState: initialState(),
  reducers: {
    setStatus(slice, action: PayloadAction<{
      node: string,
      status: boolean,
      blockchain: Blockchain,
    }>) {
      slice.nodes[action.payload.blockchain][action.payload.node] = action.payload.status;
    },
    resetStatuses(slice) {
      slice.nodes = {
        [Blockchain.Bitcoin]: {},
        [Blockchain.Monero]: {},
      }
    },
  },
});

export const { setStatus, resetStatuses } = nodesSlice.actions;
export default nodesSlice.reducer;
