'use client'

import React from 'react'
import { FitAddon } from '@xterm/addon-fit'
import { Terminal } from '@xterm/xterm'

import { UIText } from '@gitmono/ui'

import { useVmTerminalWebSocket, VmTerminalStatus } from '@/hooks/OrionClient/useVmTerminalWebSocket'

import '@xterm/xterm/css/xterm.css'

interface VmTerminalProps {
  streamKey: string
  className?: string
  /** Fixed panel height in px (matches log panel default). */
  height?: number
}

function statusLabel(status: VmTerminalStatus): string {
  switch (status) {
    case 'connecting':
      return 'Connecting…'
    case 'open':
      return 'Connected'
    case 'error':
      return 'Disconnected'
    default:
      return 'Idle'
  }
}

export function VmTerminal({ streamKey, className, height = 320 }: VmTerminalProps) {
  const containerRef = React.useRef<HTMLDivElement>(null)
  const termRef = React.useRef<Terminal | null>(null)
  const fitRef = React.useRef<FitAddon | null>(null)
  const { status, error, sendBinary, sendResize, subscribeOnData } = useVmTerminalWebSocket(streamKey)

  React.useEffect(() => {
    const el = containerRef.current

    if (!el) return

    const term = new Terminal({
      cursorBlink: true,
      fontSize: 12,
      fontFamily: 'ui-monospace, SFMono-Regular, Menlo, Monaco, Consolas, monospace',
      theme: {
        background: '#0b0f14',
        foreground: '#d7e0ea',
        cursor: '#d7e0ea'
      },
      convertEol: true
    })
    const fit = new FitAddon()

    term.loadAddon(fit)
    term.open(el)
    fit.fit()

    termRef.current = term
    fitRef.current = fit

    const onDataDisposable = term.onData((data) => {
      sendBinary(new TextEncoder().encode(data))
    })

    const onResizeDisposable = term.onResize(({ cols, rows }) => {
      sendResize(cols, rows)
    })

    // Initial size once the socket may already be open / about to open.
    sendResize(term.cols, term.rows)

    const ro = new ResizeObserver(() => {
      try {
        fit.fit()
      } catch {
        // Ignore fit errors while the container is collapsing.
      }
    })

    ro.observe(el)

    return () => {
      ro.disconnect()
      onDataDisposable.dispose()
      onResizeDisposable.dispose()
      term.dispose()
      termRef.current = null
      fitRef.current = null
    }
  }, [sendBinary, sendResize])

  React.useEffect(() => {
    return subscribeOnData((chunk) => {
      termRef.current?.write(chunk)
    })
  }, [subscribeOnData])

  const lastStatusRef = React.useRef<VmTerminalStatus | null>(null)

  React.useEffect(() => {
    const term = termRef.current

    if (!term || lastStatusRef.current === status) return
    lastStatusRef.current = status

    if (status === 'connecting') {
      term.writeln('\r\n\x1b[90mConnecting to VM terminal…\x1b[0m')
    } else if (status === 'open') {
      term.writeln('\r\n\x1b[32mShell ready.\x1b[0m')
      try {
        fitRef.current?.fit()
        sendResize(term.cols, term.rows)
      } catch {
        // ignore
      }
    } else if (status === 'error' && error) {
      term.writeln(`\r\n\x1b[31m${error}\x1b[0m`)
    }
  }, [status, error, sendResize])

  return (
    <div className={className}>
      <div className='mb-1 flex items-center justify-between gap-2'>
        <UIText weight='font-semibold' size='text-sm'>
          VM terminal
        </UIText>
        {status === 'connecting' ? (
          <span className='text-tertiary inline-flex items-center gap-1.5 text-xs'>
            <span
              className='border-tertiary inline-block size-3 animate-spin rounded-full border-2 border-t-transparent'
              aria-hidden
            />
            Connecting…
          </span>
        ) : (
          <UIText size='text-xs' color='text-muted'>
            {statusLabel(status)}
          </UIText>
        )}
      </div>
      {error && status === 'error' ? (
        <UIText size='text-sm' className='mb-1 text-red-600'>
          {error}
        </UIText>
      ) : null}
      <div
        ref={containerRef}
        style={{ height, maxHeight: height }}
        className='w-full overflow-hidden rounded border border-gray-200 bg-[#0b0f14] dark:border-gray-700'
      />
      <UIText size='text-xs' color='text-muted' className='mt-1 block'>
        Interactive shell via mono → orion-scheduler (admin only). Open after the VM is running.
      </UIText>
    </div>
  )
}
