import { base } from '@gitmono/eslint-config/flat.mjs'

/** @type {import('eslint').Linter.Config[]} */
export default [
  {
    ignores: ['dist/**', 'node_modules/**']
  },
  ...base.map((config) =>
    config.ignores
      ? config
      : {
          ...config,
          files: config.files ?? ['**/*.{js,cjs,mjs,ts,tsx}']
        }
  )
]
