import { Button } from "@material-ui/core";
import { ButtonProps } from "@material-ui/core/Button/Button";

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
