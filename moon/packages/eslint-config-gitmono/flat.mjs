import path from 'node:path'
import { fileURLToPath } from 'node:url'
import { FlatCompat } from '@eslint/eslintrc'
import js from '@eslint/js'

const __dirname = path.dirname(fileURLToPath(import.meta.url))

const compat = new FlatCompat({
  baseDirectory: __dirname,
  resolvePluginsRelativeTo: __dirname,
  recommendedConfig: js.configs.recommended,
  allConfig: js.configs.all
})

/** @type {import('eslint').Linter.Config[]} */
export const base = [...compat.extends('./base.js')]

/** @type {import('eslint').Linter.Config[]} */
export const reactInternal = [...compat.extends('./react-internal.js')]
