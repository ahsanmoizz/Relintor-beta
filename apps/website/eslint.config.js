import eslint from "@eslint/js";
import tseslint from "typescript-eslint";

export default [
  eslint.configs.recommended,
  ...tseslint.configs.recommended,
  { ignores: ["dist"] },
  { files: ["**/*.{ts,tsx}"], rules: { "@typescript-eslint/no-unused-vars": "off" } },
];
