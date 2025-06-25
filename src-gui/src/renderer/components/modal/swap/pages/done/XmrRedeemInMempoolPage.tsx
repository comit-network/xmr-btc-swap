import { Box, DialogContentText, Typography } from "@mui/material";
import { TauriSwapProgressEventContent } from "models/tauriModelExt";
import FeedbackInfoBox from "../../../../pages/help/FeedbackInfoBox";
import MoneroTransactionInfoBox from "../../MoneroTransactionInfoBox";

export default function XmrRedeemInMempoolPage(
  state: TauriSwapProgressEventContent<"XmrRedeemInMempool">,
) {
  const xmr_redeem_txid = state.xmr_redeem_txids[0] ?? null;

  return (
    <Box>
      <DialogContentText>
        The swap was successful and the Monero has been sent to the following
        address(es). The swap is completed and you may exit the application now.
      </DialogContentText>
      <Box
        style={{
          display: "flex",
          flexDirection: "column",
          gap: "0.5rem",
        }}
      >
        <MoneroTransactionInfoBox
          title="Monero Redeem Transaction"
          txId={xmr_redeem_txid}
          additionalContent={
            <Box sx={{ display: "flex", flexDirection: "column", gap: "1rem" }}>
              <Box sx={{ display: "flex", flexDirection: "column", gap: 1 }}>
                {state.xmr_receive_pool.map((pool, index) => (
                  <Box
                    key={index}
                    sx={{
                      display: "flex",
                      flexDirection: "column",
                      gap: 0.5,
                      padding: 1,
                      border: 1,
                      borderColor: "divider",
                      borderRadius: 1,
                      backgroundColor: (theme) => theme.palette.action.hover,
                    }}
                  >
                    <Box
                      sx={{
                        display: "flex",
                        justifyContent: "space-between",
                        alignItems: "center",
                      }}
                    >
                      <Typography
                        variant="body2"
                        sx={(theme) => ({
                          fontWeight: 600,
                          color: theme.palette.text.primary,
                        })}
                      >
                        {pool.label} ({pool.percentage * 100}%)
                      </Typography>
                    </Box>
                    <Typography
                      variant="caption"
                      sx={{
                        fontFamily: "monospace",
                        color: (theme) => theme.palette.text.secondary,
                        wordBreak: "break-all",
                      }}
                    >
                      {pool.address}
                    </Typography>
                  </Box>
                ))}
              </Box>
            </Box>
          }
          loading={false}
        />
        <FeedbackInfoBox />
      </Box>
    </Box>
  );
}
