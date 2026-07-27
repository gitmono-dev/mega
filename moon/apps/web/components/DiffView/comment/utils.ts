import type { AnnotationSide, FileDiffMetadata } from '@pierre/diffs'

export function extractLinesFromHunks(fileDiff: FileDiffMetadata, side: 'deletions' | 'additions'): string[] {
  const lines: string[] = []
  const sourceLines = side === 'deletions' ? fileDiff.deletionLines : fileDiff.additionLines

  for (const hunk of fileDiff.hunks) {
    for (const content of hunk.hunkContent) {
      if (content.type === 'context') {
        const start = side === 'deletions' ? content.deletionLineIndex : content.additionLineIndex

        lines.push(...sourceLines.slice(start, start + content.lines))
      } else if (content.type === 'change') {
        if (side === 'deletions') {
          lines.push(
            ...fileDiff.deletionLines.slice(content.deletionLineIndex, content.deletionLineIndex + content.deletions)
          )
        } else {
          lines.push(
            ...fileDiff.additionLines.slice(content.additionLineIndex, content.additionLineIndex + content.additions)
          )
        }
      }
    }
  }

  return lines
}

function getLineContextFromLines(
  lines: string[],
  startLineNumber: number,
  endLineNumber: number,
  contextSize: number
): {
  normalizedContent: string
  contextBefore: string[]
  contextAfter: string[]
  selectedLines: string[]
} | null {
  const startIndex = startLineNumber - 1
  const endIndex = endLineNumber - 1

  if (startIndex < 0 || endIndex >= lines.length || startIndex > endIndex) {
    return null
  }

  const selectedLines = lines.slice(startIndex, endIndex + 1)
  const normalizedContent = selectedLines.join('\n')

  const contextBeforeStart = Math.max(0, startIndex - contextSize)
  const contextBefore = lines.slice(contextBeforeStart, startIndex)

  const contextAfterEnd = Math.min(lines.length, endIndex + contextSize + 1)
  const contextAfter = lines.slice(endIndex + 1, contextAfterEnd)

  return {
    selectedLines,
    normalizedContent,
    contextBefore,
    contextAfter
  }
}

export function getLineContext(
  fileDiff: FileDiffMetadata,
  side: AnnotationSide,
  startLineNumber: number,
  endLineNumber: number | null = null,
  contextSize: number = 3
): {
  normalizedContent: string
  contextBefore: string[]
  contextAfter: string[]
  selectedLines: string[]
} | null {
  const actualEndLineNumber = endLineNumber ?? startLineNumber
  const lines = extractLinesFromHunks(fileDiff, side)

  if (lines.length === 0) {
    return null
  }

  return getLineContextFromLines(lines, startLineNumber, actualEndLineNumber, contextSize)
}
