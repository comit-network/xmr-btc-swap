import { useEffect, useState } from "react";
import {
  Dialog,
  DialogTitle,
  DialogContent,
  DialogContentText,
  DialogActions,
  Button,
  LinearProgress,
  Typography,
  LinearProgressProps,
  Box,
  Link,
} from "@mui/material";
import SystemUpdateIcon from "@mui/icons-material/SystemUpdate";
import { check, Update, DownloadEvent } from "@tauri-apps/plugin-updater";
import { useSnackbar } from "notistack";
import { relaunch } from "@tauri-apps/plugin-process";

const GITHUB_RELEASES_URL = "https://github.com/UnstoppableSwap/core/releases";
const HOMEPAGE_URL = "https://unstoppableswap.net/";

interface DownloadProgress {
  contentLength: number | null;
  downloadedBytes: number;
}

function LinearProgressWithLabel(
  props: LinearProgressProps & { label?: string },
) {
  return (
    <Box
      sx={{
        display: "flex",
        alignItems: "center",
      }}
    >
      <Box
        sx={{
          width: "100%",
          mr: 1,
        }}
      >
        <LinearProgress variant="determinate" {...props} />
      </Box>
      <Box
        sx={{
          minWidth: 85,
        }}
      >
        <Typography variant="body2" color="textSecondary">
          {props.label || `${Math.round(props.value)}%`}
        </Typography>
      </Box>
    </Box>
  );
}

export default function UpdaterDialog() {
  const [availableUpdate, setAvailableUpdate] = useState<Update | null>(null);
  const [downloadProgress, setDownloadProgress] =
    useState<DownloadProgress | null>(null);
  const { enqueueSnackbar } = useSnackbar();

  useEffect(() => {
    // Check for updates when component mounts
    check()
      .then((updateResponse) => {
        console.log("updateResponse", updateResponse);
        setAvailableUpdate(updateResponse);
      })
      .catch((err) => {
        enqueueSnackbar(`Failed to check for updates: ${err}`, {
          variant: "error",
        });
      });
  }, []);

  // If no update is available, don't render the dialog
  if (availableUpdate === null) return null;

  function hideNotification() {
    setAvailableUpdate(null);
  }

  async function handleInstall() {
    try {
      await availableUpdate.downloadAndInstall((event: DownloadEvent) => {
        if (event.event === "Started") {
          setDownloadProgress({
            contentLength: event.data.contentLength || null,
            downloadedBytes: 0,
          });
        } else if (event.event === "Progress") {
          setDownloadProgress((prev) => ({
            ...prev,
            downloadedBytes: prev.downloadedBytes + event.data.chunkLength,
          }));
        }
      });

      // Once the promise resolves, relaunch the application for the new version to be used
      relaunch();
    } catch (err) {
      enqueueSnackbar(`Failed to install update: ${err}`, {
        variant: "error",
      });
    }
  }

  const isDownloading = downloadProgress !== null;

  const progress = isDownloading
    ? Math.round(
        (downloadProgress.downloadedBytes / downloadProgress.contentLength) *
          100,
      )
    : 0;

  return (
    <Dialog
      fullWidth
      maxWidth="sm"
      open={availableUpdate?.available}
      onClose={hideNotification}
    >
      <DialogTitle>Update Available</DialogTitle>
      <DialogContent>
        <DialogContentText>
          A new version (v{availableUpdate.version}) is available. Your current
          version is {availableUpdate.currentVersion}. The update will be
          verified using PGP signature verification to ensure authenticity.
          Alternatively, you can download the update from{" "}
          <Link href={GITHUB_RELEASES_URL} target="_blank">
            GitHub
          </Link>{" "}
          or visit the{" "}
          <Link href={HOMEPAGE_URL} target="_blank">
            download page
          </Link>
          .
          {availableUpdate.body && (
            <>
              <Typography variant="h6" sx={{ mt: 2, mb: 1 }}>
                Release Notes:
              </Typography>
              <Typography
                variant="body2"
                component="div"
                sx={{ whiteSpace: "pre-line" }}
              >
                {availableUpdate.body}
              </Typography>
            </>
          )}
        </DialogContentText>

        {isDownloading && (
          <Box sx={{ mt: 2 }}>
            <LinearProgressWithLabel
              value={progress}
              label={`${(downloadProgress.downloadedBytes / 1024 / 1024).toFixed(1)} MB${
                downloadProgress.contentLength
                  ? ` / ${(downloadProgress.contentLength / 1024 / 1024).toFixed(1)} MB`
                  : ""
              }`}
            />
          </Box>
        )}
      </DialogContent>
      <DialogActions>
        <Button
          variant="text"
          onClick={hideNotification}
          disabled={isDownloading}
        >
          Remind me later
        </Button>
        <Button
          endIcon={<SystemUpdateIcon />}
          variant="contained"
          color="primary"
          onClick={handleInstall}
          disabled={isDownloading}
        >
          {isDownloading ? "DOWNLOADING..." : "INSTALL UPDATE"}
        </Button>
      </DialogActions>
    </Dialog>
  );
}
