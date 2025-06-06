import { Paper, Card, CardContent, IconButton } from "@mui/material";
import ArrowForwardIosIcon from "@mui/icons-material/ArrowForwardIos";
import { useState } from "react";
import { useAppSelector } from "store/hooks";
import MakerInfo from "./MakerInfo";
import MakerListDialog from "./MakerListDialog";

export default function MakerSelect() {
  const [selectDialogOpen, setSelectDialogOpen] = useState(false);
  const selectedMaker = useAppSelector((state) => state.makers.selectedMaker);

  if (!selectedMaker) return <>No maker selected</>;

  function handleSelectDialogClose() {
    setSelectDialogOpen(false);
  }

  function handleSelectDialogOpen() {
    setSelectDialogOpen(true);
  }

  return (
    <Paper variant="outlined" elevation={4}>
      <MakerListDialog
        open={selectDialogOpen}
        onClose={handleSelectDialogClose}
      />
      <Card sx={{ width: "100%" }}>
        <CardContent sx={{ display: "flex", alignItems: "center" }}>
          <MakerInfo maker={selectedMaker} />
          <IconButton onClick={handleSelectDialogOpen} size="small">
            <ArrowForwardIosIcon />
          </IconButton>
        </CardContent>
      </Card>
    </Paper>
  );
}
