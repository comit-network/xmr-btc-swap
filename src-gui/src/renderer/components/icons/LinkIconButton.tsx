import { IconButton } from "@mui/material";
import { open } from "@tauri-apps/plugin-shell";
import { ReactNode } from "react";

export default function LinkIconButton({
  url,
  children,
}: {
  url: string;
  children: ReactNode;
}) {
  return (
    <IconButton component="span" onClick={() => open(url)} size="large">
      {children}
    </IconButton>
  );
}
