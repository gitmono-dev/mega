'use client'

import React from 'react'
import { ThemeProvider } from '@primer/react'
import { DataTable } from '@primer/react/experimental'
import { useTheme } from 'next-themes'

import { ORION_API_URL } from '@gitmono/config'
import { CoreWorkerStatus, RunnerStatusResponse, TaskPhase } from '@gitmono/types/generated'
import { Button, Select, SelectTrigger, SelectValue, UIText } from '@gitmono/ui'

import { useGetOrionClientStatusById } from '@/hooks/OrionClient/OrionClientStatusById'

import { domainFromClientHostname, isLocalEnvironmentDomain, localOrionDomainFromUrl } from './domainFromHostname'
import { TruncatedText } from './TruncatedText'
import { deriveStatus, OrionClient, OrionClientStatus } from './types'

type EnvFilter = 'local' | 'other' | 'all'

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

  if (h > 0) return `${h}h ${m}m`
  if (m > 0) return `${m}m`
  return `${secs}s`
}

function formatDateTime(iso: string | null | undefined): string {
  if (!iso) return '—'
  const d = new Date(iso)

  if (Number.isNaN(d.getTime())) return iso
  return d.toLocaleString()
}

function formatRelative(iso: string) {
  const d = new Date(iso)
  const ts = d.getTime()

  if (Number.isNaN(ts)) return '—'

  const diffSec = Math.max(0, Math.floor((Date.now() - ts) / 1000))

  if (diffSec < 60) return `${diffSec}s ago`
  const diffMin = Math.floor(diffSec / 60)

  if (diffMin < 60) return `${diffMin}m ago`
  const diffHour = Math.floor(diffMin / 60)

  if (diffHour < 24) return `${diffHour}h ago`
  return `${Math.floor(diffHour / 24)}d ago`
}

function phaseLabel(phase: string | null | undefined): string {
  if (!phase) return '—'
  return phase
}

function phaseClass(phase: string | null | undefined): string {
  const p = (phase || '').toLowerCase()

  if (p === 'running') return 'text-green-700 dark:text-green-400'
  if (p === 'provisioning') return 'text-blue-700 dark:text-blue-400'
  if (p === 'failed') return 'text-red-600 dark:text-red-400'
  return 'text-tertiary'
}

export type FleetRow = {
  id: string
  domain: string | null
  runner: RunnerStatusResponse | null
  client: OrionClient | null
  isLocalEnv: boolean
}

function mergeFleetRows(
  runners: RunnerStatusResponse[],
  clients: OrionClient[],
  localOrionDomain: string | null
): FleetRow[] {
  const clientByDomain = new Map<string, OrionClient>()

  for (const client of clients) {
    const domain = domainFromClientHostname(client.hostname)

    if (!domain) continue
    const prev = clientByDomain.get(domain)

    if (!prev || new Date(client.last_heartbeat).getTime() > new Date(prev.last_heartbeat).getTime()) {
      clientByDomain.set(domain, client)
    }
  }

  const usedDomains = new Set<string>()
  const rows: FleetRow[] = runners.map((runner) => {
    const domain = runner.domain ?? null

    if (domain) usedDomains.add(domain)
    return {
      id: runner.vm_id,
      domain,
      runner,
      client: domain ? (clientByDomain.get(domain) ?? null) : null,
      isLocalEnv: isLocalEnvironmentDomain(domain, localOrionDomain)
    }
  })

  for (const client of clients) {
    const domain = domainFromClientHostname(client.hostname)

    if (domain && usedDomains.has(domain)) continue
    rows.push({
      id: `client:${client.client_id}`,
      domain,
      runner: null,
      client,
      isLocalEnv: isLocalEnvironmentDomain(domain, localOrionDomain)
    })
  }

  rows.sort((a, b) => {
    if (a.isLocalEnv !== b.isLocalEnv) return a.isLocalEnv ? -1 : 1
    const ar = a.runner ? phaseRank(a.runner.phase) : 9
    const br = b.runner ? phaseRank(b.runner.phase) : 9

    if (ar !== br) return ar - br
    return (a.domain ?? a.id).localeCompare(b.domain ?? b.id)
  })

  return rows
}

function phaseRank(phase: string): number {
  switch (phase) {
    case 'running':
      return 0
    case 'provisioning':
      return 1
    case 'failed':
      return 2
    default:
      return 3
  }
}

interface RunnersTableProps {
  runners: RunnerStatusResponse[]
  clients: OrionClient[]
  isLoading?: boolean
  errorMessage?: string | null
  statusFilter: OrionClientStatus | 'all'
  onStatusChange: (v: OrionClientStatus | 'all') => void
  statusOptions: { value: OrionClientStatus | 'all'; label: string }[]
  onViewRunnerLogs?: (runner: RunnerStatusResponse) => void
  onConnectRunnerTerminal?: (runner: RunnerStatusResponse) => void
  onViewClientLogs?: (client: OrionClient) => void
  onConnectClientTerminal?: (client: OrionClient) => void
  canManage?: boolean
}

export function RunnersTable({
  runners,
  clients,
  isLoading,
  errorMessage,
  statusFilter,
  onStatusChange,
  statusOptions,
  onViewRunnerLogs,
  onConnectRunnerTerminal,
  onViewClientLogs,
  onConnectClientTerminal,
  canManage = false
}: RunnersTableProps) {
  const { resolvedTheme } = useTheme()
  const [envFilter, setEnvFilter] = React.useState<EnvFilter>('local')
  const localOrionDomain = React.useMemo(() => localOrionDomainFromUrl(ORION_API_URL), [])
  const rows = React.useMemo(
    () => mergeFleetRows(runners, clients, localOrionDomain),
    [runners, clients, localOrionDomain]
  )
  const localCount = rows.filter((r) => r.isLocalEnv).length
  const otherCount = rows.length - localCount
  const visibleRows = React.useMemo(() => {
    if (envFilter === 'local') return rows.filter((r) => r.isLocalEnv)
    if (envFilter === 'other') return rows.filter((r) => !r.isLocalEnv)
    return rows
  }, [rows, envFilter])

  const showActions = Boolean(
    canManage && (onViewRunnerLogs || onConnectRunnerTerminal || onViewClientLogs || onConnectClientTerminal)
  )

  const columns = React.useMemo(
    () => [
      {
        header: 'Host',
        field: 'domain',
        rowHeader: true,
        width: colWidth(18),
        renderCell: (row: FleetRow) => {
          const host = row.domain || row.client?.hostname || '—'
          const secondary = row.runner?.vm_id
            ? row.runner.vm_ip
              ? `${row.runner.vm_id} · ${row.runner.vm_ip}`
              : row.runner.vm_id
            : row.client?.client_id || null

          return (
            <div className='min-w-0 py-0.5'>
              <div className='flex min-w-0 items-baseline gap-2'>
                <TruncatedText text={host} className='text-sm font-medium' />
                {!row.isLocalEnv ? <span className='text-tertiary shrink-0 text-[11px]'>other</span> : null}
              </div>
              {secondary ? <TruncatedText text={secondary} className='text-tertiary mt-0.5 text-[11px]' mono /> : null}
            </div>
          )
        }
      },
      {
        header: 'Status',
        field: 'runner',
        width: colWidth(12),
        renderCell: (row: FleetRow) => {
          if (row.runner) {
            return (
              <span className={`text-sm capitalize ${phaseClass(row.runner.phase)}`}>
                {phaseLabel(row.runner.phase)}
              </span>
            )
          }
          if (row.client) return <ClientStatusText client={row.client} />
          return <span className='text-tertiary text-sm'>—</span>
        }
      },
      {
        header: 'Image',
        field: 'runner',
        id: 'image',
        width: colWidth(18),
        renderCell: (row: FleetRow) => {
          if (!row.runner) return <span className='text-tertiary text-sm'>—</span>
          const digest = shortDigest(row.runner.image_digest)
          const label = row.runner.image_name || digest || '—'
          const full = [row.runner.image_name, row.runner.image_digest, row.runner.image_path]
            .filter(Boolean)
            .join('\n')
          const resources = [
            row.runner.image_cpus != null ? `${row.runner.image_cpus}c` : null,
            row.runner.image_memory_mb != null ? `${Math.round(row.runner.image_memory_mb / 1024)}G` : null,
            row.runner.image_disk_gb != null ? `${row.runner.image_disk_gb}G disk` : null
          ]
            .filter(Boolean)
            .join(' · ')

          return (
            <div className='min-w-0'>
              <TruncatedText text={label} popupText={full || label} className='text-sm' />
              {resources ? <div className='text-tertiary mt-0.5 text-[11px]'>{resources}</div> : null}
            </div>
          )
        }
      },
      {
        header: 'Client',
        field: 'client',
        width: colWidth(12),
        renderCell: (row: FleetRow) => {
          if (!row.client) return <span className='text-tertiary text-sm'>—</span>
          return (
            <div className='min-w-0'>
              <TruncatedText text={row.client.client_id} className='text-sm' />
              {row.client.orion_version ? (
                <div className='text-tertiary mt-0.5 text-[11px]'>{row.client.orion_version}</div>
              ) : null}
            </div>
          )
        }
      },
      {
        header: 'Start',
        field: 'client',
        id: 'start-time',
        width: colWidth(14),
        renderCell: (row: FleetRow) => {
          if (!row.client?.start_time) {
            if (row.runner?.uptime_secs != null) {
              return (
                <div className='min-w-0'>
                  <span className='text-tertiary text-sm'>—</span>
                  <div className='text-tertiary mt-0.5 text-[11px]'>up {formatUptime(row.runner.uptime_secs)}</div>
                </div>
              )
            }
            return <span className='text-tertiary text-sm'>—</span>
          }
          return (
            <div className='min-w-0'>
              <div className='text-sm leading-snug break-words'>{formatDateTime(row.client.start_time)}</div>
              {row.runner?.uptime_secs != null ? (
                <div className='text-tertiary mt-0.5 text-[11px]'>up {formatUptime(row.runner.uptime_secs)}</div>
              ) : null}
            </div>
          )
        }
      },
      {
        header: 'Heartbeat',
        field: 'client',
        id: 'heartbeat',
        width: colWidth(14),
        renderCell: (row: FleetRow) => {
          if (!row.client?.last_heartbeat) return <span className='text-tertiary text-sm'>—</span>
          return (
            <div className='min-w-0'>
              <div className='text-sm leading-snug break-words'>{formatDateTime(row.client.last_heartbeat)}</div>
              <div className='text-tertiary mt-0.5 text-[11px]'>{formatRelative(row.client.last_heartbeat)}</div>
            </div>
          )
        }
      },
      ...(showActions
        ? [
            {
              header: '',
              field: 'id',
              id: 'actions',
              width: colWidth(12),
              renderCell: (row: FleetRow) => (
                <div className='flex items-center justify-end gap-1'>
                  {row.runner && onViewRunnerLogs ? (
                    <Button variant='plain' size='sm' onClick={() => onViewRunnerLogs(row.runner!)}>
                      Logs
                    </Button>
                  ) : row.client && onViewClientLogs ? (
                    <Button variant='plain' size='sm' onClick={() => onViewClientLogs(row.client!)}>
                      Logs
                    </Button>
                  ) : null}
                  {row.runner?.phase === 'running' && onConnectRunnerTerminal ? (
                    <Button variant='plain' size='sm' onClick={() => onConnectRunnerTerminal(row.runner!)}>
                      Terminal
                    </Button>
                  ) : row.client && !row.runner && onConnectClientTerminal ? (
                    <Button variant='plain' size='sm' onClick={() => onConnectClientTerminal(row.client!)}>
                      Terminal
                    </Button>
                  ) : null}
                </div>
              )
            }
          ]
        : [])
    ],
    [onConnectClientTerminal, onConnectRunnerTerminal, onViewClientLogs, onViewRunnerLogs, showActions]
  )

  if (isLoading && rows.length === 0) {
    return (
      <div className='flex h-40 items-center justify-center'>
        <UIText tertiary>Loading…</UIText>
      </div>
    )
  }

  return (
    <div className='flex min-w-0 flex-col gap-3'>
      <div className='flex flex-wrap items-center justify-between gap-3'>
        <div className='flex flex-wrap items-center gap-1 rounded-md border border-gray-200 p-0.5 dark:border-gray-700'>
          {(
            [
              { id: 'local', label: 'This env', count: localCount },
              { id: 'other', label: 'Other', count: otherCount },
              { id: 'all', label: 'All', count: rows.length }
            ] as const
          ).map((tab) => {
            const active = envFilter === tab.id

            return (
              <button
                key={tab.id}
                type='button'
                onClick={() => setEnvFilter(tab.id)}
                className={[
                  'rounded px-2.5 py-1 text-xs transition-colors',
                  active
                    ? 'bg-gray-900 text-white dark:bg-gray-100 dark:text-gray-900'
                    : 'text-tertiary hover:text-primary'
                ].join(' ')}
              >
                {tab.label}
                <span className={active ? 'ml-1 opacity-70' : 'text-tertiary ml-1'}>{tab.count}</span>
              </button>
            )
          })}
        </div>

        <div className='flex flex-wrap items-center gap-3'>
          {localOrionDomain ? (
            <UIText size='text-xs' color='text-muted'>
              {localOrionDomain}
            </UIText>
          ) : null}
          <Select
            value={statusFilter}
            options={statusOptions}
            onChange={(v) => onStatusChange(v as OrionClientStatus | 'all')}
          >
            <SelectTrigger className='h-8 min-w-[140px] text-xs'>
              <SelectValue placeholder='Client status' />
            </SelectTrigger>
          </Select>
        </div>
      </div>

      {errorMessage ? (
        <UIText size='text-sm' className='text-red-600'>
          {errorMessage}
        </UIText>
      ) : null}

      <ThemeProvider colorMode={resolvedTheme === 'dark' ? 'dark' : 'light'}>
        <div
          className={[
            'border-primary overflow-hidden rounded-md border',
            '[&_table]:w-full',
            '[&_td]:min-w-0 [&_td]:py-2.5 [&_th]:min-w-0 [&_th]:py-2.5',
            '[&_td]:!border-x-0 [&_th]:!border-x-0 [&_th]:!border-t-0',
            '[&_tbody_tr]:border-b [&_tbody_tr:last-child]:border-b-0 [&_thead_tr]:border-b'
          ].join(' ')}
        >
          {visibleRows.length === 0 ? (
            <div className='flex h-32 items-center justify-center px-4 text-center'>
              <UIText tertiary size='text-sm'>
                {envFilter === 'local'
                  ? localOrionDomain
                    ? `No runners or clients for ${localOrionDomain}`
                    : 'No runners or clients in this environment'
                  : envFilter === 'other'
                    ? 'No runners or clients from other environments'
                    : 'No runners or clients'}
              </UIText>
            </div>
          ) : (
            <DataTable aria-label='Orion runners and clients' data={visibleRows} columns={columns as any} />
          )}
        </div>
      </ThemeProvider>
    </div>
  )
}

function ClientStatusText({ client }: { client: OrionClient }) {
  const { data, isLoading, isError } = useGetOrionClientStatusById(client.client_id, undefined, 5 * 60 * 1000)

  if (isLoading) {
    return <span className='text-tertiary text-sm'>…</span>
  }

  const status = isError || !data ? deriveStatus(client) : mapApiStatusToUiStatus(data.core_status, data.phase)

  const label =
    status === 'downloading'
      ? 'Downloading'
      : status === 'running'
        ? 'Building'
        : status === 'offline'
          ? 'Offline'
          : status.charAt(0).toUpperCase() + status.slice(1)

  return <span className='text-tertiary text-sm'>{label}</span>
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
