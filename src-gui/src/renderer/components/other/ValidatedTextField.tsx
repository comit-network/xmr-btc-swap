import { TextFieldProps, TextField } from "@material-ui/core";
import { useState, useEffect, useCallback } from "react";

interface ValidatedTextFieldProps extends Omit<TextFieldProps, "onChange" | "value"> {
  value: string | null;
  isValid: (value: string) => boolean;
  onValidatedChange: (value: string | null) => void;
  allowEmpty?: boolean;
  helperText?: string;
}

export default function ValidatedTextField({
  label,
  value = "",
  isValid,
  onValidatedChange,
  helperText = "Invalid input",
  variant = "standard",
  allowEmpty = false,
  ...props
}: ValidatedTextFieldProps) {
  const [inputValue, setInputValue] = useState(value || "");

  const handleChange = useCallback(
    (newValue: string) => {
      const trimmedValue = newValue.trim();
      setInputValue(trimmedValue);

      if (trimmedValue === "" && allowEmpty) {
        onValidatedChange(null);
      } else if (isValid(trimmedValue)) {
        onValidatedChange(trimmedValue);
      }
    },
    [allowEmpty, isValid, onValidatedChange]
  );

  useEffect(() => {
    setInputValue(value || "");
  }, [value]);

  const isError = allowEmpty && inputValue === "" ? false : !isValid(inputValue);

  return (
    <TextField
      label={label}
      value={inputValue}
      onChange={(e) => handleChange(e.target.value)}
      error={isError}
      helperText={isError ? helperText : ""}
      variant={variant}
      {...props}
    />
  );
}
