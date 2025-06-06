import globals from "globals";
import js from "@eslint/js";
import tseslint from "typescript-eslint";
import pluginReact from "eslint-plugin-react";

export default [
  { ignores: ["node_modules", "dist"] },
  js.configs.recommended,
  ...tseslint.configs.recommended,
  pluginReact.configs.flat.recommended,
  {
    languageOptions: {
      globals: globals.browser,
    },
    rules: {
      "react/react-in-jsx-scope": "off",
      "react/no-unescaped-entities": "off",
      "react/no-children-prop": "off",
      "@typescript-eslint/no-unused-vars": "off",
      "@typescript-eslint/no-empty-object-type": "off",
      "no-restricted-globals": [
        "warn",
        {
          name: "open",
          message:
            "Use the open(...) function from @tauri-apps/plugin-shell instead",
        },
      ],
      "no-restricted-properties": [
        "warn",
        {
          object: "window",
          property: "open",
          message:
            "Use the open(...) function from @tauri-apps/plugin-shell instead",
        },
      ],
    },
  },
];
