import { MenuItem, Select, Box } from "@material-ui/core";
import TruncatedText from "renderer/components/other/TruncatedText";
import { PiconeroAmount } from "../../other/Units";
import { parseDateString } from "utils/parseUtils";
import { useEffect } from "react";
import { useSwapInfosSortedByDate } from "store/hooks";

interface SwapSelectDropDownProps {
  selectedSwap: string | null;
  setSelectedSwap: (swapId: string | null) => void;
}

export default function SwapSelectDropDown({
  selectedSwap,
  setSelectedSwap,
}: SwapSelectDropDownProps) {
  const swaps = useSwapInfosSortedByDate();

  useEffect(() => {
    if (swaps.length > 0) {
      setSelectedSwap(swaps[0].swap_id);
    }
  }, []);

  return (
    <Select
      value={selectedSwap ?? ""}
      variant="outlined"
      onChange={(e) => setSelectedSwap(e.target.value as string || null)}
      style={{ width: "100%" }}
      displayEmpty
    >
      {swaps.map((swap) => (
          <MenuItem value={swap.swap_id} key={swap.swap_id}>
            <Box component="span" style={{ whiteSpace: 'pre' }}>
              Swap <TruncatedText>{swap.swap_id}</TruncatedText> from{' '}
              {new Date(parseDateString(swap.start_date)).toDateString()} (
              <PiconeroAmount amount={swap.xmr_amount} />)
            </Box>
          </MenuItem>
      ))}
      <MenuItem value="">Do not attach a swap</MenuItem>
    </Select>
  );
} 