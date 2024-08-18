import { Box, Drawer, makeStyles } from "@material-ui/core";
import NavigationFooter from "./NavigationFooter";
import NavigationHeader from "./NavigationHeader";

export const drawerWidth = 240;

const useStyles = makeStyles({
  drawer: {
    width: drawerWidth,
    flexShrink: 0,
  },
  drawerPaper: {
    width: drawerWidth,
  },
  drawerContainer: {
    overflow: "auto",
    display: "flex",
    flexDirection: "column",
    justifyContent: "space-between",
    height: "100%",
  },
});

export default function Navigation() {
  const classes = useStyles();

  return (
    <Drawer
      className={classes.drawer}
      variant="permanent"
      classes={{
        paper: classes.drawerPaper,
      }}
    >
      <Box className={classes.drawerContainer}>
        <NavigationHeader />
        <NavigationFooter />
      </Box>
    </Drawer>
  );
}
