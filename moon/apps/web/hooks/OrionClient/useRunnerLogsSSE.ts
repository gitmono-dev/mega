import { useEffect, useRef, useState } from 'react'

import { MONO_API_URL } from '@gitmono/config'

export type RunnerLogsStatus = 'idle' | 'connecting' | 'streaming' | 'error'

const MAX_LOG_CHARS = 400_000

/** Older schedulers spam this every second while the VM is still provisioning. */
const TRANSIENT_NO_VM_RE = /^Error:\s*No running VM for key\b/i

/** Strip CSI / OSC ANSI sequences so terminal-colored scheduler logs render cleanly in HTML. */
function stripAnsi(text: string): string {
  return text.replace(/\u001b\[[0-9;?]*[ -/]*[@-~]|\u001b\][^\u0007]*(?:\u0007|\u001b\\)/g, '')
}

function runnerLogsStreamUrl(vmId: string): string {
  const base = MONO_API_URL.replace(/\/$/, '')

  return `${base}/api/v1/orion/runners/${encodeURIComponent(vmId)}/logs/stream`
}

/**
 * Drop repeated "No running VM" errors from older schedulers, keeping a single
 * waiting notice until real log lines arrive.
 */
function filterTransientVmErrors(chunk: string, alreadyWaiting: boolean): { text: string; waiting: boolean } {
  const lines = chunk.split('\n')
  const kept: string[] = []
  let waiting = alreadyWaiting

  for (const line of lines) {
    if (TRANSIENT_NO_VM_RE.test(line.trim())) {
      if (!waiting) {
        kept.push('Waiting for VM to finish provisioning…')
        waiting = true
      }
      continue
    }
    kept.push(line)
  }

  return { text: kept.join('\n'), waiting }
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
  const waitingForVmRef = useRef(false)

  useEffect(() => {
    if (!streamKey) {
      esRef.current?.close()
      esRef.current = null
      setLogs('')
      setStatus('idle')
      setError(null)
      waitingForVmRef.current = false
      return
    }

    setLogs('')
    setError(null)
    setStatus('connecting')
    waitingForVmRef.current = false

    const es = new EventSource(runnerLogsStreamUrl(streamKey), { withCredentials: true })

    esRef.current = es

    es.onopen = () => {
      setStatus('streaming')
    }

    es.onmessage = (event) => {
      const raw = stripAnsi(event.data ?? '')

      if (!raw) return

      const { text: chunk, waiting } = filterTransientVmErrors(raw, waitingForVmRef.current)

      waitingForVmRef.current = waiting

      if (!chunk.trim()) return

      // Real log content arrived — clear the transient-wait gate so a later
      // reprovision can announce waiting again if needed.
      if (!TRANSIENT_NO_VM_RE.test(chunk.trim()) && !chunk.includes('Waiting for VM')) {
        waitingForVmRef.current = false
      }

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
