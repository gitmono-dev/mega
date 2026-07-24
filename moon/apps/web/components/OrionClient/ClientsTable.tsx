'use client'

import React from 'react'
import { DataTable } from '@primer/react/experimental'

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

export function ClientsTable({
  clients,
  isLoading,
  statusFilter,
  onStatusChange,
  statusOptions,
  onViewLogs,
  canViewLogs = false
}: ClientsTableProps) {
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
        width: showLogsAction ? '16%' : '18%',
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
        width: showLogsAction ? '16%' : '18%',
        renderCell: (row: Row) => <div className='min-w-0 truncate'>{row.hostname || '—'}</div>
      },
      { header: 'Version', field: 'orion_version', width: '10%' },
      {
        header: 'Start Time',
        field: 'start_time',
        width: showLogsAction ? '16%' : '18%',
        renderCell: (row: Row) => <div className='whitespace-normal break-words'>{formatDateTime(row.start_time)}</div>
      },
      {
        header: 'Last Heartbeat',
        field: 'last_heartbeat',
        width: showLogsAction ? '18%' : '22%',
        renderCell: (row: Row) => (
          <div className='flex flex-col gap-0.5 leading-tight'>
            <div className='whitespace-normal break-words'>{formatDateTime(row.last_heartbeat)}</div>
            <UIText color='text-muted' size='text-xs' className='whitespace-nowrap'>
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
            <SelectTrigger className='text-muted-foreground h-auto w-full justify-start gap-1 !border-none !bg-transparent p-0 text-[11px] font-semibold uppercase !shadow-none ring-0 focus:outline-none focus:ring-0'>
              <SelectValue placeholder='Status' />
            </SelectTrigger>
          </Select>
        ),
        field: 'statusDerived',
        width: '14%',
        renderCell: (row: Row) => <OrionClientStatusCell client={row} />
      },
      ...(showLogsAction
        ? [
            {
              header: 'Logs',
              field: 'client_id',
              width: '10%',
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
        <UIText color='text-muted'>Loading clients…</UIText>
      </div>
    )
  }

  const isEmpty = !rows || rows.length === 0

  return (
    <div className='border-border overflow-hidden rounded-md border [&_table]:w-full [&_table]:table-fixed [&_tbody_tr:last-child]:border-b-0 [&_tbody_tr]:border-b [&_td]:py-4 [&_th]:py-4 [&_thead_tr]:border-b'>
      <DataTable aria-label='Orion clients' data={rows} columns={columns as any} />
      {isEmpty ? (
        <div className='border-border flex h-40 items-center justify-center border-t'>
          <UIText color='text-muted'>No Orion clients</UIText>
        </div>
      ) : null}
    </div>
  )
}

function OrionClientStatusCell({ client }: { client: OrionClient }) {
  const { data, isLoading, isError } = useGetOrionClientStatusById(client.client_id, undefined, 5 * 60 * 1000)

  if (isLoading) {
    return (
      <UIText color='text-muted' size='text-xs'>
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
