import { IconButton } from "@material-ui/core";
import { ReactNode } from "react";

export default function LinkIconButton({
  url,
  children,
}: {
  url: string;
  children: ReactNode;
}) {
  return (
    <IconButton component="span" onClick={() => window.open(url, "_blank")}>
      {children}
    </IconButton>
  );
}
