import { Box, Paper, Tab, Tabs, Typography } from "@mui/material";
import PlayArrowIcon from "@mui/icons-material/PlayArrow";
import { useState } from "react";
import BitcoinAddressTextField from "renderer/components/inputs/BitcoinAddressTextField";
import MoneroAddressTextField from "renderer/components/inputs/MoneroAddressTextField";
import PromiseInvokeButton from "renderer/components/PromiseInvokeButton";
import { buyXmr } from "renderer/rpc";
import { useSettings } from "store/hooks";

export default function InitPage() {
  const [redeemAddress, setRedeemAddress] = useState("");
  const [refundAddress, setRefundAddress] = useState("");
  const [useExternalRefundAddress, setUseExternalRefundAddress] =
    useState(false);
  const [useExternalRedeemAddress, setUseExternalRedeemAddress] =
    useState(false);

  const [redeemAddressValid, setRedeemAddressValid] = useState(false);
  const [refundAddressValid, setRefundAddressValid] = useState(false);

  const donationRatio = useSettings((s) => s.donateToDevelopment);

  async function init() {
    await buyXmr(
      useExternalRefundAddress ? refundAddress : null,
      useExternalRedeemAddress ? redeemAddress : null,
      donationRatio,
    );
  }

  return (
    <>
      <Box
        sx={{
          display: "flex",
          flexDirection: "column",
          gap: 1.5,
        }}
      >
        <Paper variant="outlined" style={{}}>
          <Tabs
            value={useExternalRedeemAddress ? 1 : 0}
            indicatorColor="primary"
            variant="fullWidth"
            onChange={(_, newValue) =>
              setUseExternalRedeemAddress(newValue === 1)
            }
          >
            <Tab label="Redeem to internal Monero wallet" value={0} />
            <Tab label="Redeem to external Monero address" value={1} />
          </Tabs>
          <Box style={{ padding: "16px" }}>
            {useExternalRedeemAddress ? (
              <MoneroAddressTextField
                label="External Monero redeem address"
                address={redeemAddress}
                onAddressChange={setRedeemAddress}
                onAddressValidityChange={setRedeemAddressValid}
                helperText="The monero will be sent to this address if the swap is successful."
                fullWidth
              />
            ) : (
              <Typography variant="caption">
                The Monero will be sent to the internal Monero wallet of the GUI. 
                You can then withdraw them from there or use them for another swap directly.
              </Typography>
            )}
          </Box>
        </Paper>

        <Paper variant="outlined" style={{}}>
          <Tabs
            value={useExternalRefundAddress ? 1 : 0}
            indicatorColor="primary"
            variant="fullWidth"
            onChange={(_, newValue) =>
              setUseExternalRefundAddress(newValue === 1)
            }
          >
            <Tab label="Refund to internal Bitcoin wallet" value={0} />
            <Tab label="Refund to external Bitcoin address" value={1} />
          </Tabs>
          <Box style={{ padding: "16px" }}>
            {useExternalRefundAddress ? (
              <BitcoinAddressTextField
                label="External Bitcoin refund address"
                address={refundAddress}
                onAddressChange={setRefundAddress}
                onAddressValidityChange={setRefundAddressValid}
                helperText="In case something goes wrong, the Bitcoin will be refunded to this address."
                fullWidth
              />
            ) : (
              <Typography variant="caption">
                In case something goes wrong, the Bitcoin will be refunded to
                the internal Bitcoin wallet of the GUI. You can then withdraw
                them from there or use them for another swap directly.
              </Typography>
            )}
          </Box>
        </Paper>
      </Box>
      <Box style={{ display: "flex", justifyContent: "center" }}>
        <PromiseInvokeButton
          disabled={
            (!refundAddressValid && useExternalRefundAddress) ||
            (!redeemAddressValid && useExternalRedeemAddress)
          }
          variant="contained"
          color="primary"
          size="large"
          sx={{ marginTop: 1 }}
          endIcon={<PlayArrowIcon />}
          onInvoke={init}
          displayErrorSnackbar
        >
          Continue
        </PromiseInvokeButton>
      </Box>
    </>
  );
}
