import humanizeDuration from "humanize-duration";

const AVG_BLOCK_TIME_MS = 10 * 60 * 1000;

export default function HumanizedBitcoinBlockDuration({
  blocks,
}: {
  blocks: number;
}) {
  return (
    <>
      {`${humanizeDuration(blocks * AVG_BLOCK_TIME_MS, {
        conjunction: " and ",
      })} (${blocks} blocks)`}
    </>
  );
}
