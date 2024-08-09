import { Button, CircularProgress, IconButton } from "@material-ui/core";
import RefreshIcon from "@material-ui/icons/Refresh";
import IpcInvokeButton from "../../IpcInvokeButton";
import { checkBitcoinBalance } from "renderer/rpc";
import PromiseInvokeButton from "renderer/components/PromiseInvokeButton";

export default function WalletRefreshButton() {
  return (
    <PromiseInvokeButton
      endIcon={<RefreshIcon />}
      isIconButton
      onClick={() => checkBitcoinBalance()}
      size="small"
    />
  );
}
