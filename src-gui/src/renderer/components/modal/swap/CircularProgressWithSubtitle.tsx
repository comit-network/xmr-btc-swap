import {
  Box,
  CircularProgress,
  makeStyles,
  Typography,
} from "@material-ui/core";
import { ReactNode } from "react";

const useStyles = makeStyles((theme) => ({
  subtitle: {
    paddingTop: theme.spacing(1),
  },
}));

export default function CircularProgressWithSubtitle({
  description,
}: {
  description: string | ReactNode;
}) {
  const classes = useStyles();

  return (
    <Box
      display="flex"
      justifyContent="center"
      alignItems="center"
      flexDirection="column"
    >
      <CircularProgress size={50} />
      <Typography variant="subtitle2" className={classes.subtitle}>
        {description}
      </Typography>
    </Box>
  );
}
