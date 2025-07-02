import CircularProgressWithSubtitle from "../components/CircularProgressWithSubtitle";
import { useActiveSwapInfo, useSwapInfosSortedByDate } from "store/hooks";
import { Box } from "@mui/material";

export default function EncryptedSignatureSentPage() {
  return (
    <CircularProgressWithSubtitle description="Waiting for them to redeem the Bitcoin" />
  );
}
