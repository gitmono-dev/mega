/**
 * Custom markdown-it parser fields used by createMarkdownParser.
 * TipTap 3 NodeConfig/MarkConfig are stricter; keep these as local augmentations.
 */
import '@tiptap/core'

declare module '@tiptap/core' {
  interface NodeConfig {
    markdownParseSpec?: () => unknown
    markdownToken?: string
  }

  interface MarkConfig {
    markdownParseSpec?: () => unknown
    markdownToken?: string
  }

  interface ExtensionConfig {
    markdownParseSpec?: () => unknown
    markdownToken?: string
  }
}
