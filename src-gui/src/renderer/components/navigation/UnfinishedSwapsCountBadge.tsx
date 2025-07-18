import React from "react";
import { Badge } from "@mui/material";
import {
  useIsSwapRunning,
  useResumeableSwapsCountExcludingPunished,
} from "store/hooks";

export default function UnfinishedSwapsBadge({
  children,
}: {
  children: React.ReactNode;
}) {
  const isSwapRunning = useIsSwapRunning();
  const resumableSwapsCount = useResumeableSwapsCountExcludingPunished();

  const displayedResumableSwapsCount = isSwapRunning
    ? resumableSwapsCount - 1
    : resumableSwapsCount;

  if (displayedResumableSwapsCount > 0) {
    return (
      <Badge badgeContent={resumableSwapsCount} color="primary">
        {children}
      </Badge>
    );
  }
  return children;
}
