import { useEffect, useState } from 'react';
import {
  Dialog,
  DialogTitle,
  DialogContent,
  DialogContentText,
  DialogActions,
  Button,
  LinearProgress,
  Typography,
  makeStyles,
  LinearProgressProps,
  Box,
} from '@material-ui/core';
import SystemUpdateIcon from '@material-ui/icons/SystemUpdate';
import { check, Update, DownloadEvent } from '@tauri-apps/plugin-updater';
import { useSnackbar } from 'notistack';
import { relaunch } from '@tauri-apps/plugin-process';

const useStyles = makeStyles((theme) => ({
  progress: {
    marginTop: theme.spacing(2)
  },
  releaseNotes: {
    marginTop: theme.spacing(2),
    marginBottom: theme.spacing(1)
  },
  noteContent: {
    whiteSpace: 'pre-line'
  }
}));

interface DownloadProgress {
  contentLength: number | null;
  downloadedBytes: number;
}

function LinearProgressWithLabel(props: LinearProgressProps & { label?: string }) {
  return (
    <Box display="flex" alignItems="center">
      <Box width="100%" mr={1}>
        <LinearProgress variant="determinate" {...props} />
      </Box>
      <Box minWidth={85}>
        <Typography variant="body2" color="textSecondary">
          {props.label || `${Math.round(props.value)}%`}
        </Typography>
      </Box>
    </Box>
  );
}

export default function UpdaterDialog() {
  const classes = useStyles();
  const [availableUpdate, setAvailableUpdate] = useState<Update | null>(null);
  const [downloadProgress, setDownloadProgress] = useState<DownloadProgress | null>(null);
  const {enqueueSnackbar} = useSnackbar();

  useEffect(() => {
    // Check for updates when component mounts
    check().then((updateResponse) => {
        setAvailableUpdate(updateResponse);
    }).catch((err) => {
        enqueueSnackbar(`Failed to check for updates: ${err}`, {
            variant: 'error',
        });
    });
  }, []);

  // If no update is available, don't render the dialog
  if (!availableUpdate?.available) return null;

  function hideNotification() {
    setAvailableUpdate(null);
  };

  async function handleInstall() {
    try {
      await availableUpdate.downloadAndInstall((event: DownloadEvent) => {
        if (event.event === 'Started') {
          setDownloadProgress({
            contentLength: event.data.contentLength || null,
            downloadedBytes: 0,
          });
        } else if (event.event === 'Progress') {
          setDownloadProgress(prev => ({
            ...prev,
            downloadedBytes: prev.downloadedBytes + event.data.chunkLength,
          }));
        } else if (event.event === 'Finished') {
            // Relaunch the application for the new version to be used
          relaunch();
        }
      });
    } catch (err) {
        enqueueSnackbar(`Failed to install update: ${err}`, {
            variant: "error"
        });
    }
  };

  const isDownloading = downloadProgress !== null;

  const progress = isDownloading
    ? Math.round((downloadProgress.downloadedBytes / downloadProgress.contentLength) * 100)
    : 0;

  return (
    <Dialog
      fullWidth
      maxWidth="sm"
      open={availableUpdate.available}
      onClose={hideNotification}
    >
      <DialogTitle>Update Available</DialogTitle>
      <DialogContent>
        <DialogContentText>
          A new version (v{availableUpdate.version}) is available. Your current version is {availableUpdate.currentVersion}.
          The update will be verified using PGP signature verification to ensure authenticity.
          {availableUpdate.body && (
            <>
              <Typography variant="h6" className={classes.releaseNotes}>
                Release Notes:
              </Typography>
              <Typography variant="body2" component="div" className={classes.noteContent}>
                {availableUpdate.body}
              </Typography>
            </>
          )}
        </DialogContentText>
        
        {isDownloading && (
          <Box className={classes.progress}>
            <LinearProgressWithLabel 
              value={progress}
              label={`${(downloadProgress.downloadedBytes / 1024 / 1024).toFixed(1)} MB${
                downloadProgress.contentLength 
                  ? ` / ${(downloadProgress.contentLength / 1024 / 1024).toFixed(1)} MB` 
                  : ''
              }`}
            />
          </Box>
        )}
      </DialogContent>
      <DialogActions>
        <Button
          variant="text"
          color="default"
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
          {isDownloading ? 'DOWNLOADING...' : 'INSTALL UPDATE'}
        </Button>
      </DialogActions>
    </Dialog>
  );
}
