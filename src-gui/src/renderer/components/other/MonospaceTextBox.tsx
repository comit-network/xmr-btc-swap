import { Box, Typography, makeStyles } from "@material-ui/core";
import { ReactNode } from "react";

type Props = {
  content: string;
  onClick?: (content: string) => void;
  endIcon?: ReactNode;
};

const useStyles = makeStyles((theme) => ({
  root: {
    display: "flex",
    alignItems: "center",
    backgroundColor: theme.palette.grey[900],
    borderRadius: theme.shape.borderRadius,
    padding: theme.spacing(1),
    gap: theme.spacing(1),
  },
  content: {
    wordBreak: "break-word",
    whiteSpace: "pre-wrap",
    fontFamily: "monospace",
    lineHeight: "1.5em",
  },
}));

export default function MonospaceTextBox({ content, endIcon, onClick }: Props) {
  const classes = useStyles();

  const handleClick = () => onClick?.(content);

  return (
    <Box className={classes.root} onClick={handleClick}>
      <Typography
        component="span"
        variant="overline"
        className={classes.content}
      >
        {content}
      </Typography>
      {endIcon}
    </Box>
  );
}
