import {
  Box,
  Button,
  Dialog,
  DialogActions,
  DialogContent,
  DialogContentText,
  Link,
} from "@material-ui/core";
import { ButtonProps } from "@material-ui/core/Button/Button";
import { GetSwapInfoArgs } from "models/tauriModel";
import { rpcResetMoneroRecoveryKeys } from "store/features/rpcSlice";
import { useAppDispatch, useAppSelector } from "store/hooks";
import DialogHeader from "../../../modal/DialogHeader";
import ScrollablePaperTextBox from "../../../other/ScrollablePaperTextBox";

function MoneroRecoveryKeysDialog() {
  // TODO: Reimplement this using the new Tauri API
  return null;

  const dispatch = useAppDispatch();
  const keys = useAppSelector((s) => s.rpc.state.moneroRecovery);

  function onClose() {
    dispatch(rpcResetMoneroRecoveryKeys());
  }

  if (keys === null || keys.swapId !== swap.swap_id) {
    return <></>;
  }

  return (
    <Dialog open onClose={onClose} maxWidth="sm" fullWidth>
      <DialogHeader
        title={`Recovery Keys for swap ${swap.swap_id.substring(0, 5)}...`}
      />
      <DialogContent>
        <DialogContentText>
          You can use the keys below to manually redeem the Monero funds from
          the multi-signature wallet.
          <ul>
            <li>
              This is useful if the swap daemon fails to redeem the funds itself
            </li>
            <li>
              If you have come this far, there is no risk of losing funds. You
              are the only one with access to these keys and can use them to
              access your funds
            </li>
            <li>
              View{" "}
              <Link
                href="https://www.getmonero.org/resources/user-guides/restore_from_keys.html"
                target="_blank"
                rel="noreferrer"
              >
                this guide
              </Link>{" "}
              for a detailed description on how to import the keys and spend the
              funds.
            </li>
          </ul>
        </DialogContentText>
        <Box
          style={{
            display: "flex",
            gap: "0.5rem",
            flexDirection: "column",
          }}
        >
          {[
            ["Primary Address", keys.keys.address],
            ["View Key", keys.keys.view_key],
            ["Spend Key", keys.keys.spend_key],
            ["Restore Height", keys.keys.restore_height.toString()],
          ].map(([title, value]) => (
            <ScrollablePaperTextBox
              minHeight="2rem"
              title={title}
              copyValue={value}
              rows={[value]}
            />
          ))}
        </Box>
      </DialogContent>
      <DialogActions>
        <Button onClick={onClose} color="primary" variant="contained">
          Done
        </Button>
      </DialogActions>
    </Dialog>
  );
}

export function SwapMoneroRecoveryButton({
  swap,
  ...props
}: { swap: GetSwapInfoArgs } & ButtonProps) {
  return <> </>;
  /* TODO: Reimplement this using the new Tauri API
  const isRecoverable = isSwapMoneroRecoverable(swap.state_name);


  if (!isRecoverable) {
    return <></>;
  }

  return (
    <>
      <PromiseInvokeButton
        onClick={async () => {
          throw new Error("Not implemented");
        }}
        {...props}
      >
        Display Monero Recovery Keys
      </PromiseInvokeButton>
      <MoneroRecoveryKeysDialog swap={swap} />
    </>
  );
  */
}
