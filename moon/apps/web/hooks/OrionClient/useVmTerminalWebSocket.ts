import { useCallback, useEffect, useRef, useState } from 'react'

import { MONO_API_URL } from '@gitmono/config'

export type VmTerminalStatus = 'idle' | 'connecting' | 'open' | 'error'

type DataListener = (data: Uint8Array) => void

function terminalWsUrl(streamKey: string): string {
  const base = MONO_API_URL.replace(/\/$/, '')
  const wsBase = base.replace(/^http/i, 'ws')

  return `${wsBase}/api/v1/orion/runners/${encodeURIComponent(streamKey)}/terminal`
}

function isFatalTerminalText(text: string): boolean {
  return text.startsWith('Failed to open') || text.startsWith('Error:')
}

/**
 * Browser WebSocket client for the mono-proxied VM terminal endpoint.
 * `streamKey` is a scheduler VM id or domain host (same as log stream keys).
 *
 * Status stays `connecting` until the scheduler sends `Shell ready` (or PTY binary),
 * so a bare TCP upgrade is not shown as Connected while SSH is still opening.
 */
export function useVmTerminalWebSocket(streamKey: string | null) {
  const [status, setStatus] = useState<VmTerminalStatus>('idle')
  const [error, setError] = useState<string | null>(null)
  const wsRef = useRef<WebSocket | null>(null)
  const listenersRef = useRef(new Set<DataListener>())

  const subscribeOnData = useCallback((listener: DataListener) => {
    listenersRef.current.add(listener)
    return () => {
      listenersRef.current.delete(listener)
    }
  }, [])

  const sendBinary = useCallback((data: Uint8Array | ArrayBuffer) => {
    const ws = wsRef.current

    if (!ws || ws.readyState !== WebSocket.OPEN) return
    if (data instanceof ArrayBuffer) {
      ws.send(data)
      return
    }
    // Copy into a concrete ArrayBuffer view — TS DOM typings reject SharedArrayBuffer-backed views.
    const copy = new Uint8Array(data.byteLength)

    copy.set(data)
    ws.send(copy)
  }, [])

  const sendResize = useCallback((cols: number, rows: number) => {
    const ws = wsRef.current

    if (!ws || ws.readyState !== WebSocket.OPEN) return
    ws.send(JSON.stringify({ type: 'resize', cols, rows }))
  }, [])

  useEffect(() => {
    if (!streamKey) {
      wsRef.current?.close()
      wsRef.current = null
      setStatus('idle')
      setError(null)
      return
    }

    setStatus('connecting')
    setError(null)

    let closedByEffect = false
    let sawOpen = false
    let shellReady = false
    let ws: WebSocket

    try {
      // Cookies for the mono API host are sent automatically on this handshake
      // (same pattern as EventSource withCredentials for first-party / trusted API hosts).
      ws = new WebSocket(terminalWsUrl(streamKey))
    } catch (e) {
      setStatus('error')
      setError(e instanceof Error ? e.message : 'Failed to open terminal WebSocket')
      return
    }

    ws.binaryType = 'arraybuffer'
    wsRef.current = ws

    ws.onopen = () => {
      if (closedByEffect) return
      sawOpen = true
      // Stay in connecting until Shell ready / first PTY bytes.
      setStatus('connecting')
      setError(null)
    }

    const markReady = () => {
      if (shellReady || closedByEffect) return
      shellReady = true
      setStatus('open')
      setError(null)
    }

    ws.onmessage = (event) => {
      if (closedByEffect) return

      let bytes: Uint8Array | null = null

      if (event.data instanceof ArrayBuffer) {
        bytes = new Uint8Array(event.data)
        if (bytes.length > 0) markReady()
      } else if (typeof Blob !== 'undefined' && event.data instanceof Blob) {
        void event.data.arrayBuffer().then((buf) => {
          if (closedByEffect) return
          const chunk = new Uint8Array(buf)

          if (chunk.length > 0) markReady()
          listenersRef.current.forEach((listener) => listener(chunk))
        })
        return
      } else if (typeof event.data === 'string') {
        if (isFatalTerminalText(event.data)) {
          setStatus('error')
          setError(event.data)
          return
        }
        if (event.data === 'Shell ready') {
          markReady()
          return
        }
        // Status text from proxy/scheduler (e.g. Opening interactive shell…)
        bytes = new TextEncoder().encode(`${event.data}\r\n`)
      }

      if (!bytes || bytes.length === 0) return
      listenersRef.current.forEach((listener) => listener(bytes!))
    }

    ws.onerror = () => {
      if (closedByEffect) return
      setStatus('error')
      setError((prev) => prev ?? 'Terminal connection failed (check mono ↔ scheduler and admin auth)')
    }

    ws.onclose = (ev) => {
      if (closedByEffect) return
      if (wsRef.current === ws) {
        wsRef.current = null
      }
      const reason = (ev.reason || '').trim()

      setStatus('error')
      setError((prev) => {
        if (prev) return prev
        if (reason) return reason
        if (!sawOpen) return 'Terminal handshake failed (unauthorized, missing scheduler, or route unavailable)'
        if (!shellReady) return 'Terminal closed before shell was ready'
        return 'Terminal disconnected'
      })
    }

    return () => {
      closedByEffect = true
      ws.close()
      if (wsRef.current === ws) {
        wsRef.current = null
      }
    }
  }, [streamKey])

  return { status, error, sendBinary, sendResize, subscribeOnData }
}
