import Link from "@material-ui/core/Link";
import { open } from "@tauri-apps/plugin-shell";

export default function ExternalLink({children, href}: {children: React.ReactNode, href: string}) {
    return (
        <Link style={{cursor: 'pointer'}} onClick={() => open(href)}>
            {children}
        </Link>
    )
}