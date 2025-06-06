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
  const truncatedText =
    children.length > limit
      ? truncateMiddle
        ? children.slice(0, Math.floor(limit / 2)) +
          ellipsis +
          children.slice(children.length - Math.floor(limit / 2))
        : children.slice(0, limit) + ellipsis
      : children;

  return <span>{truncatedText}</span>;
}
