import Button, { ButtonProps } from "@mui/material/Button";

export default function ClipboardIconButton({
  text,
  ...props
}: { text: string } & ButtonProps) {
  function writeToClipboard() {
    throw new Error("Not implemented");
  }

  return (
    <Button onClick={writeToClipboard} {...props}>
      Copy
    </Button>
  );
}
