import { Box } from "@mui/material";
import QRCode from "react-qr-code";

export default function BitcoinQrCode({ address }: { address: string }) {
  return (
    <Box
      sx={{
        display: "flex",
        justifyContent: "center",
        alignItems: "center",
        width: "100%",
      }}
    >
      <Box
        sx={{
          backgroundColor: "white",
          padding: 1,
          borderRadius: 1,
          width: "100%",
          aspectRatio: "1 / 1",
        }}
      >
        <QRCode
          value={`bitcoin:${address}`}
          size={1}
          style={{
            display: "block",
            width: "100%",
            height: "min-content",
            aspectRatio: 1,
          }}
          /* eslint-disable-next-line @typescript-eslint/ban-ts-comment */
          /* @ts-ignore */
          viewBox="0 0 1 1"
        />
      </Box>
    </Box>
  );
}
