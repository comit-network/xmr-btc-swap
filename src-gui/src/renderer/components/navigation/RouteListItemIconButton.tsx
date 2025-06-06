import { ListItemIcon, ListItemText } from "@mui/material";
import { ReactNode } from "react";
import { useNavigate } from "react-router-dom";

import ListItemButton from "@mui/material/ListItemButton";

export default function RouteListItemIconButton({
  name,
  route,
  children,
}: {
  name: string;
  route: string;
  children: ReactNode;
}) {
  const navigate = useNavigate();

  return (
    <ListItemButton onClick={() => navigate(route)} key={name}>
      <ListItemIcon>{children}</ListItemIcon>
      <ListItemText primary={name} />
    </ListItemButton>
  );
}
