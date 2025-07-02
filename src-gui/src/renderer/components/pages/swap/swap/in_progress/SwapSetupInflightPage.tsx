import { useState, useEffect } from "react";
import { resolveApproval } from "renderer/rpc";
import {
  PendingLockBitcoinApprovalRequest,
  TauriSwapProgressEventContent,
} from "models/tauriModelExt";
import { SatsAmount, PiconeroAmount } from "renderer/components/other/Units";
import { Box, Typography, Divider, Theme } from "@mui/material";
import { useActiveSwapId, usePendingLockBitcoinApproval } from "store/hooks";
import PromiseInvokeButton from "renderer/components/PromiseInvokeButton";
import CircularProgressWithSubtitle from "../components/CircularProgressWithSubtitle";
import CheckIcon from "@mui/icons-material/Check";
import ArrowRightAltIcon from "@mui/icons-material/ArrowRightAlt";
import TruncatedText from "renderer/components/other/TruncatedText";

/// A hook that returns the LockBitcoin confirmation request for the active swap
/// Returns null if no confirmation request is found
function useActiveLockBitcoinApprovalRequest(): PendingLockBitcoinApprovalRequest | null {
  const approvals = usePendingLockBitcoinApproval();
  const activeSwapId = useActiveSwapId();

  return (
    approvals?.find((r) => r.request.content.swap_id === activeSwapId) || null
  );
}

export default function SwapSetupInflightPage({
  btc_lock_amount,
}: TauriSwapProgressEventContent<"SwapSetupInflight">) {
  const request = useActiveLockBitcoinApprovalRequest();

  const [timeLeft, setTimeLeft] = useState<number>(0);
  const expirationTs =
    request?.request_status.state === "Pending"
      ? request.request_status.content.expiration_ts
      : null;

  useEffect(() => {
    if (expirationTs == null) {
      return;
    }

    const expiresAtMs = expirationTs * 1000 || 0;

    const tick = () => {
      const remainingMs = Math.max(expiresAtMs - Date.now(), 0);
      setTimeLeft(Math.ceil(remainingMs / 1000));
    };

    tick();
    const id = setInterval(tick, 250);
    return () => clearInterval(id);
  }, [request, expirationTs]);

  // If we do not have an approval request yet for the Bitcoin lock transaction, we haven't received the offer from Alice yet
  // Display a loading spinner to the user for as long as the swap_setup request is in flight
  if (request == null) {
    return (
      <Box
        sx={{
          height: 200,
          display: "flex",
          alignItems: "center",
          justifyContent: "center",
        }}
      >
        <CircularProgressWithSubtitle
          description={
            <>
              Negotiating offer for <SatsAmount amount={btc_lock_amount} />
            </>
          }
        />
      </Box>
    );
  }

  const { btc_network_fee, monero_receive_pool, xmr_receive_amount } =
    request.request.content;

  return (
    <Box
      sx={{
        display: "flex",
        flexDirection: "column",
        alignItems: "space-between",
        justifyContent: "space-between",
        height: "100%",
        flex: 1,
      }}
    >
      {/* Grid layout for perfect alignment */}
      <Box
        sx={{
          display: "flex",
          flexDirection: { xs: "column", lg: "row" },
          gap: "1.5rem",
          alignItems: "stretch",
          justifyContent: "space-between",
        }}
      >
        {/* Row 1: Bitcoin box */}
        <Box sx={{ height: "100%", flex: "0 0 auto" }}>
          <BitcoinMainBox
            btc_lock_amount={btc_lock_amount}
            btc_network_fee={btc_network_fee}
          />
        </Box>

        {/* Row 1: Animated arrow */}
        <Box
          sx={{
            display: "flex",
            alignItems: "center",
            justifyContent: "center",
          }}
        >
          <AnimatedArrow />
        </Box>

        {/* Row 1: Monero main box */}
        <Box sx={{ flex: "0 0 auto" }}>
          <MoneroMainBox
            monero_receive_pool={monero_receive_pool}
            xmr_receive_amount={xmr_receive_amount}
          />
        </Box>
      </Box>

      <Box
        sx={{
          marginTop: 4,
          display: "flex",
          flexDirection: "column",
          alignItems: "center",
          gap: 1.5,
        }}
      >
        <Box sx={{ display: "flex", justifyContent: "center", gap: 2 }}>
          <PromiseInvokeButton
            variant="text"
            size="large"
            sx={(theme) => ({ color: theme.palette.text.secondary })}
            onInvoke={() =>
              resolveApproval(request.request_id, false as unknown as object)
            }
            displayErrorSnackbar
            requiresContext
          >
            Deny
          </PromiseInvokeButton>

          <PromiseInvokeButton
            variant="contained"
            color="primary"
            size="large"
            onInvoke={() =>
              resolveApproval(request.request_id, true as unknown as object)
            }
            displayErrorSnackbar
            requiresContext
            endIcon={<CheckIcon />}
          >
            {`Confirm`}
          </PromiseInvokeButton>
        </Box>
        <Typography
          variant="caption"
          sx={{
            textAlign: "center",
            color: (theme) => theme.palette.text.secondary,
          }}
        >
          {`Offer expires in ${timeLeft}s`}
        </Typography>
      </Box>
    </Box>
  );
}

/**
 * Pure presentational components -------------------------------------------------
 * They live in the same file to avoid additional imports yet keep
 * JSX for the main page tidy. All styling values are kept identical
 * to their previous inline counterparts so that the visual appearance
 * stays exactly the same while making the code easier to reason about.
 */

interface BitcoinSendSectionProps {
  btc_lock_amount: number;
  btc_network_fee: number;
}

const BitcoinMainBox = ({
  btc_lock_amount,
  btc_network_fee,
}: {
  btc_lock_amount: number;
  btc_network_fee: number;
}) => (
  <Box
    sx={{
      position: "relative",
      height: "100%",
      display: "flex",
      flexDirection: "column",
      gap: 1,
    }}
  >
    <Box
      sx={{
        display: "flex",
        justifyContent: "space-between",
        alignItems: "center",
        padding: 1.5,
        border: 1,
        gap: "0.5rem 1rem",
        borderColor: "warning.main",
        borderRadius: 1,
        flexGrow: 1,
        backgroundColor: (theme) => theme.palette.warning.light + "10",
        background: (theme) =>
          `linear-gradient(135deg, ${theme.palette.warning.light}20, ${theme.palette.warning.light}05)`,
      }}
    >
      <Typography
        variant="body1"
        sx={(theme) => ({
          color: theme.palette.text.primary,
        })}
      >
        You send
      </Typography>
      <Typography
        variant="h5"
        sx={(theme) => ({
          fontWeight: "bold",
          color: theme.palette.warning.dark,
          textShadow: "0 1px 2px rgba(0,0,0,0.1)",
        })}
      >
        <SatsAmount amount={btc_lock_amount} />
      </Typography>
    </Box>

    {/* Network fee box attached to the bottom */}
    <Box
      sx={{
        padding: "0.25rem 0.75rem",
        backgroundColor: (theme) => theme.palette.warning.main,
        color: (theme) => theme.palette.warning.contrastText,
        borderRadius: "4px",
        fontSize: "0.75rem",
        fontWeight: 600,
        boxShadow: "0 2px 4px rgba(0,0,0,0.1)",
        whiteSpace: "nowrap",
        zIndex: 1,
      }}
    >
      Network fee: <SatsAmount amount={btc_network_fee} />
    </Box>
  </Box>
);

interface PoolBreakdownProps {
  monero_receive_pool: Array<{
    address: string;
    label: string;
    percentage: number;
  }>;
  xmr_receive_amount: number;
}

const PoolBreakdown = ({
  monero_receive_pool,
  xmr_receive_amount,
}: PoolBreakdownProps) => {
  // Find the pool entry with the highest percentage to exclude it (since it's shown in main box)
  const highestPercentagePool = monero_receive_pool.reduce((prev, current) =>
    prev.percentage > current.percentage ? prev : current,
  );

  // Filter out the highest percentage pool since it's already displayed in the main box
  const remainingPools = monero_receive_pool.filter(
    (pool) => pool !== highestPercentagePool,
  );

  return (
    <Box
      sx={{ display: "flex", flexDirection: "column", gap: 1, width: "100%" }}
    >
      {remainingPools.map((pool) => (
        <Box
          key={pool.address}
          sx={{
            display: "flex",
            justifyContent: "flex-start",
            alignItems: "stretch",
            padding: pool.percentage >= 0.05 ? 1.5 : "0.25rem 0.75rem",
            border: 1,
            borderColor:
              pool.percentage >= 0.05 ? "success.main" : "success.light",
            borderRadius: 1,
            backgroundColor: (theme) =>
              pool.percentage >= 0.05
                ? theme.palette.success.light + "10"
                : theme.palette.action.hover,
            width: "100%", // Ensure full width
            minWidth: 0,
            opacity: pool.percentage >= 0.05 ? 1 : 0.75,
            animation:
              pool.percentage >= 0.05
                ? "poolPulse 2s ease-in-out infinite"
                : "none",
            "@keyframes poolPulse": {
              "0%": {
                transform: "scale(1)",
                opacity: 1,
              },
              "50%": {
                transform: "scale(1.02)",
                opacity: 0.95,
              },
              "100%": {
                transform: "scale(1)",
                opacity: 1,
              },
            },
          }}
        >
          <Box
            sx={{
              display: "flex",
              flexDirection: "column",
              justifyContent: "center",
              gap: 0.5,
              flex: "1 1 0",
              minWidth: 0,
            }}
          >
            <Typography
              variant="body2"
              sx={(theme) => ({
                color: theme.palette.text.primary,
                fontSize: "0.75rem",
                fontWeight: 600,
              })}
            >
              {pool.label === "user address" ? "Your Wallet" : pool.label}
            </Typography>
            {pool.label === "user address" && (
              <Typography
                variant="body2"
                sx={{
                  fontFamily: "monospace",
                  fontSize: "0.75rem",
                  color: (theme) => theme.palette.text.secondary,
                }}
              >
                <TruncatedText truncateMiddle limit={15}>
                  {pool.address}
                </TruncatedText>
              </Typography>
            )}
          </Box>
          <Box
            sx={{
              display: "flex",
              flexDirection: "column",
              alignItems: "flex-end",
              gap: 0.5,
              flex: "0 0 auto",
              minWidth: 140,
              justifyContent: "center",
            }}
          >
            {pool.percentage >= 0.05 && (
              <Typography
                variant="body2"
                sx={(theme) => ({
                  fontWeight: "bold",
                  color: theme.palette.success.main,
                  fontSize: "0.875rem",
                  whiteSpace: "nowrap",
                })}
              >
                <PiconeroAmount
                  amount={pool.percentage * Number(xmr_receive_amount)}
                />
              </Typography>
            )}
            <Typography
              variant="caption"
              sx={(theme) => ({
                color: theme.palette.text.secondary,
                whiteSpace: "nowrap",
              })}
            >
              {pool.percentage * 100}%
            </Typography>
          </Box>
        </Box>
      ))}
    </Box>
  );
};

interface MoneroReceiveSectionProps {
  monero_receive_pool: PoolBreakdownProps["monero_receive_pool"];
  xmr_receive_amount: number;
}

const MoneroMainBox = ({
  monero_receive_pool,
  xmr_receive_amount,
}: MoneroReceiveSectionProps) => {
  // Find the pool entry with the highest percentage
  const highestPercentagePool = monero_receive_pool.reduce((prev, current) =>
    prev.percentage > current.percentage ? prev : current,
  );

  return (
    <Box sx={{ display: "flex", flexDirection: "column", gap: 1 }}>
      <Box
        sx={{
          display: "flex",
          justifyContent: "space-between",
          alignItems: "center",
          padding: 1.5,
          border: 1,
          gap: "0.5rem 1rem",
          borderColor: "success.main",
          borderRadius: 1,
          backgroundColor: (theme) => theme.palette.success.light + "10",
          background: (theme) =>
            `linear-gradient(135deg, ${theme.palette.success.light}20, ${theme.palette.success.light}05)`,
          flex: "1 1 0",
        }}
      >
        <Box sx={{ display: "flex", flexDirection: "column", gap: 0.25 }}>
          <Typography
            variant="body1"
            sx={(theme) => ({
              color: theme.palette.text.primary,
              fontWeight: 700,
              letterSpacing: 0.5,
            })}
          >
            {highestPercentagePool.label}
          </Typography>
          <Typography
            variant="caption"
            sx={{
              fontFamily: "monospace",
              fontSize: "0.65rem",
              color: (theme) => theme.palette.text.secondary,
            }}
          >
            <TruncatedText truncateMiddle limit={15}>
              {highestPercentagePool.address}
            </TruncatedText>
          </Typography>
        </Box>
        <Box
          sx={{
            display: "flex",
            flexDirection: "column",
            alignItems: "flex-end",
            justifyContent: "center",
          }}
        >
          <Typography
            variant="h5"
            sx={(theme) => ({
              fontWeight: "bold",
              color: theme.palette.success.dark,
              textShadow: "0 1px 2px rgba(0,0,0,0.1)",
            })}
          >
            <PiconeroAmount
              amount={
                highestPercentagePool.percentage * Number(xmr_receive_amount)
              }
            />
          </Typography>
        </Box>
      </Box>

      {/* Secondary Monero content attached to the bottom */}
      <MoneroSecondaryContent
        monero_receive_pool={monero_receive_pool}
        xmr_receive_amount={xmr_receive_amount}
      />
    </Box>
  );
};

const MoneroSecondaryContent = ({
  monero_receive_pool,
  xmr_receive_amount,
}: MoneroReceiveSectionProps) => (
  <PoolBreakdown
    monero_receive_pool={monero_receive_pool}
    xmr_receive_amount={xmr_receive_amount}
  />
);

// Arrow animation styling extracted for reuse
const arrowSx = {
  fontSize: "3rem",
  color: (theme: Theme) => theme.palette.primary.main,
  animation: "slideArrow 2s infinite",
  "@keyframes slideArrow": {
    "0%": {
      opacity: 0.6,
      transform: "translateX(-8px)",
    },
    "50%": {
      opacity: 1,
      transform: "translateX(8px)",
    },
    "100%": {
      opacity: 0.6,
      transform: "translateX(-8px)",
    },
  },
} as const;

const AnimatedArrow = () => (
  <Box
    sx={{
      display: "flex",
      alignItems: "flex-start",
      justifyContent: "center",
      alignSelf: "center",
      flex: "0 0 auto",
      transform: { xs: "rotate(90deg)", lg: "rotate(0deg)" },
    }}
  >
    <ArrowRightAltIcon sx={arrowSx} />
  </Box>
);
