import js from "@eslint/js";
import simpleImportSort from "eslint-plugin-simple-import-sort";
import pluginVue from "eslint-plugin-vue";
import vitest from "@vitest/eslint-plugin";
import tseslint from "typescript-eslint";
import { defineConfig } from "eslint/config";

// Severity policy (docs/DEVELOPMENT.md § Quality pipeline): rules with an
// existing backlog are staged at `warn` (tracked, non-blocking — CI does not
// pass --max-warnings); burn the backlog down, then promote to `error` and
// record the promotion. Never blanket-disable to get green.

// Shared non-type-aware TS guardrails for src/tests .ts files and SFC
// <script lang="ts"> blocks. Type-aware linting is deliberately not wired:
// `vue-tsc --noEmit` (npm run build) is the type gate.
const tsGuardrails = {
  "@typescript-eslint/consistent-type-imports": [
    "error",
    { prefer: "type-imports", fixStyle: "separate-type-imports" },
  ],
  "@typescript-eslint/no-unused-vars": [
    "error",
    { args: "none", ignoreRestSiblings: true },
  ],
  // src/ is at zero explicit `any` (docs/Gaps.md §10) — promoted from day one
  // to block regressions. Tests keep their own override below.
  "@typescript-eslint/no-explicit-any": "error",
  "simple-import-sort/imports": "error",
  "simple-import-sort/exports": "error",
};

// Src-only safety gate, shared between src/**/*.ts and src/**/*.vue.
const srcSafetyRules = {
  // Frontend diagnostics must funnel through src/logging.ts so they land in
  // vault-buddy.log (AGENTS.md § Diagnostics invariants). Staged at `warn`:
  // one offender, main.ts's last-resort Vue errorHandler console.error.
  "no-console": "warn",
  "no-new-func": "error",
  // Raw HTML injection is the XSS vector for a webview rendering strings
  // derived from vault contents (search results, note titles — see
  // docs/Gaps.md GAP-34). Zero offenders today; goes straight to `error`.
  "no-restricted-syntax": [
    "error",
    {
      selector:
        'AssignmentExpression > MemberExpression[property.name="innerHTML"]',
      message:
        "Assigning to innerHTML is banned (XSS risk — vault-derived strings). Render through Vue templates; HighlightText shows the index-based pattern.",
    },
    {
      selector:
        'AssignmentExpression > MemberExpression[property.name="outerHTML"]',
      message:
        "Assigning to outerHTML is banned (XSS risk — vault-derived strings). Render through Vue templates.",
    },
    {
      selector: 'CallExpression[callee.property.name="insertAdjacentHTML"]',
      message:
        "insertAdjacentHTML is banned (XSS risk — vault-derived strings). Render through Vue templates.",
    },
  ],
};

export default defineConfig([
  {
    ignores: [
      "dist/**",
      "node_modules/**",
      "coverage/**",
      ".fallow/**",
      "src-tauri/target/**",
      // Vendored superpowers skills framework — third-party code, not ours
      // to lint (see docs/DEVELOPMENT.md § Superpowers skills).
      ".claude/**",
    ],
  },
  js.configs.recommended,
  {
    files: ["scripts/**/*.mjs", "*.mjs"],
    languageOptions: {
      globals: {
        console: "readonly",
        process: "readonly",
        Buffer: "readonly",
        URL: "readonly",
        setTimeout: "readonly",
        clearTimeout: "readonly",
      },
    },
  },
  ...tseslint.configs.recommended,
  {
    // tsc (vue-tsc) owns undefined-identifier checking for TypeScript; core
    // no-undef false-positives on browser/DOM globals there. Mirrors
    // typescript-eslint's own guidance. The .vue block below does the same.
    files: ["**/*.ts"],
    rules: { "no-undef": "off" },
  },
  {
    // Vue SFC lint. flat/recommended = base + essential (errors) +
    // strongly-recommended + recommended (warnings — the tracked backlog
    // tier). Scoped via extends so the vue/* rules never resolve against
    // plain .ts files.
    files: ["**/*.vue"],
    extends: [pluginVue.configs["flat/recommended"]],
    languageOptions: {
      parserOptions: {
        // vue-eslint-parser stays the outer parser (set by the configs
        // above); the TS parser handles <script lang="ts"> blocks.
        parser: tseslint.parser,
        extraFileExtensions: [".vue"],
        sourceType: "module",
      },
    },
    plugins: {
      "simple-import-sort": simpleImportSort,
    },
    rules: {
      // v-html sets innerHTML under the hood — same XSS reasoning as the
      // srcSafetyRules ban. Zero offenders; error from day one.
      "vue/no-v-html": "error",
      // The panel views are deliberately single-word (Search, Tasks,
      // Recordings — AGENTS.md § Frontend state names them); the rule exists
      // to avoid clashes with future HTML elements, which these roots and
      // views don't risk.
      "vue/multi-word-component-names": "off",
      // vue-tsc owns undefined-identifier checking for <script lang="ts">;
      // core no-undef false-positives on browser globals there.
      "no-undef": "off",
      ...tsGuardrails,
    },
  },
  {
    files: ["src/**/*.ts", "tests/**/*.ts"],
    plugins: {
      "simple-import-sort": simpleImportSort,
    },
    rules: tsGuardrails,
  },
  {
    files: ["src/**/*.ts"],
    rules: srcSafetyRules,
  },
  {
    files: ["src/**/*.vue"],
    rules: srcSafetyRules,
  },
  {
    // Function-health signal the file-level LOC guard (scripts/check-loc.mjs)
    // can't see. Adopted at `error` directly — the 2026-07-10 adoption run
    // found zero offenders, so there was no backlog to stage.
    files: ["src/**/*.ts", "src/**/*.vue"],
    rules: {
      "max-lines-per-function": [
        "error",
        { max: 200, skipBlankLines: true, skipComments: true, IIFEs: true },
      ],
      complexity: ["error", { max: 25 }],
      "max-params": ["error", { max: 6 }],
      "max-depth": ["error", { max: 5 }],
    },
  },
  {
    files: ["tests/**/*.ts"],
    plugins: { vitest },
    rules: {
      ...vitest.configs.recommended.rules,
      // Tests legitimately use `any` for mocking IPC/plugin shapes; the
      // zero-any guardrail targets production src/ only.
      "@typescript-eslint/no-explicit-any": "off",
      // vi.mock factories are hoisted above imports; require() inside them
      // is the sanctioned pattern (see tests using vi.hoisted).
      "@typescript-eslint/no-require-imports": "off",
      "vitest/no-disabled-tests": "error",
      "vitest/no-commented-out-tests": "error",
    },
  },
]);
