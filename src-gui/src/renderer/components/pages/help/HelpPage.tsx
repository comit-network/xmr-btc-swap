import { Box, makeStyles } from "@material-ui/core";
import ContactInfoBox from "./ContactInfoBox";
import DonateInfoBox from "./DonateInfoBox";
import FeedbackInfoBox from "./FeedbackInfoBox";
import DaemonControlBox from "./DaemonControlBox";
import SettingsBox from "./SettingsBox";
import ExportDataBox from "./ExportDataBox";
import { useLocation } from "react-router-dom";
import { useEffect } from "react";
const useStyles = makeStyles((theme) => ({
  outer: {
    display: "flex",
    gap: theme.spacing(2),
    flexDirection: "column",
    paddingBottom: theme.spacing(2),
  },
}));

export default function HelpPage() {
  const classes = useStyles();
  const location = useLocation(); 

  useEffect(() => {
    if (location.hash) {
      const element = document.getElementById(location.hash.slice(1));
      element?.scrollIntoView({ behavior: "smooth" });
    }
  }, [location]);

  return (
    <Box className={classes.outer}>
      <FeedbackInfoBox />
      <SettingsBox />
      <ExportDataBox />
      <DaemonControlBox />
      <ContactInfoBox />
      <DonateInfoBox />
    </Box>
  );
}
