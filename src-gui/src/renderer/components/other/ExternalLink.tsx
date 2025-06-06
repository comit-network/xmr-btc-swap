import Link from "@mui/material/Link";
import { open } from "@tauri-apps/plugin-shell";

export default function ExternalLink({
  children,
  href,
}: {
  children: React.ReactNode;
  href: string;
}) {
  return (
    <Link style={{ cursor: "pointer" }} onClick={() => open(href)}>
      {children}
    </Link>
  );
}
