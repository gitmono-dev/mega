import { useEffect, useRef, useState } from 'react'

import { MONO_API_URL } from '@gitmono/config'

export type RunnerLogsStatus = 'idle' | 'connecting' | 'streaming' | 'error'

const MAX_LOG_CHARS = 400_000

/** Strip CSI / OSC ANSI sequences so terminal-colored scheduler logs render cleanly in HTML. */
function stripAnsi(text: string): string {
  return text.replace(/\u001b\[[0-9;?]*[ -/]*[@-~]|\u001b\][^\u0007]*(?:\u0007|\u001b\\)/g, '')
}

function runnerLogsStreamUrl(vmId: string): string {
  const base = MONO_API_URL.replace(/\/$/, '')

  return `${base}/api/v1/orion/runners/${encodeURIComponent(vmId)}/logs/stream`
}

/**
 * Subscribe to mono-proxied Orion runner startup logs (SSE).
 * `streamKey` is a scheduler VM id or domain host (client hostname is the WS URL).
 * Enabled while set; closes the EventSource when cleared or unmounted.
 */
export function useRunnerLogsSSE(streamKey: string | null) {
  const [logs, setLogs] = useState('')
  const [status, setStatus] = useState<RunnerLogsStatus>('idle')
  const [error, setError] = useState<string | null>(null)
  const esRef = useRef<EventSource | null>(null)

  useEffect(() => {
    if (!streamKey) {
      esRef.current?.close()
      esRef.current = null
      setLogs('')
      setStatus('idle')
      setError(null)
      return
    }

    setLogs('')
    setError(null)
    setStatus('connecting')

    const es = new EventSource(runnerLogsStreamUrl(streamKey), { withCredentials: true })

    esRef.current = es

    es.onopen = () => {
      setStatus('streaming')
    }

    es.onmessage = (event) => {
      const chunk = stripAnsi(event.data ?? '')

      if (!chunk) return

      setLogs((prev) => {
        // EventSource joins multi-line SSE `data:` fields with `\n` but does not
        // guarantee a trailing newline between successive events.
        const sep = prev && !prev.endsWith('\n') && !chunk.startsWith('\n') ? '\n' : ''
        const next = prev ? `${prev}${sep}${chunk}` : chunk

        if (next.length <= MAX_LOG_CHARS) return next
        return next.slice(next.length - MAX_LOG_CHARS)
      })
    }

    es.onerror = () => {
      // EventSource reconnects automatically on transient errors; only surface a
      // sticky error once the connection is permanently closed.
      if (es.readyState === EventSource.CLOSED) {
        setStatus('error')
        setError('Log stream disconnected')
      }
    }

    return () => {
      es.close()
      if (esRef.current === es) {
        esRef.current = null
      }
    }
  }, [streamKey])

  return { logs, status, error }
}
