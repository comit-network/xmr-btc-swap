import {
  useTheme,
  Tooltip,
  Typography,
  Box,
  LinearProgress,
  Paper,
} from "@mui/material";
import { ExpiredTimelocks } from "models/tauriModel";
import { GetSwapInfoResponseExt, getAbsoluteBlock } from "models/tauriModelExt";
import HumanizedBitcoinBlockDuration from "renderer/components/other/HumanizedBitcoinBlockDuration";

interface TimelineSegment {
  title: string;
  label: string;
  bgcolor: string;
  startBlock: number;
}

interface TimelineSegmentProps {
  segment: TimelineSegment;
  isActive: boolean;
  absoluteBlock: number;
  durationOfSegment: number | null;
  totalBlocks: number;
}

function TimelineSegment({
  segment,
  isActive,
  absoluteBlock,
  durationOfSegment,
  totalBlocks,
}: TimelineSegmentProps) {
  const theme = useTheme();

  return (
    <Tooltip title={<Typography variant="caption">{segment.title}</Typography>}>
      <Box
        sx={{
          display: "flex",
          flexDirection: "column",
          alignItems: "center",
          justifyContent: "center",
          bgcolor: segment.bgcolor,
          width: `${durationOfSegment ? (durationOfSegment / totalBlocks) * 85 : 15}%`,
          position: "relative",
        }}
        style={{
          opacity: isActive ? 1 : 0.3,
        }}
      >
        {isActive && (
          <Box
            sx={{
              position: "absolute",
              top: 0,
              left: 0,
              height: "100%",
              width: `${Math.max(5, ((absoluteBlock - segment.startBlock) / durationOfSegment) * 100)}%`,
              zIndex: 1,
            }}
          >
            <LinearProgress
              variant="indeterminate"
              color="primary"
              style={{
                height: "100%",
                backgroundColor: theme.palette.primary.dark,
                opacity: 0.3,
              }}
            />
          </Box>
        )}
        <Typography
          variant="subtitle2"
          color="inherit"
          align="center"
          style={{ zIndex: 2 }}
        >
          {segment.label}
        </Typography>
        {durationOfSegment && (
          <Typography
            variant="caption"
            color="inherit"
            align="center"
            style={{
              zIndex: 2,
              opacity: 0.8,
            }}
          >
            {isActive && (
              <>
                <HumanizedBitcoinBlockDuration
                  blocks={
                    durationOfSegment - (absoluteBlock - segment.startBlock)
                  }
                />{" "}
                left
              </>
            )}
            {!isActive && (
              <HumanizedBitcoinBlockDuration blocks={durationOfSegment} />
            )}
          </Typography>
        )}
      </Box>
    </Tooltip>
  );
}

export function TimelockTimeline({
  swap,
}: {
  // This forces the timelock to not be null
  swap: GetSwapInfoResponseExt & { timelock: ExpiredTimelocks };
}) {
  const theme = useTheme();

  const timelineSegments: TimelineSegment[] = [
    {
      title: "Normally a swap is completed during this period",
      label: "Normal",
      bgcolor: theme.palette.success.main,
      startBlock: 0,
    },
    {
      title:
        "If the swap hasn't been completed before we reach this period, the Bitcoin needs to be refunded. For that, you need to have the app open sometime within the refund period",
      label: "Refund",
      bgcolor: theme.palette.warning.main,
      startBlock: swap.cancel_timelock,
    },
    {
      title:
        "If you didn't refund within the refund window, you will enter this period. At this point, the Bitcoin can no longer be refunded. It may still be possible to redeem the Monero with cooperation from the other party but this cannot be guaranteed.",
      label: "Danger",
      bgcolor: theme.palette.error.main,
      startBlock: swap.cancel_timelock + swap.punish_timelock,
    },
  ];

  const totalBlocks = swap.cancel_timelock + swap.punish_timelock;
  const absoluteBlock = getAbsoluteBlock(
    swap.timelock,
    swap.cancel_timelock,
    swap.punish_timelock,
  );

  // This calculates the duration of a segment
  // by getting the the difference to the next segment
  function durationOfSegment(index: number): number | null {
    const nextSegment = timelineSegments[index + 1];
    if (nextSegment == null) {
      return null;
    }
    return nextSegment.startBlock - timelineSegments[index].startBlock;
  }

  // This function returns the index of the active segment based on the current block
  // We iterate in reverse to find the first segment that has a start block less than the current block
  function getActiveSegmentIndex() {
    return (
      Array.from(
        timelineSegments
          .slice()
          // We use .entries() to keep the indexes despite reversing
          .entries(),
      )
        .reverse()
        .find(([_, segment]) => absoluteBlock >= segment.startBlock)?.[0] ?? 0
    );
  }

  return (
    <Box
      sx={{
        width: "100%",
        minWidth: "100%",
        flexGrow: 1,
      }}
    >
      <Paper
        style={{
          position: "relative",
          height: "5rem",
          overflow: "hidden",
        }}
        elevation={3}
        variant="outlined"
      >
        <Box
          sx={{
            position: "relative",
            height: "100%",
            display: "flex",
          }}
        >
          {timelineSegments.map((segment, index) => (
            <TimelineSegment
              key={index}
              segment={segment}
              isActive={getActiveSegmentIndex() === index}
              absoluteBlock={absoluteBlock}
              durationOfSegment={durationOfSegment(index)}
              totalBlocks={totalBlocks}
            />
          ))}
        </Box>
      </Paper>
    </Box>
  );
}
