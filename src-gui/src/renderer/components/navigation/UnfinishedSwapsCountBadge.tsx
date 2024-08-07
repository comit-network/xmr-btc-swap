import { Badge } from '@material-ui/core';
import { useResumeableSwapsCount } from 'store/hooks';

export default function UnfinishedSwapsBadge({
  children,
}: {
  children: JSX.Element;
}) {
  const resumableSwapsCount = useResumeableSwapsCount();

  if (resumableSwapsCount > 0) {
    return (
      <Badge badgeContent={resumableSwapsCount} color="primary">
        {children}
      </Badge>
    );
  }
  return children;
}
