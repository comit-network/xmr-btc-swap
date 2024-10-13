import { Badge } from "@material-ui/core";
import { useResumeableSwapsCountExcludingPunished } from "store/hooks";

export default function UnfinishedSwapsBadge({
  children,
}: {
  children: JSX.Element;
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
