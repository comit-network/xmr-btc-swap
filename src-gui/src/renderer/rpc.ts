import { invoke } from "@tauri-apps/api/core";
import { store } from "./store/storeRenderer";
import { rpcSetBalance, rpcSetSwapInfo } from "store/features/rpcSlice";

export async function checkBitcoinBalance() {
  const response = (await invoke("get_balance")) as {
    balance: number;
  };

  store.dispatch(rpcSetBalance(response.balance));
}

export async function getRawSwapInfos() {
  const response = await invoke("get_swap_infos_all");

  (response as any[]).forEach((info) => store.dispatch(rpcSetSwapInfo(info)));
}

export async function withdrawBtc(address: string): Promise<string> {
  const response = (await invoke("withdraw_btc", {
    args: {
      address,
      amount: null,
    },
  })) as {
    txid: string;
    amount: number;
  };

  return response.txid;
}
