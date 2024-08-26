import {
  Box,
  Card,
  CardContent,
  IconButton,
  makeStyles,
} from "@material-ui/core";
import ArrowForwardIosIcon from "@material-ui/icons/ArrowForwardIos";
import { useState } from "react";
import { useAppSelector } from "store/hooks";
import ProviderInfo from "./ProviderInfo";
import ProviderListDialog from "./ProviderListDialog";

const useStyles = makeStyles({
  inner: {
    textAlign: "left",
    width: "100%",
    height: "100%",
  },
  providerCard: {
    width: "100%",
  },
  providerCardContent: {
    display: "flex",
    alignItems: "center",
  },
});

export default function ProviderSelect() {
  const classes = useStyles();
  const [selectDialogOpen, setSelectDialogOpen] = useState(false);
  const selectedProvider = useAppSelector(
    (state) => state.providers.selectedProvider,
  );

  if (!selectedProvider) return <>No provider selected</>;

  function handleSelectDialogClose() {
    setSelectDialogOpen(false);
  }

  function handleSelectDialogOpen() {
    setSelectDialogOpen(true);
  }

  return (
    <Box>
      <ProviderListDialog
        open={selectDialogOpen}
        onClose={handleSelectDialogClose}
      />
      <Card variant="outlined" className={classes.providerCard}>
        <CardContent className={classes.providerCardContent}>
          <ProviderInfo provider={selectedProvider} />
          <IconButton onClick={handleSelectDialogOpen} size="small">
            <ArrowForwardIosIcon />
          </IconButton>
        </CardContent>
      </Card>
    </Box>
  );
}
