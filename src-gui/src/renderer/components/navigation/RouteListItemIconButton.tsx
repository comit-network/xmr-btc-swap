import { ListItemIcon, ListItemText } from "@mui/material";
import { ReactNode } from "react";
import { useNavigate, useLocation } from "react-router-dom";

import ListItemButton from "@mui/material/ListItemButton";

export default function RouteListItemIconButton({
  name,
  route,
  children,
}: {
  name: string;
  route: string[] | string;
  children: ReactNode;
}) {
  const navigate = useNavigate();
  const location = useLocation();

  const routeArray = Array.isArray(route) ? route : [route];
  const firstRoute = routeArray[0];
  const isSelected = routeArray.some((r) => location.pathname === r);

  return (
    <ListItemButton
      onClick={() => navigate(firstRoute)}
      key={name}
      sx={
        isSelected
          ? {
              backgroundColor: "action.hover",
              "&:hover": {
                backgroundColor: "action.selected",
              },
            }
          : undefined
      }
    >
      <ListItemIcon>{children}</ListItemIcon>
      <ListItemText primary={name} />
    </ListItemButton>
  );
}
