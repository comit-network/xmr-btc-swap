import globals from "globals";
import pluginJs from "@eslint/js";
import tseslint from "typescript-eslint";
import pluginReact from "eslint-plugin-react";

export default [
  {
    ignores: ["node_modules", "dist"],
  },
  pluginJs.configs.recommended,
  ...tseslint.configs.recommended,
  pluginReact.configs.flat.recommended,
  {
    files: ["**/*.{js,mjs,cjs,ts,jsx,tsx}"],
    languageOptions: { globals: globals.browser },
    rules: {
      "react/react-in-jsx-scope": "off",
      // Disallow the use of the `open` on the gloal object
      "no-restricted-globals": [
        "warn",
        {
          name: "open",
          message:
            "Use the open(...) function from @tauri-apps/plugin-shell instead",
        },
      ],
      // Disallow the use of the `open` on the `window` object
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
