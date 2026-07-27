'use client'

import React from 'react'
import { ThemeProvider } from '@primer/react'
import { DataTable } from '@primer/react/experimental'
import { useTheme } from 'next-themes'

import { CoreWorkerStatus, TaskPhase } from '@gitmono/types/generated'
import { Button, Select, SelectTrigger, SelectValue, UIText } from '@gitmono/ui'

import { useGetOrionClientStatusById } from '@/hooks/OrionClient/OrionClientStatusById'

import { StatusBadge } from './StatusBadge'
import { deriveStatus, OrionClient, OrionClientStatus } from './types'

interface ClientsTableProps {
  clients: OrionClient[]
  isLoading?: boolean
  statusFilter: OrionClientStatus | 'all'
  onStatusChange: (v: OrionClientStatus | 'all') => void
  statusOptions: { value: OrionClientStatus | 'all'; label: string }[]
  /** When set, show a per-row action to open that client's runner logs. */
  onViewLogs?: (client: OrionClient) => void
  canViewLogs?: boolean
  capabilityFilter?: string
  capabilityOptions?: string[]
  onCapabilityChange?: (v: string) => void
}

type Row = OrionClient & { statusDerived: OrionClientStatus }
type UniqueRow = Row & { id: string }

/** Primer DataTable uses CSS Grid; fr tracks keep original proportions and can shrink. */
function colWidth(parts: number) {
  return `minmax(0, ${parts}fr)`
}

export function ClientsTable({
  clients,
  isLoading,
  statusFilter,
  onStatusChange,
  statusOptions,
  onViewLogs,
  canViewLogs = false
}: ClientsTableProps) {
  const { resolvedTheme } = useTheme()
  const rows = React.useMemo<UniqueRow[]>(() => {
    return clients.map((c) => ({
      ...c,
      statusDerived: deriveStatus(c),
      id: c.client_id
    }))
  }, [clients])

  const showLogsAction = Boolean(canViewLogs && onViewLogs)

  const columns = React.useMemo(
    () => [
      {
        header: 'Client ID',
        field: 'client_id',
        rowHeader: true,
        // Original ratios: 18/18/10/18/22/14 (or 16/16/10/16/18/14/10 with Logs)
        width: colWidth(showLogsAction ? 16 : 18),
        renderCell: (row: Row) => (
          <div className='min-w-0'>
            <UIText weight='font-semibold' className='block truncate text-sm'>
              {row.client_id}
            </UIText>
          </div>
        )
      },
      {
        header: 'Hostname',
        field: 'hostname',
        width: colWidth(showLogsAction ? 16 : 18),
        renderCell: (row: Row) => <div className='min-w-0 truncate'>{row.hostname || '—'}</div>
      },
      { header: 'Version', field: 'orion_version', width: colWidth(10) },
      {
        header: 'Start Time',
        field: 'start_time',
        width: colWidth(showLogsAction ? 16 : 18),
        renderCell: (row: Row) => <div className='break-words whitespace-normal'>{formatDateTime(row.start_time)}</div>
      },
      {
        header: 'Last Heartbeat',
        field: 'last_heartbeat',
        width: colWidth(showLogsAction ? 18 : 22),
        renderCell: (row: Row) => (
          <div className='flex min-w-0 flex-col gap-0.5 leading-tight'>
            <div className='break-words whitespace-normal'>{formatDateTime(row.last_heartbeat)}</div>
            <UIText tertiary size='text-xs' className='whitespace-nowrap'>
              {formatRelative(row.last_heartbeat)}
            </UIText>
          </div>
        )
      },
      {
        header: () => (
          <Select
            value={statusFilter}
            options={statusOptions}
            onChange={(v) => onStatusChange(v as OrionClientStatus | 'all')}
          >
            <SelectTrigger className='text-tertiary h-auto w-full min-w-0 justify-start gap-1 !border-none !bg-transparent p-0 text-[11px] font-semibold uppercase !shadow-none ring-0 focus:ring-0 focus:outline-hidden'>
              <SelectValue placeholder='Status' />
            </SelectTrigger>
          </Select>
        ),
        field: 'statusDerived',
        width: colWidth(14),
        renderCell: (row: Row) => <OrionClientStatusCell client={row} />
      },
      ...(showLogsAction
        ? [
            {
              header: 'Logs',
              id: 'logs',
              field: 'client_id',
              width: colWidth(10),
              renderCell: (row: Row) => (
                <Button
                  variant='plain'
                  size='sm'
                  onClick={() => onViewLogs?.(row)}
                  accessibilityLabel={`View logs for ${row.client_id}`}
                >
                  View logs
                </Button>
              )
            }
          ]
        : [])
    ],
    [onStatusChange, onViewLogs, showLogsAction, statusFilter, statusOptions]
  )

  if (isLoading) {
    return (
      <div className='flex h-40 items-center justify-center'>
        <UIText tertiary>Loading clients…</UIText>
      </div>
    )
  }

  const isEmpty = !rows || rows.length === 0

  return (
    <ThemeProvider colorMode={resolvedTheme === 'dark' ? 'dark' : 'light'}>
      <div
        className={[
          'border-primary overflow-hidden rounded-md border',
          // Keep Primer grid full-width; drop side/top cell chrome that doubles our outer border.
          '[&_table]:w-full',
          '[&_td]:min-w-0 [&_td]:py-4 [&_th]:min-w-0 [&_th]:py-4',
          '[&_td]:!border-x-0 [&_th]:!border-x-0 [&_th]:!border-t-0',
          '[&_tbody_tr]:border-b [&_tbody_tr:last-child]:border-b-0 [&_thead_tr]:border-b'
        ].join(' ')}
      >
        <DataTable aria-label='Orion clients' data={rows} columns={columns as any} />
        {isEmpty ? (
          <div className='border-primary flex h-40 items-center justify-center border-t'>
            <UIText tertiary>No Orion clients</UIText>
          </div>
        ) : null}
      </div>
    </ThemeProvider>
  )
}

function OrionClientStatusCell({ client }: { client: OrionClient }) {
  const { data, isLoading, isError } = useGetOrionClientStatusById(client.client_id, undefined, 5 * 60 * 1000)

  if (isLoading) {
    return (
      <UIText tertiary size='text-xs'>
        Loading…
      </UIText>
    )
  }

  if (isError || !data) {
    return <StatusBadge status={deriveStatus(client)} />
  }

  return <StatusBadge status={mapApiStatusToUiStatus(data.core_status, data.phase)} />
}

function mapApiStatusToUiStatus(coreStatus: CoreWorkerStatus, phase: TaskPhase | null | undefined): OrionClientStatus {
  if (coreStatus === CoreWorkerStatus.Idle) return 'idle'
  if (coreStatus === CoreWorkerStatus.Error) return 'error'
  if (coreStatus === CoreWorkerStatus.Lost) return 'offline'

  if (coreStatus === CoreWorkerStatus.Busy) {
    if (phase === TaskPhase.DownloadingSource) return 'downloading'
    if (phase === TaskPhase.RunningBuild) return 'running'
    return 'busy'
  }

  return 'idle'
}

function formatDateTime(iso: string) {
  if (!iso) return '—'
  const d = new Date(iso)

  if (Number.isNaN(d.getTime())) return iso
  return d.toLocaleString()
}

function formatRelative(iso: string) {
  const d = new Date(iso)
  const ts = d.getTime()

  if (Number.isNaN(ts)) return 'invalid'

  const diffMs = Date.now() - ts
  const diffSec = Math.max(0, Math.floor(diffMs / 1000))

  if (diffSec < 60) return `${diffSec}s ago`
  const diffMin = Math.floor(diffSec / 60)

  if (diffMin < 60) return `${diffMin}m ago`
  const diffHour = Math.floor(diffMin / 60)

  if (diffHour < 24) return `${diffHour}h ago`
  const diffDay = Math.floor(diffHour / 24)

  return `${diffDay}d ago`
}
