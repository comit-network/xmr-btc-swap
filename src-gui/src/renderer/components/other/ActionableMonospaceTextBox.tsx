import { Box, Button, IconButton, Tooltip, makeStyles } from "@material-ui/core";
import { FileCopyOutlined, CropFree as CropFreeIcon } from "@material-ui/icons";
import { writeText } from "@tauri-apps/plugin-clipboard-manager";
import { useState } from "react";
import MonospaceTextBox from "./MonospaceTextBox";
import { Modal } from "@material-ui/core";
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

const useStyles = makeStyles((theme) => ({
  container: {
    display: "flex",
    alignItems: "center",
    cursor: "pointer",
  },
  textBoxWrapper: {
    flexGrow: 1,
  },
  iconButton: {
    marginLeft: theme.spacing(1),
  },
  modalContent: {
    display: "flex",
    flexDirection: "column",
    gap: theme.spacing(2),
    justifyContent: "center",
    alignItems: "center",
    height: "100%",
  },
  qrCode: {
    maxWidth: "90%",
    maxHeight: "90%",
  },
}));

function QRCodeModal({ open, onClose, content }: ModalProps) {
  const classes = useStyles();
  return (
    <Modal open={open} onClose={onClose}>
      <Box className={classes.modalContent}>
        <QRCode
          value={content}
          size={500}
          className={classes.qrCode}
          viewBox="0 0 500 500"
        />
        <Button onClick={onClose} size="large" variant="contained" color="primary">
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
  const classes = useStyles();
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
      <Tooltip title={isQrCodeButtonHovered ? "" : (copied ? "Copied to clipboard" : "Click to copy")} arrow>
        <Box className={classes.container}>
          <Box className={classes.textBoxWrapper} onClick={handleCopy}>
            <MonospaceTextBox>
              {content}
              {displayCopyIcon && (
                <IconButton onClick={handleCopy} size="small" className={classes.iconButton}>
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
                    className={classes.iconButton}
                  >
                    <CropFreeIcon />
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