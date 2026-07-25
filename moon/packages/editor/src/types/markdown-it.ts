import type Token from 'markdown-it/lib/token.mjs'

/** markdown-it@14 ships Token via .mjs / .d.mts; import the class type directly. */
export type MarkdownItToken = Token
