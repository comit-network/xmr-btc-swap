import { ReactNode } from 'react';
import { useNavigate } from 'react-router-dom';
import { ListItem, ListItemIcon, ListItemText } from '@material-ui/core';

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
