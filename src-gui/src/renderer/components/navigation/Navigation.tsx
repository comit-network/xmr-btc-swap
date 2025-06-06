import { Box, Drawer } from "@mui/material";
import NavigationFooter from "./NavigationFooter";
import NavigationHeader from "./NavigationHeader";

export const drawerWidth = "240px";

export default function Navigation() {
  return (
    <Drawer
      variant="permanent"
      sx={{
        width: drawerWidth,
        flexShrink: 0,
        "& .MuiDrawer-paper": {
          width: drawerWidth,
        },
      }}
    >
      <Box
        sx={{
          overflow: "auto",
          display: "flex",
          flexDirection: "column",
          justifyContent: "space-between",
          height: "100%",
        }}
      >
        <NavigationHeader />
        <NavigationFooter />
      </Box>
    </Drawer>
  );
}
