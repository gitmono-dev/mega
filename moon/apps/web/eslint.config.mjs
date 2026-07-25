import tanstackQuery from '@tanstack/eslint-plugin-query'
import nextVitals from 'eslint-config-next/core-web-vitals'
import storybook from 'eslint-plugin-storybook'
import unusedImports from 'eslint-plugin-unused-imports'

/** @type {import('eslint').Linter.Config[]} */
const eslintConfig = [
  ...nextVitals,
  ...storybook.configs['flat/recommended'],
  {
    plugins: {
      '@tanstack/query': tanstackQuery,
      'unused-imports': unusedImports
    },
    settings: {
      react: {
        version: 'detect'
      }
    },
    languageOptions: {
      globals: {
        React: 'writable'
      }
    },
    rules: {
      // @gitmono/eslint-config/base.js (subset; avoid redefining @typescript-eslint from next)
      'no-console': ['error', { allow: ['warn', 'error'] }],
      'no-irregular-whitespace': 'error',
      'no-empty-function': 'error',
      'newline-after-var': 'error',
      'no-unused-vars': 'off',
      'no-fallthrough': ['error', { allowEmptyCase: true }],
      'no-extra-semi': 'off',
      'max-lines': 'off',
      'unused-imports/no-unused-imports': 'error',
      'unused-imports/no-unused-vars': [
        'warn',
        {
          vars: 'all',
          varsIgnorePattern: '^_',
          args: 'after-used',
          argsIgnorePattern: '^_'
        }
      ],

      // @gitmono/eslint-config/next.js
      // Plugin v5 is stricter (often flags queryClient/params); keep as warn during Next 16 cutover.
      '@tanstack/query/exhaustive-deps': 'warn',
      'storybook/no-renderer-packages': 'warn',
      'react/no-array-index-key': 'error',
      'react-hooks/exhaustive-deps': 'error',
      'react/prop-types': 'off',

      // eslint-plugin-react-hooks@7 / React Compiler rules (next 16 defaults).
      // Existing Pages code trips these widely; keep visible as warnings for follow-up.
      'react-hooks/set-state-in-effect': 'warn',
      'react-hooks/set-state-in-render': 'warn',
      'react-hooks/refs': 'warn',
      'react-hooks/immutability': 'warn',
      'react-hooks/preserve-manual-memoization': 'warn',
      'react-hooks/static-components': 'warn',
      'react-hooks/purity': 'warn',
      'react-hooks/incompatible-library': 'warn',
      'react-hooks/use-memo': 'warn',

      // rules/no-restricted-imports
      'no-restricted-imports': [
        'error',
        {
          paths: [
            {
              name: 'next/link',
              message: "Please import it from '@gitmono/ui/Link' instead."
            },
            {
              name: 'framer-motion',
              importNames: ['useInView'],
              message: "Please import it from 'react-intersection-observer' instead."
            },
            {
              name: 'react-hotkeys-hook',
              importNames: ['useHotkeys'],
              message: "Please import it from '@gitmono/ui' instead."
            },
            {
              name: 'react-error-boundary',
              message: "Please import it from '@gitmono/ui' instead."
            }
          ]
        }
      ]
    }
  },
  {
    files: ['**/__tests__/**/*'],
    languageOptions: {
      globals: {
        jest: true
      }
    }
  },
  {
    ignores: ['.next/**', 'out/**', 'build/**', 'next-env.d.ts', 'storybook-static/**', 'public/**']
  }
]

export default eslintConfig
