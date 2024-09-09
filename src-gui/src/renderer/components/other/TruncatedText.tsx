export default function TruncatedText({
  children,
  limit = 6,
  ellipsis = "...",
}: {
  children: string;
  limit?: number;
  ellipsis?: string;
}) {
  const truncatedText =
    children.length > limit ? children.slice(0, limit) + ellipsis : children;

  return truncatedText;
}
