import { createTheme } from "@material-ui/core";
import { indigo } from "@material-ui/core/colors";

export enum Theme {
  Light = "light",
  Dark = "dark",
  Darker = "darker"
}

const darkTheme = createTheme({
  palette: {
    type: "dark",
    primary: {
      main: "#f4511e", // Monero orange
    },
    secondary: indigo,
  },
  typography: {
    overline: {
      textTransform: "none", // This prevents the text from being all caps
      fontFamily: "monospace"
    },
  },
});

const lightTheme = createTheme({
  ...darkTheme,
  palette: {
    type: "light",
    primary: {
      main: "#f4511e", // Monero orange
    },
    secondary: indigo,
  },
});

const darkerTheme = createTheme({
  ...darkTheme,
  palette: {
    type: 'dark',
    primary: {
      main: "#f4511e",
    },
    secondary: indigo,
    background: {
      default: "#080808",
      paper: "#181818",
    },
  },
});

export const themes = {
  [Theme.Dark]: darkTheme,
  [Theme.Light]: lightTheme,
  [Theme.Darker]: darkerTheme,
};
