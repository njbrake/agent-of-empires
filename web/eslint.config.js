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
