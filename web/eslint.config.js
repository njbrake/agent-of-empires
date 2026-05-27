import js from '@eslint/js'
import globals from 'globals'
import reactHooks from 'eslint-plugin-react-hooks'
import reactRefresh from 'eslint-plugin-react-refresh'
import tseslint from 'typescript-eslint'
import { defineConfig, globalIgnores } from 'eslint/config'

export default defineConfig([
  globalIgnores(['dist']),
  {
    files: ['**/*.{ts,tsx}'],
    extends: [
      js.configs.recommended,
      tseslint.configs.recommended,
      reactHooks.configs.flat.recommended,
      reactRefresh.configs.vite,
    ],
    languageOptions: {
      ecmaVersion: 2020,
      globals: globals.browser,
    },
    rules: {
      // Playwright tests destructure an empty fixture bag (`({}, testInfo) => ...`)
      // to reach the second argument. v10's no-empty-pattern flags this; allow it.
      'no-empty-pattern': ['error', { allowObjectPatternsAsParameters: true }],
      '@typescript-eslint/no-unused-vars': [
        'error',
        {
          argsIgnorePattern: '^_',
          varsIgnorePattern: '^_',
          destructuredArrayIgnorePattern: '^_',
          caughtErrorsIgnorePattern: '^_',
        },
      ],
      // Deferred from the v10 upgrade. eslint-plugin-react-hooks v7 added
      // compiler-aware rules (set-state-in-effect, immutability) and
      // react-refresh tightened only-export-components. The codebase has
      // ~30 pre-existing violations; re-enable after a dedicated cleanup
      // pass instead of bundling it into the lint-engine bump.
      'react-hooks/set-state-in-effect': 'off',
      'react-hooks/immutability': 'off',
      'react-refresh/only-export-components': 'off',
    },
  },
  {
    // Test specs match ANSI escape codes in regexes by design (terminal output).
    // Playwright fixture callbacks use `use(value)`; eslint-plugin-react-hooks v7
    // misidentifies these as the React `use` hook.
    files: ['tests/**/*.{ts,tsx}', 'src/**/*.test.{ts,tsx}', 'src/**/__tests__/**'],
    rules: {
      'no-control-regex': 'off',
      'react-hooks/rules-of-hooks': 'off',
    },
  },
  {
    // Ban bare localStorage.setItem in production source. All non-critical
    // writes must route through safeSetItem in src/lib/safeStorage.ts so
    // QuotaExceededError, SecurityError, and private-mode throws stay
    // swallowed. Exceptions (token.ts, deviceBinding.ts) have inline
    // `eslint-disable-next-line no-restricted-syntax` annotations
    // documenting their deliberate rethrow contracts. See
    // docs/development/web-storage.md and #1345.
    files: ['src/**/*.{ts,tsx}'],
    ignores: ['src/**/*.test.{ts,tsx}', 'src/**/__tests__/**'],
    rules: {
      'no-restricted-syntax': [
        'error',
        {
          selector:
            "CallExpression[callee.object.name='localStorage'][callee.property.name='setItem']",
          message:
            'Use safeSetItem from src/lib/safeStorage.ts instead of bare localStorage.setItem.',
        },
        {
          selector:
            "CallExpression[callee.object.object.name='window'][callee.object.property.name='localStorage'][callee.property.name='setItem']",
          message:
            'Use safeSetItem from src/lib/safeStorage.ts instead of bare window.localStorage.setItem.',
        },
      ],
    },
  },
])
