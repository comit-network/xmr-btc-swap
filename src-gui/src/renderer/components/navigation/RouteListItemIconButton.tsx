import { ListItem, ListItemIcon, ListItemText } from "@material-ui/core";
import { ReactNode } from "react";
import { useNavigate } from "react-router-dom";

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
    <ListItem button onClick={() => navigate(route)} key={name}>
      <ListItemIcon>{children}</ListItemIcon>
      <ListItemText primary={name} />
    </ListItem>
  );
}
