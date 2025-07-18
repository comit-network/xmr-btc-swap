export default function TruncatedText({
  children,
  limit = 6,
  ellipsis = "...",
  truncateMiddle = false,
}: {
  children: string;
  limit?: number;
  ellipsis?: string;
  truncateMiddle?: boolean;
}) {
  let finalChildren = children ?? "";

  const truncatedText =
    finalChildren.length > limit
      ? truncateMiddle
        ? finalChildren.slice(0, Math.floor(limit / 2)) +
          ellipsis +
          finalChildren.slice(finalChildren.length - Math.floor(limit / 2))
        : finalChildren.slice(0, limit) + ellipsis
      : finalChildren;

  return <span>{truncatedText}</span>;
}
