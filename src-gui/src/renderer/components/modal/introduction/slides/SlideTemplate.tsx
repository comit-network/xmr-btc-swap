import { Paper, Box, Typography, Button } from "@mui/material";

type slideTemplateProps = {
  handleContinue: () => void;
  handlePrevious: () => void;
  hidePreviousButton?: boolean;
  stepLabel?: string;
  title: string;
  children?: React.ReactNode;
  imagePath?: string;
  imagePadded?: boolean;
  customContinueButtonText?: string;
};

export default function SlideTemplate({
  handleContinue,
  handlePrevious,
  hidePreviousButton,
  stepLabel,
  title,
  children,
  imagePath,
  imagePadded,
  customContinueButtonText,
}: slideTemplateProps) {
  return (
    <Paper
      sx={{
        height: "80%",
        width: "80%",
        display: "flex",
        justifyContent: "space-between",
      }}
    >
      <Box
        sx={{
          m: 3,
          alignContent: "center",
          position: "relative",
          width: "50%",
          flexGrow: 1,
        }}
      >
        <Box>
          {stepLabel && (
            <Typography variant="overline" sx={{ textTransform: "uppercase" }}>
              {stepLabel}
            </Typography>
          )}
          <Typography variant="h4" style={{ marginBottom: 16 }}>
            {title}
          </Typography>
          {children}
        </Box>
        <Box
          sx={{
            position: "absolute",
            bottom: 0,
            width: "100%",
            display: "flex",
            justifyContent: hidePreviousButton ? "flex-end" : "space-between",
          }}
        >
          {!hidePreviousButton && (
            <Button onClick={handlePrevious}>Back</Button>
          )}
          <Button onClick={handleContinue} variant="contained" color="primary">
            {customContinueButtonText ? customContinueButtonText : "Next"}
          </Button>
        </Box>
      </Box>
      {imagePath && (
        <Box
          sx={{
            bgcolor: "#212121",
            width: "50%",
            display: "flex",
            justifyContent: "center",
            p: imagePadded ? "1.5em" : 0,
          }}
        >
          <img
            src={imagePath}
            style={{
              height: "100%",
              width: "100%",
              objectFit: "contain",
            }}
          />
        </Box>
      )}
    </Paper>
  );
}
