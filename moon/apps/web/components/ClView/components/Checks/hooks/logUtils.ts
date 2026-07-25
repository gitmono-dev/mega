import { GetBuildsLogsV2Data } from '@gitmono/types/generated'

import { type AnsiSegment } from '../ansi'
import { BuildDTO, BuildStatus, TaskInfoDTO } from '../cpns/store'

export type LogStatus = 'idle' | 'loading' | 'success' | 'empty' | 'error'

export const TERMINAL_BUILD_STATUSES = new Set<BuildStatus>(['Completed', 'Failed', 'Interrupted'])

export const MAX_MOUNTED_LOG_PANELS = 6

/** Default log foreground from ansi.ts — used to decide when to apply error highlighting. */
const DEFAULT_LOG_FG = '#d4d4d4'

export const ERROR_LOG_FG = '#f14c4c'

/** Heuristics for plain-text build errors (buck2, rustc, scorpio, etc.). */
const ERROR_LINE_PATTERNS: RegExp[] = [
  /\bBUILD FAILED\b/,
  /\bAction failed\b/i,
  /\(os error \d+\)/i,
  /\bNo such file or directory\b/i,
  /\bPermission denied\b/i,
  /\bBad file descriptor\b/i,
  /\bInput\/output error\b/i,
  /\btransport endpoint is not connected\b/i,
  /\bfatal:\s/i,
  /\bpanicked at\b/i,
  /^\s*error\[/i,
  /^\[error\]/i,
  /: error:/i
]

function stripAnsiSequences(line: string): string {
  return line.replace(/\x1b\[[0-9;?]*m/g, '')
}

/** True when a log line looks like a build/runtime error in plain text. */
export function isErrorLogLine(line: string): boolean {
  const plain = stripAnsiSequences(line).trim()

  if (!plain) return false

  return ERROR_LINE_PATTERNS.some((pattern) => pattern.test(plain))
}

export function isDefaultLogColor(color: string | undefined): boolean {
  return !color || color === DEFAULT_LOG_FG
}

export function applyErrorHighlight(segments: AnsiSegment[]): AnsiSegment[] {
  return segments.map((seg) =>
    isDefaultLogColor(seg.style.color as string | undefined)
      ? { ...seg, style: { ...seg.style, color: ERROR_LOG_FG } }
      : seg
  )
}

export function parseBuildLogResponse(res: GetBuildsLogsV2Data | null | undefined): {
  status: LogStatus
  text: string
} {
  if (!res || !res.data) {
    return { status: 'empty', text: '' }
  }

  if (Array.isArray(res.data) && res.data.length === 0) {
    return { status: 'empty', text: '' }
  }

  if (res.len === 0) {
    return { status: 'empty', text: '' }
  }

  const text = Array.isArray(res.data) ? res.data.join('\n') : String(res.data || '')

  if (!text) {
    return { status: 'empty', text: '' }
  }

  return { status: 'success', text }
}

export function findBuildInTasks(tasks: TaskInfoDTO[], buildId: string): BuildDTO | undefined {
  for (const task of tasks) {
    const build = task.build_list?.find((b) => b.id === buildId)

    if (build) return build
  }

  return undefined
}

/** Adjacent build ids in chronological order within the same task. */
export function getAdjacentBuildIds(tasks: TaskInfoDTO[], buildId: string): string[] {
  for (const task of tasks) {
    const list = [...(task.build_list ?? [])].sort(
      (a, b) => new Date(a.start_at).getTime() - new Date(b.start_at).getTime()
    )
    const index = list.findIndex((b) => b.id === buildId)

    if (index === -1) continue

    const adjacent: string[] = []

    if (index > 0) adjacent.push(list[index - 1].id)
    if (index < list.length - 1) adjacent.push(list[index + 1].id)

    return adjacent
  }

  return []
}
