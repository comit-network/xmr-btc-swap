import {
  Accordion,
  AccordionDetails,
  AccordionSummary,
  Button,
  Dialog,
  DialogActions,
  DialogContent,
  TextField,
  Typography,
  Radio,
} from "@mui/material";

import { DialogTitle } from "@mui/material";
import { useState, useEffect } from "react";
import { getRestoreHeight, setMoneroRestoreHeight } from "renderer/rpc";
import { DatePicker } from "@mui/x-date-pickers/DatePicker";
import { Dayjs } from "dayjs";
import PromiseInvokeButton from "renderer/components/PromiseInvokeButton";

enum RestoreOption {
  BlockHeight = "blockHeight",
  RestoreDate = "restoreDate",
}

export default function SetRestoreHeightModal({
  open,
  onClose,
}: {
  open: boolean;
  onClose: () => void;
}) {
  const [restoreOption, setRestoreOption] = useState(RestoreOption.BlockHeight);
  const [restoreHeight, setRestoreHeight] = useState<number | "">("");
  const [restoreDate, setRestoreDate] = useState<Dayjs | null>(null);
  const [isPending, setIsPending] = useState(false);
  const [currentRestoreHeight, setCurrentRestoreHeight] =
    useState<string>("Loading...");

  const handleRestoreHeight = async () => {
    if (restoreOption === RestoreOption.BlockHeight) {
      if (typeof restoreHeight === "number") {
        await setMoneroRestoreHeight(restoreHeight);
      }
    } else if (restoreOption === RestoreOption.RestoreDate) {
      if (restoreDate) {
        await setMoneroRestoreHeight(restoreDate.toDate());
      }
    }
  };

  useEffect(() => {
    const fetchCurrentRestoreHeight = async () => {
      try {
        const response = await getRestoreHeight();
        setCurrentRestoreHeight(response.height.toString());
        setRestoreHeight(response.height); // Set the input field to current height
      } catch (error) {
        console.error("Failed to fetch restore height:", error);
        setCurrentRestoreHeight("Error");
      }
    };

    if (open) {
      fetchCurrentRestoreHeight();
    }
  }, [open, isPending]);

  const accordionStyle = {
    "& .MuiAccordionSummary-content": {
      display: "flex",
      alignItems: "center",
      justifyContent: "space-between",
      gap: 1,
    },
    "&::before": {
      opacity: "1 !important",
    },
  };

  return (
    <Dialog open={open} onClose={onClose}>
      <DialogTitle>Set Restore Height</DialogTitle>
      <DialogContent sx={{ minWidth: "500px", minHeight: "300px" }}>
        <Accordion
          elevation={0}
          expanded={restoreOption === RestoreOption.BlockHeight}
          onChange={() => setRestoreOption(RestoreOption.BlockHeight)}
          disableGutters
          sx={accordionStyle}
        >
          <AccordionSummary>
            <Typography>Restore by block height</Typography>
            <Radio checked={restoreOption === RestoreOption.BlockHeight} />
          </AccordionSummary>
          <AccordionDetails>
            <TextField
              label="Restore Height"
              type="number"
              value={restoreHeight}
              onChange={(e) => {
                const value = e.target.value;
                setRestoreHeight(value === "" ? "" : Number(value));
              }}
            />
          </AccordionDetails>
        </Accordion>
        <Accordion
          elevation={0}
          expanded={restoreOption === RestoreOption.RestoreDate}
          onChange={() => setRestoreOption(RestoreOption.RestoreDate)}
          disableGutters
          sx={accordionStyle}
        >
          <AccordionSummary>
            <Typography>Restore by date</Typography>
            <Radio checked={restoreOption === RestoreOption.RestoreDate} />
          </AccordionSummary>
          <AccordionDetails>
            <DatePicker
              label="Restore Date"
              value={restoreDate}
              disableFuture
              onChange={(date) => setRestoreDate(date)}
            />
          </AccordionDetails>
        </Accordion>
      </DialogContent>
      <DialogActions>
        <Button onClick={onClose}>Cancel</Button>
        <PromiseInvokeButton
          onInvoke={handleRestoreHeight}
          onSuccess={onClose}
          displayErrorSnackbar={true}
          onPendingChange={setIsPending}
        >
          Confirm
        </PromiseInvokeButton>
      </DialogActions>
    </Dialog>
  );
}
