import { invoke } from "@tauri-apps/api/core";
import { store } from "./store/storeRenderer";
import { rpcSetBalance, rpcSetSwapInfo } from "store/features/rpcSlice";

export async function checkBitcoinBalance() {
  // TODO: use tauri-bindgen here
  const response = (await invoke("balance")) as {
    balance: number;
  };

  store.dispatch(rpcSetBalance(response.balance));
}

export async function getRawSwapInfos() {
  const response = await invoke("swap_infos_all");

  (response as any[]).forEach((info) => store.dispatch(rpcSetSwapInfo(info)));
}
