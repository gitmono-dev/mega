/** @type {import("eslint").Linter.Config} */
module.exports = {
  extends: [
    'plugin:react/recommended',
    'plugin:react-hooks/recommended',
    require.resolve('./rules/no-restricted-imports')
  ],
  globals: {
    React: 'writable'
  },
  env: {
    browser: true
  },
  settings: {
    next: {
      rootDir: ['apps/*/', 'packages/*/']
    },
    react: {
      version: '19.2'
    }
  },
  rules: {
    'no-console': ['error', { allow: ['warn', 'error'] }],
    'react/no-array-index-key': 'error',
    'react/prop-types': 'off',
    'react-hooks/exhaustive-deps': 'error',

    // eslint-plugin-react-hooks@7 / React Compiler rules.
    // Existing package code trips these widely; keep visible as warnings for follow-up.
    'react-hooks/set-state-in-effect': 'warn',
    'react-hooks/set-state-in-render': 'warn',
    'react-hooks/refs': 'warn',
    'react-hooks/immutability': 'warn',
    'react-hooks/preserve-manual-memoization': 'warn',
    'react-hooks/static-components': 'warn',
    'react-hooks/purity': 'warn',
    'react-hooks/incompatible-library': 'warn',
    'react-hooks/use-memo': 'warn'
  },
  overrides: [
    {
      files: ['**/*.stories.tsx'],
      rules: {
        'react-hooks/rules-of-hooks': 'off'
      }
    },
    {
      // Internal helpers sometimes use leading underscore (e.g. _Avatar).
      files: ['**/Avatar/Avatar.tsx'],
      rules: {
        'react-hooks/rules-of-hooks': 'off'
      }
    }
  ]
}
