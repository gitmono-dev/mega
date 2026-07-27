import { base, reactInternal } from '@gitmono/eslint-config/flat.mjs'

/** @type {import('eslint').Linter.Config[]} */
export default [
  {
    ignores: ['node_modules/**']
  },
  ...[...base, ...reactInternal].map((config) =>
    config.ignores
      ? config
      : {
          ...config,
          files: config.files ?? ['**/*.{js,cjs,mjs,ts,tsx}']
        }
  ),
  // FlatCompat turns override `files` into matchers that miss these paths; keep explicit.
  {
    files: ['**/Avatar/Avatar.tsx', '**/*.stories.tsx'],
    rules: {
      'react-hooks/rules-of-hooks': 'off'
    }
  }
]
