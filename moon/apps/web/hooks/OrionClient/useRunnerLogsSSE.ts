import { useEffect, useRef, useState } from 'react'

import { MONO_API_URL } from '@gitmono/config'

export type RunnerLogsStatus = 'idle' | 'connecting' | 'streaming' | 'error'

const MAX_LOG_CHARS = 400_000

function runnerLogsStreamUrl(vmId: string): string {
  const base = MONO_API_URL.replace(/\/$/, '')

  return `${base}/api/v1/orion/runners/${encodeURIComponent(vmId)}/logs/stream`
}

/**
 * Subscribe to mono-proxied Orion runner startup logs (SSE).
 * Enabled while `vmId` is set; closes the EventSource when cleared or unmounted.
 */
export function useRunnerLogsSSE(vmId: string | null) {
  const [logs, setLogs] = useState('')
  const [status, setStatus] = useState<RunnerLogsStatus>('idle')
  const [error, setError] = useState<string | null>(null)
  const esRef = useRef<EventSource | null>(null)

  useEffect(() => {
    if (!vmId) {
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

    const es = new EventSource(runnerLogsStreamUrl(vmId), { withCredentials: true })

    esRef.current = es

    es.onopen = () => {
      setStatus('streaming')
    }

    es.onmessage = (event) => {
      const chunk = event.data

      if (!chunk) return

      setLogs((prev) => {
        const next = prev ? `${prev}${chunk}` : chunk

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
  }, [vmId])

  return { logs, status, error }
}
