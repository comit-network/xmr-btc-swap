import { Box, Button, IconButton, Tooltip } from "@mui/material";
import { FileCopyOutlined, QrCode as QrCodeIcon } from "@mui/icons-material";
import { writeText } from "@tauri-apps/plugin-clipboard-manager";
import { useState } from "react";
import MonospaceTextBox from "./MonospaceTextBox";
import { Modal } from "@mui/material";
import QRCode from "react-qr-code";

type ModalProps = {
  open: boolean;
  onClose: () => void;
  content: string;
};

type Props = {
  content: string;
  displayCopyIcon?: boolean;
  enableQrCode?: boolean;
};

function QRCodeModal({ open, onClose, content }: ModalProps) {
  return (
    <Modal open={open} onClose={onClose}>
      <Box
        sx={{
          display: "flex",
          flexDirection: "column",
          gap: 2,
          justifyContent: "center",
          alignItems: "center",
          height: "100%",
        }}
      >
        <QRCode
          value={content}
          size={500}
          style={{
            maxWidth: "90%",
            maxHeight: "90%",
          }}
          viewBox="0 0 500 500"
        />
        <Button
          onClick={onClose}
          size="large"
          variant="contained"
          color="primary"
        >
          Done
        </Button>
      </Box>
    </Modal>
  );
}

export default function ActionableMonospaceTextBox({
  content,
  displayCopyIcon = true,
  enableQrCode = true,
}: Props) {
  const [copied, setCopied] = useState(false);
  const [qrCodeOpen, setQrCodeOpen] = useState(false);
  const [isQrCodeButtonHovered, setIsQrCodeButtonHovered] = useState(false);

  const handleCopy = async () => {
    await writeText(content);
    setCopied(true);
    setTimeout(() => setCopied(false), 2000);
  };

  return (
    <>
      <Tooltip
        title={
          isQrCodeButtonHovered
            ? ""
            : copied
              ? "Copied to clipboard"
              : "Click to copy"
        }
        arrow
      >
        <Box
          sx={{
            display: "flex",
            alignItems: "center",
            cursor: "pointer",
          }}
        >
          <Box sx={{ flexGrow: 1 }} onClick={handleCopy}>
            <MonospaceTextBox>
              {content}
              {displayCopyIcon && (
                <IconButton
                  onClick={handleCopy}
                  size="small"
                  sx={{ marginLeft: 1 }}
                >
                  <FileCopyOutlined />
                </IconButton>
              )}
              {enableQrCode && (
                <Tooltip title="Show QR Code" arrow>
                  <IconButton
                    onClick={() => setQrCodeOpen(true)}
                    onMouseEnter={() => setIsQrCodeButtonHovered(true)}
                    onMouseLeave={() => setIsQrCodeButtonHovered(false)}
                    size="small"
                    sx={{ marginLeft: 1 }}
                  >
                    <QrCodeIcon />
                  </IconButton>
                </Tooltip>
              )}
            </MonospaceTextBox>
          </Box>
        </Box>
      </Tooltip>
      {enableQrCode && (
        <QRCodeModal
          open={qrCodeOpen}
          onClose={() => setQrCodeOpen(false)}
          content={content}
        />
      )}
    </>
  );
}
