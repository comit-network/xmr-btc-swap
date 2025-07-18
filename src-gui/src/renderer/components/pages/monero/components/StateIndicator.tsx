import { Box, darken, lighten, useTheme } from "@mui/material";

function getColor(colorName: string) {
  const theme = useTheme();
  switch (colorName) {
    case "primary":
      return theme.palette.primary.main;
    case "secondary":
      return theme.palette.secondary.main;
    case "success":
      return theme.palette.success.main;
    case "warning":
      return theme.palette.warning.main;
  }
}

export default function StateIndicator({
  color,
  pulsating,
}: {
  color: string;
  pulsating: boolean;
}) {
  const mainShade = getColor(color);
  const darkShade = darken(mainShade, 0.4);
  const glowShade = lighten(mainShade, 0.4);

  const intensePulsatingStyles = {
    animation: "pulse 2s infinite",
    "@keyframes pulse": {
      "0%": { opacity: 0.5 },
      "50%": { opacity: 1 },
      "100%": { opacity: 0.5 },
    },
  };

  const softPulsatingStyles = {
    animation: "pulse 3.5s infinite",
    "@keyframes pulse": {
      "0%": { opacity: 0.7 },
      "50%": { opacity: 1 },
      "100%": { opacity: 0.7 },
    },
  };

  return (
    <Box
      sx={{
        width: 10,
        height: 10,
        borderRadius: "50%",
        backgroundImage: `radial-gradient(circle, ${mainShade}, ${darkShade})`,
        boxShadow: `0 0 10px 0 ${glowShade}`,
        ...(pulsating ? intensePulsatingStyles : softPulsatingStyles),
      }}
    ></Box>
  );
}
