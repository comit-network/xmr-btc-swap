import { CircularProgress } from "@material-ui/core";
import { Alert, AlertProps } from "@material-ui/lab";
import { TauriContextInitializationProgress } from "models/tauriModel";
import { useState } from "react";
import { useAppSelector } from "store/hooks";
import { exhaustiveGuard } from "utils/typescriptUtils";

const FUNNY_INIT_MESSAGES = [
  "Initializing quantum entanglement...",
  "Generating one-time pads from cosmic background radiation...",
  "Negotiating key exchange with aliens...",
  "Optimizing elliptic curves for maximum sneakiness...",
  "Transforming plaintext into ciphertext via arcane XOR rituals...",
  "Salting your hash with exotic mathematical seasonings...",
  "Performing advanced modular arithmetic gymnastics...",
  "Consulting the Oracle of Randomness...",
  "Executing top-secret permutation protocols...",
  "Summoning prime factors from the mathematical aether...",
  "Deploying steganographic squirrels to hide your nuts of data...",
  "Initializing the quantum superposition of your keys...",
  "Applying post-quantum cryptographic voodoo...",
  "Encrypting your data with the tears of frustrated regulators...",
];

function LoadingSpinnerAlert({ ...rest }: AlertProps) {
  return <Alert icon={<CircularProgress size={22} />} {...rest} />;
}

export default function DaemonStatusAlert() {
  const contextStatus = useAppSelector((s) => s.rpc.status);

  const [initMessage] = useState(
    FUNNY_INIT_MESSAGES[Math.floor(Math.random() * FUNNY_INIT_MESSAGES.length)],
  );

  if (contextStatus == null) {
    return (
      <LoadingSpinnerAlert severity="warning">
        {initMessage}
      </LoadingSpinnerAlert>
    );
  }

  switch (contextStatus.type) {
    case "Initializing":
      switch (contextStatus.content) {
        case TauriContextInitializationProgress.OpeningBitcoinWallet:
          return (
            <LoadingSpinnerAlert severity="warning">
              Connecting to the Bitcoin network
            </LoadingSpinnerAlert>
          );
        case TauriContextInitializationProgress.OpeningMoneroWallet:
          return (
            <LoadingSpinnerAlert severity="warning">
              Connecting to the Monero network
            </LoadingSpinnerAlert>
          );
        case TauriContextInitializationProgress.OpeningDatabase:
          return (
            <LoadingSpinnerAlert severity="warning">
              Opening the local database
            </LoadingSpinnerAlert>
          );
      }
      break;
    case "Available":
      return <Alert severity="success">The daemon is running</Alert>;
    case "Failed":
      return (
        <Alert severity="error">The daemon has stopped unexpectedly</Alert>
      );
    default:
      return exhaustiveGuard(contextStatus);
  }
}
