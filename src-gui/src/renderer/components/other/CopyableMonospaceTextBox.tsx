import { Box, Tooltip } from "@material-ui/core";
import { FileCopyOutlined } from "@material-ui/icons";
import { writeText } from "@tauri-apps/plugin-clipboard-manager";
import { useState } from "react";
import MonospaceTextBox from "./MonospaceTextBox";

type Props = {
  address: string;
  noIcon?: boolean;
};

/** Display addresses monospaced and clickable such that a click copies the address to the clipboard. */
export default function CopyableMonospaceTextBox({
  address,
  noIcon = false,
}: Props) {
  // Signal that the address was copied
  const [copied, setCopied] = useState(false);
  const tooltip = copied ? "Copied to clipboard" : "Click to copy";

  // Copy address to clipboard on-click
  const handleClick = async () => {
    // Copy to clipboard
    await writeText(address);
    // Change tooltip to show that we copied the address
    setCopied(true);
    // After a delay, show default tooltip again (2sec)
    setTimeout(() => setCopied(false), 2_000);
  };

  // Apply icon unless specified otherwise
  const icon = noIcon ? null : <FileCopyOutlined />;

  return (
    <Tooltip title={tooltip} arrow>
      {/* Div is necessary to make the tooltip work */}
      <Box style={{ cursor: "pointer" }}>
        <MonospaceTextBox
          content={address}
          endIcon={icon}
          onClick={handleClick}
        />
      </Box>
    </Tooltip>
  );
}
