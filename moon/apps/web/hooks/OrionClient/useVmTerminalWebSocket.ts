import { useCallback, useEffect, useRef, useState } from 'react'

import { MONO_API_URL } from '@gitmono/config'

export type VmTerminalStatus = 'idle' | 'connecting' | 'open' | 'error'

type DataListener = (data: Uint8Array) => void

function terminalWsUrl(streamKey: string): string {
  const base = MONO_API_URL.replace(/\/$/, '')
  const wsBase = base.replace(/^http/i, 'ws')

  return `${wsBase}/api/v1/orion/runners/${encodeURIComponent(streamKey)}/terminal`
}

/**
 * Browser WebSocket client for the mono-proxied VM terminal endpoint.
 * `streamKey` is a scheduler VM id or domain host (same as log stream keys).
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
      setStatus('open')
      setError(null)
    }

    ws.onmessage = (event) => {
      if (closedByEffect) return

      let bytes: Uint8Array | null = null

      if (event.data instanceof ArrayBuffer) {
        bytes = new Uint8Array(event.data)
      } else if (typeof Blob !== 'undefined' && event.data instanceof Blob) {
        void event.data.arrayBuffer().then((buf) => {
          if (closedByEffect) return
          const chunk = new Uint8Array(buf)

          listenersRef.current.forEach((listener) => listener(chunk))
        })
        return
      } else if (typeof event.data === 'string') {
        bytes = new TextEncoder().encode(event.data)
      }

      if (!bytes || bytes.length === 0) return
      listenersRef.current.forEach((listener) => listener(bytes!))
    }

    ws.onerror = () => {
      if (closedByEffect) return
      setStatus('error')
      setError('Terminal connection failed (endpoint may be unavailable)')
    }

    ws.onclose = () => {
      if (closedByEffect) return
      if (wsRef.current === ws) {
        wsRef.current = null
      }
      setStatus((prev) => (prev === 'error' ? prev : 'error'))
      setError((prev) => prev ?? 'Terminal disconnected')
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
