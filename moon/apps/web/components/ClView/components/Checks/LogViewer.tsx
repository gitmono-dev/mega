import {
  forwardRef,
  memo,
  useCallback,
  useMemo,
  useRef,
  useState,
  type CSSProperties,
  type HTMLAttributes,
  type ReactNode
} from 'react'
import { Virtuoso, type VirtuosoHandle } from 'react-virtuoso'

import { parseAnsi, type AnsiSegment } from './ansi'
import { applyErrorHighlight, isErrorLogLine } from './hooks/logUtils'

export interface LogViewerProps {
  text: string
  height: number | 'auto'
}

const TOOLBAR_HEIGHT = 40
const LINE_NUMBER_WIDTH = 52
const GUTTER_PAD = 'px-2 py-0.5'

/** Split log text into lines; drop a single trailing empty line from a final newline. */
export function splitLogLines(text: string): string[] {
  if (!text) return []

  return text.endsWith('\n') ? text.slice(0, -1).split('\n') : text.split('\n')
}

interface LogRowProps {
  lineNumber: number
  line: string
  wrap: boolean
  isSearchMatch: boolean
  isCurrentMatch: boolean
  searchQuery: string
  getSegments: (line: string) => AnsiSegment[]
}

function highlightSegmentText(text: string, query: string, baseStyle: CSSProperties): ReactNode {
  if (!query) return text

  const lowerText = text.toLowerCase()
  const lowerQuery = query.toLowerCase()
  const parts: ReactNode[] = []
  let start = 0

  while (start < text.length) {
    const idx = lowerText.indexOf(lowerQuery, start)

    if (idx === -1) {
      parts.push(text.slice(start))
      break
    }

    if (idx > start) {
      parts.push(text.slice(start, idx))
    }

    parts.push(
      <mark key={`${idx}-${start}`} className='rounded-xs bg-yellow-500/40 text-inherit' style={baseStyle}>
        {text.slice(idx, idx + query.length)}
      </mark>
    )
    start = idx + query.length
  }

  return parts
}

const LogRow = memo(function LogRow({
  lineNumber,
  line,
  wrap,
  isSearchMatch,
  isCurrentMatch,
  searchQuery,
  getSegments
}: LogRowProps) {
  const isErrorLine = isErrorLogLine(line)
  const segments = isErrorLine ? applyErrorHighlight(getSegments(line)) : getSegments(line)

  return (
    <div
      className={`flex min-h-[20px] font-mono text-[12px] leading-5 ${
        isCurrentMatch ? 'bg-yellow-500/15' : isSearchMatch ? 'bg-white/5' : isErrorLine ? 'bg-red-950/25' : ''
      }`}
    >
      <div
        className={`text-tertiary shrink-0 text-right select-none ${GUTTER_PAD}`}
        style={{ width: LINE_NUMBER_WIDTH, minWidth: LINE_NUMBER_WIDTH }}
      >
        {lineNumber}
      </div>
      <div
        className={`min-w-0 flex-1 select-text ${GUTTER_PAD} ${wrap ? 'break-all whitespace-pre-wrap' : 'whitespace-pre'}`}
      >
        {(() => {
          let offset = 0

          return segments.map((seg) => {
            const key = `${offset}:${seg.text.length}`

            offset += seg.text.length

            return (
              <span key={key} style={seg.style}>
                {searchQuery ? highlightSegmentText(seg.text, searchQuery, seg.style) : seg.text}
              </span>
            )
          })
        })()}
      </div>
    </div>
  )
})

const LogScroller = forwardRef<HTMLDivElement, HTMLAttributes<HTMLDivElement>>(function LogScroller(props, ref) {
  return (
    <div {...props} ref={ref} className={`log-viewer-scroll bg-[#1e1e1e] text-[#d4d4d4] ${props.className ?? ''}`} />
  )
})

export function LogViewer({ text, height }: LogViewerProps) {
  const virtuosoRef = useRef<VirtuosoHandle>(null)
  const segmentCacheRef = useRef<Map<string, AnsiSegment[]>>(new Map())

  const [searchQuery, setSearchQuery] = useState('')
  const [currentMatchIdx, setCurrentMatchIdx] = useState(0)
  const [wrap, setWrap] = useState(false)
  const [follow, setFollow] = useState(true)
  const [atBottom, setAtBottom] = useState(true)
  const [copied, setCopied] = useState(false)

  const lines = useMemo(() => splitLogLines(text), [text])

  const matches = useMemo(() => {
    if (!searchQuery.trim()) return []

    const q = searchQuery.toLowerCase()

    return lines.reduce<number[]>((acc, line, index) => {
      if (line.toLowerCase().includes(q)) acc.push(index)

      return acc
    }, [])
  }, [lines, searchQuery])

  const getSegments = useCallback((line: string) => {
    const cache = segmentCacheRef.current
    const cached = cache.get(line)

    if (cached) return cached

    const parsed = parseAnsi(line)

    cache.set(line, parsed)

    if (cache.size > 5000) {
      const firstKey = cache.keys().next().value

      if (firstKey !== undefined) cache.delete(firstKey)
    }

    return parsed
  }, [])

  const scrollToMatch = useCallback(
    (matchListIndex: number) => {
      const lineIndex = matches[matchListIndex]

      if (lineIndex === undefined) return

      setCurrentMatchIdx(matchListIndex)
      virtuosoRef.current?.scrollToIndex({ index: lineIndex, align: 'center', behavior: 'smooth' })
    },
    [matches]
  )

  const goToPrevMatch = useCallback(() => {
    if (matches.length === 0) return

    const next = (currentMatchIdx - 1 + matches.length) % matches.length

    scrollToMatch(next)
  }, [currentMatchIdx, matches, scrollToMatch])

  const goToNextMatch = useCallback(() => {
    if (matches.length === 0) return

    const next = (currentMatchIdx + 1) % matches.length

    scrollToMatch(next)
  }, [currentMatchIdx, matches, scrollToMatch])

  const handleSearchChange = useCallback((value: string) => {
    setSearchQuery(value)
    setCurrentMatchIdx(0)
  }, [])

  const handleCopyAll = useCallback(async () => {
    try {
      await navigator.clipboard.writeText(text)
      setCopied(true)
      setTimeout(() => setCopied(false), 1500)
    } catch {
      // ignore
    }
  }, [text])

  const jumpToBottom = useCallback(() => {
    setFollow(true)
    virtuosoRef.current?.scrollToIndex({ index: lines.length - 1, align: 'end', behavior: 'smooth' })
  }, [lines.length])

  const containerStyle: CSSProperties =
    height === 'auto' ? { height: '100%' } : { height: Math.max(height - TOOLBAR_HEIGHT, 0) }

  const currentMatchLine = matches[currentMatchIdx]

  return (
    <div className='flex h-full flex-col overflow-hidden rounded-xs border border-[#333]'>
      <div
        className='flex shrink-0 items-center gap-2 border-b border-[#333] bg-[#252526] px-2'
        style={{ height: TOOLBAR_HEIGHT }}
      >
        <input
          type='search'
          placeholder='Search logs…'
          value={searchQuery}
          onChange={(e) => handleSearchChange(e.target.value)}
          onKeyDown={(e) => {
            if (e.key === 'Enter') {
              e.shiftKey ? goToPrevMatch() : goToNextMatch()
            }
          }}
          className='h-7 min-w-0 flex-1 rounded border border-[#3c3c3c] bg-[#3c3c3c] px-2 text-xs text-[#d4d4d4] placeholder:text-[#888] focus:border-[#007acc] focus:outline-hidden'
        />
        {searchQuery.trim() ? (
          <span className='shrink-0 text-xs text-[#888]'>
            {matches.length === 0 ? '0' : `${currentMatchIdx + 1}`}/{matches.length}
          </span>
        ) : null}
        <button
          type='button'
          onClick={goToPrevMatch}
          disabled={matches.length === 0}
          className='shrink-0 rounded px-1.5 py-0.5 text-xs text-[#ccc] hover:bg-[#3c3c3c] disabled:opacity-40'
          title='Previous match (Shift+Enter)'
        >
          ↑
        </button>
        <button
          type='button'
          onClick={goToNextMatch}
          disabled={matches.length === 0}
          className='shrink-0 rounded px-1.5 py-0.5 text-xs text-[#ccc] hover:bg-[#3c3c3c] disabled:opacity-40'
          title='Next match (Enter)'
        >
          ↓
        </button>
        <button
          type='button'
          onClick={() => setWrap((w) => !w)}
          className={`shrink-0 rounded px-2 py-0.5 text-xs ${wrap ? 'bg-[#094771] text-white' : 'text-[#ccc] hover:bg-[#3c3c3c]'}`}
          title='Toggle line wrap'
        >
          Wrap
        </button>
        <button
          type='button'
          onClick={() => setFollow((f) => !f)}
          className={`shrink-0 rounded px-2 py-0.5 text-xs ${follow ? 'bg-[#094771] text-white' : 'text-[#ccc] hover:bg-[#3c3c3c]'}`}
          title='Auto-scroll to bottom on new lines'
        >
          Follow
        </button>
        <button
          type='button'
          onClick={handleCopyAll}
          className='shrink-0 rounded px-2 py-0.5 text-xs text-[#ccc] hover:bg-[#3c3c3c]'
          title='Copy entire log'
        >
          {copied ? 'Copied' : 'Copy'}
        </button>
      </div>

      <div className='relative min-h-0 flex-1' style={containerStyle}>
        {!follow && !atBottom && lines.length > 0 ? (
          <button
            type='button'
            onClick={jumpToBottom}
            className='absolute right-4 bottom-3 z-10 rounded-full bg-[#007acc] px-3 py-1 text-xs text-white shadow-lg hover:bg-[#0062a3]'
          >
            Jump to bottom
          </button>
        ) : null}

        <Virtuoso
          ref={virtuosoRef}
          style={{ height: '100%' }}
          data={lines}
          followOutput={follow ? 'smooth' : false}
          atBottomStateChange={setAtBottom}
          increaseViewportBy={{ top: 400, bottom: 400 }}
          components={{ Scroller: LogScroller }}
          itemContent={(index, line) => (
            <LogRow
              lineNumber={index + 1}
              line={line}
              wrap={wrap}
              isSearchMatch={matches.includes(index)}
              isCurrentMatch={index === currentMatchLine}
              searchQuery={searchQuery.trim()}
              getSegments={getSegments}
            />
          )}
        />
      </div>
    </div>
  )
}

/** Keep LogViewer mounted but visually hidden so Virtuoso retains scroll position. */
export const CachedLogPanel = memo(function CachedLogPanel({
  text,
  height,
  visible
}: LogViewerProps & { visible: boolean }) {
  return (
    <div
      className='absolute inset-0'
      style={{
        visibility: visible ? 'visible' : 'hidden',
        zIndex: visible ? 1 : 0,
        pointerEvents: visible ? 'auto' : 'none'
      }}
      aria-hidden={!visible}
    >
      <LogViewer text={text} height={height} />
    </div>
  )
})
