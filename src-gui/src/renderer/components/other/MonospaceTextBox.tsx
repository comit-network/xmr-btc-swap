import { Box, Typography, makeStyles } from "@material-ui/core";

type Props = {
  children: React.ReactNode;
};

const useStyles = makeStyles((theme) => ({
  root: {
    display: "flex",
    alignItems: "center",
    backgroundColor: theme.palette.grey[900],
    borderRadius: theme.shape.borderRadius,
    padding: theme.spacing(1),
  },
  content: {
    wordBreak: "break-word",
    whiteSpace: "pre-wrap",
    fontFamily: "monospace",
    lineHeight: 1.5,
    display: "flex",
    alignItems: "center",
  },
}));

export default function MonospaceTextBox({ children }: Props) {
  const classes = useStyles();

  return (
    <Box className={classes.root}>
      <Typography component="span" variant="overline" className={classes.content}>
        {children}
      </Typography>
    </Box>
  );
}