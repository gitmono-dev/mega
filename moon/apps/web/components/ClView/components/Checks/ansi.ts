import type { CSSProperties } from 'react'

export interface AnsiSegment {
  text: string
  style: CSSProperties
  bold?: boolean
}

const ANSI_16: Record<number, string> = {
  0: '#d4d4d4',
  1: '#cd3131',
  2: '#0dbc79',
  3: '#e5e510',
  4: '#2472c8',
  5: '#bc3fbc',
  6: '#11a8cd',
  7: '#e5e5e5',
  8: '#666666',
  9: '#f14c4c',
  10: '#23d18b',
  11: '#f5f543',
  12: '#3b8eea',
  13: '#d670d6',
  14: '#29b8db',
  15: '#ffffff'
}

const DEFAULT_FG = '#d4d4d4'
const DEFAULT_BG = 'transparent'

function color256(n: number): string {
  if (n < 16) return ANSI_16[n] ?? DEFAULT_FG
  if (n < 232) {
    const i = n - 16
    const r = Math.floor(i / 36)
    const g = Math.floor((i % 36) / 6)
    const b = i % 6
    const toHex = (v: number) => (v === 0 ? 0 : 55 + v * 40).toString(16).padStart(2, '0')

    return `#${toHex(r)}${toHex(g)}${toHex(b)}`
  }
  const gray = (n - 232) * 10 + 8

  return `#${gray.toString(16).padStart(2, '0').repeat(3)}`
}

interface StyleState {
  color: string
  backgroundColor: string
  fontWeight: CSSProperties['fontWeight']
  fontStyle: CSSProperties['fontStyle']
  textDecoration: CSSProperties['textDecoration']
  opacity: number
}

function defaultState(): StyleState {
  return {
    color: DEFAULT_FG,
    backgroundColor: DEFAULT_BG,
    fontWeight: 'normal',
    fontStyle: 'normal',
    textDecoration: 'none',
    opacity: 1
  }
}

function toSegmentStyle(state: StyleState): CSSProperties {
  return {
    color: state.color,
    backgroundColor: state.backgroundColor === DEFAULT_BG ? undefined : state.backgroundColor,
    fontWeight: state.fontWeight,
    fontStyle: state.fontStyle,
    textDecoration: state.textDecoration,
    opacity: state.opacity < 1 ? state.opacity : undefined
  }
}

function applyCode(state: StyleState, code: number): void {
  if (code === 0) {
    Object.assign(state, defaultState())
    return
  }
  if (code === 1) {
    state.fontWeight = 'bold'
    return
  }
  if (code === 22) {
    state.fontWeight = 'normal'
    state.opacity = 1
    return
  }
  if (code === 2) {
    state.opacity = 0.65
    return
  }
  if (code === 3) {
    state.fontStyle = 'italic'
    return
  }
  if (code === 23) {
    state.fontStyle = 'normal'
    return
  }
  if (code === 4) {
    state.textDecoration = 'underline'
    return
  }
  if (code === 24) {
    state.textDecoration = 'none'
    return
  }
  if (code === 39) {
    state.color = DEFAULT_FG
    return
  }
  if (code === 49) {
    state.backgroundColor = DEFAULT_BG
    return
  }
  if (code >= 30 && code <= 37) {
    state.color = ANSI_16[code - 30] ?? DEFAULT_FG
    return
  }
  if (code >= 90 && code <= 97) {
    state.color = ANSI_16[code - 90 + 8] ?? DEFAULT_FG
    return
  }
  if (code >= 40 && code <= 47) {
    state.backgroundColor = ANSI_16[code - 40] ?? DEFAULT_BG
    return
  }
  if (code >= 100 && code <= 107) {
    state.backgroundColor = ANSI_16[code - 100 + 8] ?? DEFAULT_BG
  }
}

function applyExtended(state: StyleState, codes: number[], start: number): number {
  const kind = codes[start]

  if (kind === 38 && codes[start + 1] === 5 && codes[start + 2] !== undefined) {
    state.color = color256(codes[start + 2])
    return start + 2
  }
  if (
    kind === 38 &&
    codes[start + 1] === 2 &&
    codes[start + 2] !== undefined &&
    codes[start + 3] !== undefined &&
    codes[start + 4] !== undefined
  ) {
    const r = codes[start + 2]
    const g = codes[start + 3]
    const b = codes[start + 4]

    state.color = `#${r.toString(16).padStart(2, '0')}${g.toString(16).padStart(2, '0')}${b.toString(16).padStart(2, '0')}`
    return start + 4
  }
  if (kind === 48 && codes[start + 1] === 5 && codes[start + 2] !== undefined) {
    state.backgroundColor = color256(codes[start + 2])
    return start + 2
  }
  if (
    kind === 48 &&
    codes[start + 1] === 2 &&
    codes[start + 2] !== undefined &&
    codes[start + 3] !== undefined &&
    codes[start + 4] !== undefined
  ) {
    const r = codes[start + 2]
    const g = codes[start + 3]
    const b = codes[start + 4]

    state.backgroundColor = `#${r.toString(16).padStart(2, '0')}${g.toString(16).padStart(2, '0')}${b.toString(16).padStart(2, '0')}`
    return start + 4
  }

  return start
}

/** Strip non-SGR escape sequences (cursor moves, erase, etc.). */
function stripNonSgr(input: string): string {
  return input.replace(/\x1b\[[0-9;?]*[A-Za-z]/g, (seq) => {
    const letter = seq.slice(-1)

    if (letter === 'm') return seq

    return ''
  })
}

/**
 * Parse a single log line with ANSI SGR codes into styled segments.
 */
export function parseAnsi(line: string): AnsiSegment[] {
  const cleaned = stripNonSgr(line)
  const segments: AnsiSegment[] = []
  const state = defaultState()
  let i = 0
  let textStart = 0

  const pushText = (end: number) => {
    if (end <= textStart) return

    const text = cleaned.slice(textStart, end)

    if (!text) return

    segments.push({
      text,
      style: toSegmentStyle(state),
      bold: state.fontWeight === 'bold'
    })
  }

  while (i < cleaned.length) {
    if (cleaned[i] === '\x1b' && cleaned[i + 1] === '[') {
      pushText(i)
      const end = cleaned.indexOf('m', i)

      if (end === -1) {
        textStart = i
        break
      }

      const body = cleaned.slice(i + 2, end)
      const codes = body.split(';').map((p) => (p === '' ? 0 : Number.parseInt(p, 10)))

      for (let c = 0; c < codes.length; c++) {
        const code = codes[c]

        if (Number.isNaN(code)) continue

        if (code === 38 || code === 48) {
          c = applyExtended(state, codes, c)
        } else {
          applyCode(state, code)
        }
      }

      i = end + 1
      textStart = i
      continue
    }

    i += 1
  }

  pushText(cleaned.length)

  if (segments.length === 0) {
    segments.push({ text: cleaned, style: { color: DEFAULT_FG } })
  }

  return segments
}
