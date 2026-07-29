'use client'

import React from 'react'
import { ThemeProvider } from '@primer/react'
import { DataTable } from '@primer/react/experimental'
import { useTheme } from 'next-themes'

import type { RunnerStatusResponse } from '@gitmono/types/generated'
import { Badge, Button, UIText } from '@gitmono/ui'

function colWidth(parts: number) {
  return `minmax(0, ${parts}fr)`
}

function shortDigest(digest: string | null | undefined): string | null {
  if (!digest) return null
  return digest.replace(/^sha256:/, '').slice(0, 12)
}

function formatUptime(totalSecs: number | null | undefined): string {
  if (totalSecs == null) return '—'
  const secs = Math.max(0, Math.floor(totalSecs))
  const h = Math.floor(secs / 3600)
  const m = Math.floor((secs % 3600) / 60)
  const s = secs % 60

  if (h > 0) return `${h}h ${m}m`
  if (m > 0) return `${m}m ${s}s`
  return `${s}s`
}

function phaseBadge(phase: string) {
  const normalized = phase.toLowerCase()
  const color =
    normalized === 'running'
      ? 'green'
      : normalized === 'provisioning'
        ? 'blue'
        : normalized === 'failed'
          ? 'brand'
          : 'default'

  return (
    <Badge className='min-w-[110px] justify-center px-3 py-0.5 text-xs capitalize' color={color}>
      {phase || 'unknown'}
    </Badge>
  )
}

interface RunnersTableProps {
  runners: RunnerStatusResponse[]
  isLoading?: boolean
  errorMessage?: string | null
  onViewLogs?: (runner: RunnerStatusResponse) => void
  onConnectTerminal?: (runner: RunnerStatusResponse) => void
  canManage?: boolean
}

type Row = RunnerStatusResponse & { id: string }

export function RunnersTable({
  runners,
  isLoading,
  errorMessage,
  onViewLogs,
  onConnectTerminal,
  canManage = false
}: RunnersTableProps) {
  const { resolvedTheme } = useTheme()
  const rows = React.useMemo<Row[]>(() => runners.map((r) => ({ ...r, id: r.vm_id })), [runners])
  const showActions = Boolean(canManage && (onViewLogs || onConnectTerminal))

  const columns = React.useMemo(
    () => [
      {
        header: 'VM ID',
        field: 'vm_id',
        rowHeader: true,
        width: colWidth(14),
        renderCell: (row: Row) => (
          <div className='min-w-0'>
            <UIText weight='font-semibold' className='block truncate font-mono text-xs'>
              {row.vm_id}
            </UIText>
          </div>
        )
      },
      {
        header: 'Domain',
        field: 'domain',
        width: colWidth(16),
        renderCell: (row: Row) => <div className='min-w-0 truncate'>{row.domain || '—'}</div>
      },
      {
        header: 'Phase',
        field: 'phase',
        width: colWidth(12),
        renderCell: (row: Row) => phaseBadge(row.phase)
      },
      {
        header: 'VM IP',
        field: 'vm_ip',
        width: colWidth(12),
        renderCell: (row: Row) => <div className='font-mono text-xs'>{row.vm_ip || '—'}</div>
      },
      {
        header: 'Image',
        field: 'image_name',
        width: colWidth(18),
        renderCell: (row: Row) => {
          const digest = shortDigest(row.image_digest)
          const label = row.image_name || digest || '—'

          return (
            <div className='min-w-0 truncate' title={[row.image_name, row.image_digest].filter(Boolean).join(' ')}>
              {label}
              {row.image_name && digest ? (
                <span className='text-tertiary ml-1 font-mono text-[11px]'>({digest})</span>
              ) : null}
            </div>
          )
        }
      },
      {
        header: 'Resources',
        field: 'image_cpus',
        width: colWidth(14),
        renderCell: (row: Row) => {
          const parts = [
            row.image_cpus != null ? `${row.image_cpus} vCPU` : null,
            row.image_memory_mb != null ? `${Math.round(row.image_memory_mb / 1024)} GiB` : null,
            row.image_disk_gb != null ? `${row.image_disk_gb} GiB disk` : null
          ].filter(Boolean)

          return <div className='min-w-0 truncate text-xs'>{parts.length ? parts.join(' / ') : '—'}</div>
        }
      },
      {
        header: 'Uptime',
        field: 'uptime_secs',
        width: colWidth(10),
        renderCell: (row: Row) => <div className='text-xs'>{formatUptime(row.uptime_secs)}</div>
      },
      ...(showActions
        ? [
            {
              header: 'Actions',
              field: 'id',
              width: colWidth(14),
              renderCell: (row: Row) => (
                <div className='flex flex-wrap items-center gap-1'>
                  {onViewLogs ? (
                    <Button variant='plain' size='sm' onClick={() => onViewLogs(row)}>
                      Logs
                    </Button>
                  ) : null}
                  {onConnectTerminal && row.phase === 'running' ? (
                    <Button variant='plain' size='sm' onClick={() => onConnectTerminal(row)}>
                      Terminal
                    </Button>
                  ) : null}
                </div>
              )
            }
          ]
        : [])
    ],
    [onConnectTerminal, onViewLogs, showActions]
  )

  return (
    <div className='flex min-w-0 flex-col gap-2'>
      <div className='flex items-baseline justify-between gap-2'>
        <UIText weight='font-semibold' size='text-sm'>
          Runner VMs
        </UIText>
        <UIText size='text-xs' color='text-muted'>
          {isLoading ? 'Loading…' : `${runners.length} tracked`}
        </UIText>
      </div>
      {errorMessage ? (
        <UIText size='text-sm' className='text-red-600'>
          {errorMessage}
        </UIText>
      ) : null}
      <ThemeProvider colorMode={resolvedTheme === 'dark' ? 'dark' : 'light'}>
        {rows.length === 0 && !isLoading ? (
          <div className='rounded-md border border-dashed border-gray-300 px-4 py-6 text-center dark:border-gray-700'>
            <UIText size='text-sm' color='text-muted'>
              No runner VMs yet. Use Start Runner to provision one — it will stay listed here while the scheduler tracks
              it.
            </UIText>
          </div>
        ) : (
          <DataTable aria-labelledby='runner-vms-table' data={rows} columns={columns as any} />
        )}
      </ThemeProvider>
    </div>
  )
}
