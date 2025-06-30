import React from "react";
import { Badge } from "@mui/material";
import { useResumeableSwapsCountExcludingPunished } from "store/hooks";

export default function UnfinishedSwapsBadge({
  children,
}: {
  children: React.ReactNode;
}) {
  const resumableSwapsCount = useResumeableSwapsCountExcludingPunished();

  if (resumableSwapsCount > 0) {
    return (
      <Badge badgeContent={resumableSwapsCount} color="primary">
        {children}
      </Badge>
    );
  }
  return children;
}
