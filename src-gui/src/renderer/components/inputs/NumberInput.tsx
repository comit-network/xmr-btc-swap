import { useEffect, useRef, useState } from "react";
import { darken, useTheme } from "@mui/material";

interface NumberInputProps {
  value: string;
  onChange: (value: string) => void;
  placeholder?: string;
  fontSize?: string;
  fontWeight?: number;
  textAlign?: "left" | "center" | "right";
  minWidth?: number;
  step?: number;
  largeStep?: number;
  className?: string;
  style?: React.CSSProperties;
}

export default function NumberInput({
  value,
  onChange,
  placeholder = "0.00",
  fontSize = "2em",
  fontWeight = 600,
  textAlign = "right",
  minWidth = 60,
  step = 0.001,
  largeStep = 0.1,
  className,
  style,
}: NumberInputProps) {
  const inputRef = useRef<HTMLInputElement>(null);
  const [inputWidth, setInputWidth] = useState(minWidth);
  const [isFocused, setIsFocused] = useState(false);
  const measureRef = useRef<HTMLSpanElement>(null);

  // Calculate precision from step value
  const getDecimalPrecision = (num: number): number => {
    const str = num.toString();
    if (str.includes(".")) {
      return str.split(".")[1].length;
    }
    return 0;
  };

  const [userPrecision, setUserPrecision] = useState(() =>
    getDecimalPrecision(step),
  ); // Track user's decimal precision
  const [minPrecision, setMinPrecision] = useState(3);
  const appliedPrecision =
    userPrecision > minPrecision ? userPrecision : minPrecision;

  const theme = useTheme();

  // Initialize with placeholder if no value provided
  useEffect(() => {
    if (
      (!value || value.trim() === "" || parseFloat(value) === 0) &&
      !isFocused
    ) {
      onChange(placeholder);
    }
  }, [placeholder, isFocused, value, onChange]);

  // Update precision when step changes
  useEffect(() => {
    setUserPrecision(getDecimalPrecision(step));
    setMinPrecision(getDecimalPrecision(step));
  }, [step]);

  // Measure text width to size input dynamically
  useEffect(() => {
    if (measureRef.current) {
      const text = value;
      measureRef.current.textContent = text;
      const textWidth = measureRef.current.offsetWidth;
      setInputWidth(Math.max(textWidth + 5, minWidth)); // Add padding and minimum width
    }
  }, [value, placeholder, minWidth]);

  const handleKeyDown = (e: React.KeyboardEvent) => {
    if (e.key === "ArrowUp" || e.key === "ArrowDown") {
      e.preventDefault();
      const currentValue = parseFloat(value) || 0;
      const increment = e.shiftKey ? largeStep : step;
      const newValue =
        e.key === "ArrowUp"
          ? currentValue + increment
          : Math.max(0, currentValue - increment);
      // Use the user's precision for formatting
      onChange(newValue.toFixed(appliedPrecision));
    }
  };

  const handleChange = (e: React.ChangeEvent<HTMLInputElement>) => {
    const inputValue = e.target.value;
    // Allow empty string, numbers, and decimal points
    if (inputValue === "" || /^\d*\.?\d*$/.test(inputValue)) {
      onChange(inputValue);

      // Track the user's decimal precision
      if (inputValue.includes(".")) {
        const decimalPart = inputValue.split(".")[1];
        setUserPrecision(decimalPart ? decimalPart.length : 0);
      } else if (inputValue && !isNaN(parseFloat(inputValue))) {
        // No decimal point, so precision is 0
        setUserPrecision(0);
      }
    }
  };

  const handleBlur = () => {
    setIsFocused(false);
    // Reset to placeholder if value is zero or empty
    if (!value || value.trim() === "" || parseFloat(value) === 0) {
      onChange(placeholder);
    } else if (!isNaN(parseFloat(value))) {
      // Format valid numbers on blur using user's precision
      const formatted = parseFloat(value).toFixed(appliedPrecision);
      // Remove trailing zeros if precision allows
      onChange(parseFloat(formatted).toString());
    }
  };

  const handleFocus = () => {
    setIsFocused(true);
  };

  // Determine if we should show placeholder styling
  const isShowingPlaceholder = value === placeholder && !isFocused;

  const defaultStyle: React.CSSProperties = {
    fontSize,
    fontWeight,
    textAlign,
    border: "none",
    outline: "none",
    background: "transparent",
    width: `${inputWidth}px`,
    minWidth: `${minWidth}px`,
    fontFamily: "inherit",
    color: isShowingPlaceholder
      ? darken(theme.palette.text.primary, 0.5)
      : theme.palette.text.primary,
    padding: "4px 0",
    transition: "color 0.2s ease",
    ...style,
  };

  return (
    <div style={{ position: "relative", display: "inline-block" }}>
      {/* Hidden span for measuring text width */}
      <span
        ref={measureRef}
        style={{
          position: "absolute",
          visibility: "hidden",
          fontSize,
          fontWeight,
          fontFamily: "inherit",
          whiteSpace: "nowrap",
        }}
      />

      <input
        ref={inputRef}
        type="text"
        inputMode="decimal"
        value={value}
        onChange={handleChange}
        onKeyDown={handleKeyDown}
        onBlur={handleBlur}
        onFocus={handleFocus}
        className={className}
        style={defaultStyle}
      />
    </div>
  );
}
