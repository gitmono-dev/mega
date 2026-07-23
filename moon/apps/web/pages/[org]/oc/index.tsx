'use client'

import React from 'react'
import { Pagination } from '@primer/react'
import Head from 'next/head'

import {
  CoreWorkerStatus,
  PageParamsOrionClientQuery,
  PostOrionClientsInfoData,
  TaskPhase
} from '@gitmono/types/generated'
import { Button, UIText } from '@gitmono/ui'
import { RefreshIcon } from '@gitmono/ui/Icons'

import { AppLayout } from '@/components/Layout/AppLayout'
import { ClientsTable, OrionClient, OrionClientStatus } from '@/components/OrionClient'
import AuthAppProviders from '@/components/Providers/AuthAppProviders'
import { useAdminCheck } from '@/hooks/admin/useAdminCheck'
import { usePostOrionClientsInfo } from '@/hooks/OrionClient/OrionClientsInfo'
import { useGetRunnerStatus } from '@/hooks/OrionClient/useGetRunnerStatus'
import { usePostStartRunner } from '@/hooks/OrionClient/usePostStartRunner'
import { useRunnerLogsSSE } from '@/hooks/OrionClient/useRunnerLogsSSE'
import { PageWithLayout } from '@/utils/types'

/** Client `hostname` is the WS URL (e.g. wss://orion.example/ws); scheduler keys VMs by that host. */
function domainFromClientHostname(hostname: string): string | null {
  const raw = hostname.trim()

  if (!raw) return null

  try {
    const url = new URL(raw.includes('://') ? raw : `ws://${raw}`)

    return url.hostname || null
  } catch {
    const host = raw.split('/')[0]?.split(':')[0]

    return host || null
  }
}

type LogPanelSource = 'runner' | 'client'

const OrionClientPage: PageWithLayout<any> = () => {
  const [hostnameInput, setHostnameInput] = React.useState<string>('')
  const [debouncedHostname, setDebouncedHostname] = React.useState<string>('')
  const [statusFilter, setStatusFilter] = React.useState<OrionClientStatus | 'all'>('all')
  const [currentPage, setCurrentPage] = React.useState<number>(1)
  /** Stream key for scheduler logs: VM id (after Start Runner) or domain host (from client list). */
  const [activeLogKey, setActiveLogKey] = React.useState<string | null>(null)
  const [activePhase, setActivePhase] = React.useState<string | null>(null)
  const [activeDomain, setActiveDomain] = React.useState<string | null>(null)
  const [logSource, setLogSource] = React.useState<LogPanelSource | null>(null)
  const [logClientId, setLogClientId] = React.useState<string | null>(null)
  const logPanelRef = React.useRef<HTMLDivElement>(null)

  const perPage = 8

  const { data: adminCheck } = useAdminCheck()
  const isAdmin = adminCheck?.data?.is_admin || false

  const { mutate: startRunner, isPending: isStartingRunner } = usePostStartRunner()
  const runnerStatusVmId = logSource === 'runner' ? activeLogKey : null
  const { data: runnerStatus } = useGetRunnerStatus(runnerStatusVmId, activePhase)
  const { logs: runnerLogs, status: runnerLogsStatus, error: runnerLogsError } = useRunnerLogsSSE(activeLogKey)
  const logsPreRef = React.useRef<HTMLPreElement>(null)
  const logsFollowRef = React.useRef(true)

  const { mutate, isPending, error } = usePostOrionClientsInfo()
  const [clientsPage, setClientsPage] = React.useState<PostOrionClientsInfoData | null>(null)

  React.useEffect(() => {
    const el = logsPreRef.current

    if (!el || !logsFollowRef.current) return
    el.scrollTop = el.scrollHeight
  }, [runnerLogs])

  React.useEffect(() => {
    const handle = setTimeout(() => {
      setDebouncedHostname(hostnameInput)
    }, 500)

    return () => clearTimeout(handle)
  }, [hostnameInput])

  const requestPayload = React.useMemo<PageParamsOrionClientQuery>(() => {
    const text = debouncedHostname.trim()
    const additional: PageParamsOrionClientQuery['additional'] = {}

    if (text !== '') {
      additional.hostname = text
    }

    if (statusFilter === 'idle') {
      additional.status = CoreWorkerStatus.Idle
    } else if (statusFilter === 'error') {
      additional.status = CoreWorkerStatus.Error
    } else if (statusFilter === 'offline') {
      additional.status = CoreWorkerStatus.Lost
    } else if (statusFilter === 'busy') {
      additional.status = CoreWorkerStatus.Busy
    } else if (statusFilter === 'downloading') {
      additional.status = CoreWorkerStatus.Busy
      additional.phase = TaskPhase.DownloadingSource
    } else if (statusFilter === 'running') {
      additional.status = CoreWorkerStatus.Busy
      additional.phase = TaskPhase.RunningBuild
    }

    return {
      pagination: { page: currentPage, per_page: perPage },
      additional
    }
  }, [currentPage, debouncedHostname, perPage, statusFilter])

  const handleRefresh = React.useCallback(() => {
    mutate(requestPayload, {
      onSuccess: (data) => {
        setClientsPage(data)
      }
    })
  }, [mutate, requestPayload])

  React.useEffect(() => {
    if (!runnerStatus) return
    setActivePhase(runnerStatus.phase)
    if (runnerStatus.domain) {
      setActiveDomain(runnerStatus.domain)
    }
  }, [runnerStatus])

  React.useEffect(() => {
    if (runnerStatus?.phase === 'running') {
      handleRefresh()
    }
  }, [runnerStatus?.phase, handleRefresh])

  const openLogPanel = React.useCallback((key: string, source: LogPanelSource, opts?: { domain?: string | null; clientId?: string | null; phase?: string | null }) => {
    setActiveLogKey(key)
    setLogSource(source)
    setActiveDomain(opts?.domain ?? null)
    setLogClientId(opts?.clientId ?? null)
    setActivePhase(opts?.phase ?? null)
    logsFollowRef.current = true
    requestAnimationFrame(() => {
      logPanelRef.current?.scrollIntoView({ behavior: 'smooth', block: 'start' })
    })
  }, [])

  const handleCloseLogs = React.useCallback(() => {
    setActiveLogKey(null)
    setLogSource(null)
    setActiveDomain(null)
    setLogClientId(null)
    setActivePhase(null)
  }, [])

  const handleStartRunner = React.useCallback(
    (replace = false) => {
      startRunner(
        { replace },
        {
          onSuccess: (data) => {
            openLogPanel(data.vm_id, 'runner', {
              domain: data.domain ?? null,
              phase: data.phase
            })
          }
        }
      )
    },
    [openLogPanel, startRunner]
  )

  const handleViewClientLogs = React.useCallback(
    (client: OrionClient) => {
      const domain = domainFromClientHostname(client.hostname)

      if (!domain) {
        return
      }

      openLogPanel(domain, 'client', { domain, clientId: client.client_id })
    },
    [openLogPanel]
  )

  React.useEffect(() => {
    mutate(requestPayload, {
      onSuccess: (data) => {
        setClientsPage(data)
      }
    })
  }, [mutate, requestPayload])

  const total = clientsPage?.total ?? 0

  const pageCount = React.useMemo(() => {
    return Math.max(1, Math.ceil(total / perPage))
  }, [perPage, total])

  React.useEffect(() => {
    setCurrentPage(1)
  }, [hostnameInput, statusFilter])

  React.useEffect(() => {
    setCurrentPage((p) => Math.min(Math.max(1, p), pageCount))
  }, [pageCount])

  const clients = React.useMemo(() => {
    const items = clientsPage?.items ?? []

    return items.map((c) => ({
      client_id: c.client_id,
      hostname: c.hostname,
      orion_version: c.orion_version,
      start_time: c.start_time,
      last_heartbeat: c.last_heartbeat
    }))
  }, [clientsPage])

  return (
    <>
      <Head>
        <title>Orion Client</title>
      </Head>
      {/* AppLayout main is overflow-hidden; this page must own scrolling. */}
      <div className='flex h-full min-h-0 flex-col gap-4 overflow-y-auto p-4'>
        <div className='flex min-w-0 flex-col gap-2'>
          <div className='flex flex-wrap items-center justify-between gap-3'>
            <div>
              <h1 className='text-xl font-semibold'>Orion Clients</h1>
              <UIText color='text-muted' size='text-sm'>
                Total clients {total}
              </UIText>
            </div>
            <div className='flex flex-wrap items-center gap-2'>
              {isAdmin ? (
                <Button
                  variant='primary'
                  onClick={() => handleStartRunner(true)}
                  disabled={isStartingRunner || activePhase === 'provisioning'}
                >
                  {isStartingRunner ? 'Starting…' : 'Start Runner'}
                </Button>
              ) : null}
              <Button
                variant='plain'
                iconOnly={<RefreshIcon />}
                accessibilityLabel='Refresh'
                onClick={handleRefresh}
                disabled={isPending}
                tooltip='Refresh'
              />
            </div>
          </div>

          {activeLogKey ? (
            <div
              ref={logPanelRef}
              className='min-w-0 overflow-hidden rounded-md border border-gray-200 bg-gray-50 p-3 dark:border-gray-700 dark:bg-gray-900'
            >
              <div className='flex items-start justify-between gap-2'>
                <div className='min-w-0'>
                  <UIText weight='font-semibold' size='text-sm'>
                    {logSource === 'client' && logClientId
                      ? `Client ${logClientId}`
                      : `Runner ${activeLogKey}`}
                  </UIText>
                  {logSource === 'client' ? (
                    <UIText size='text-xs' color='text-muted' className='mt-0.5 block'>
                      Streaming scheduler logs for domain {activeDomain ?? activeLogKey}
                    </UIText>
                  ) : null}
                </div>
                <Button variant='plain' size='sm' onClick={handleCloseLogs}>
                  Close
                </Button>
              </div>
              <div className='mt-1 flex flex-col gap-1'>
                {(runnerStatus?.domain ?? activeDomain) ? (
                  <UIText size='text-sm' color='text-muted'>
                    Domain: {runnerStatus?.domain ?? activeDomain}
                  </UIText>
                ) : null}
                {logSource === 'runner' ? (
                  <UIText size='text-sm'>
                    Phase:{' '}
                    <span className='font-medium capitalize'>{runnerStatus?.phase ?? activePhase ?? 'unknown'}</span>
                  </UIText>
                ) : null}
                {runnerStatus?.vm_ip ? (
                  <UIText size='text-sm' color='text-muted'>
                    VM IP: {runnerStatus.vm_ip}
                  </UIText>
                ) : null}
                {runnerStatus?.log_file ? (
                  <UIText size='text-sm' color='text-muted'>
                    Log file: {runnerStatus.log_file}
                  </UIText>
                ) : null}
                {runnerStatus?.error ? (
                  <UIText size='text-sm' className='text-red-600'>
                    {runnerStatus.error}
                  </UIText>
                ) : null}
                {logSource === 'runner' && runnerStatus?.phase === 'failed' ? (
                  <Button variant='primary' size='sm' className='mt-1 w-fit' onClick={() => handleStartRunner(true)}>
                    Retry
                  </Button>
                ) : null}
              </div>

              <div className='mt-3 min-w-0'>
                <div className='mb-1 flex items-center justify-between gap-2'>
                  <UIText weight='font-semibold' size='text-sm'>
                    {logSource === 'client' ? 'Runner logs' : 'Startup logs'}
                  </UIText>
                  <UIText size='text-xs' color='text-muted'>
                    {runnerLogsStatus === 'connecting'
                      ? 'Connecting…'
                      : runnerLogsStatus === 'streaming'
                        ? 'Live'
                        : runnerLogsStatus === 'error'
                          ? 'Disconnected'
                          : 'Idle'}
                  </UIText>
                </div>
                {runnerLogsError ? (
                  <UIText size='text-sm' className='mb-1 text-red-600'>
                    {runnerLogsError}
                  </UIText>
                ) : null}
                <pre
                  ref={logsPreRef}
                  onScroll={(e) => {
                    const el = e.currentTarget
                    const distanceFromBottom = el.scrollHeight - el.scrollTop - el.clientHeight

                    logsFollowRef.current = distanceFromBottom < 48
                  }}
                  className='h-80 max-h-80 w-full min-w-0 overflow-auto overscroll-contain whitespace-pre-wrap break-words rounded border border-gray-200 bg-black/90 p-3 font-mono text-xs leading-5 text-green-100 dark:border-gray-700'
                >
                  {runnerLogs ||
                    (runnerLogsStatus === 'connecting'
                      ? 'Waiting for log stream…'
                      : 'No log lines yet. Logs appear while the runner is running.')}
                </pre>
              </div>
            </div>
          ) : null}

          <div className='border-b' />
        </div>

        <div className='group flex min-h-[35px] items-center rounded-md border border-gray-300 bg-white px-3 shadow-sm transition-all focus-within:border-blue-500 focus-within:shadow-md focus-within:ring-2 focus-within:ring-blue-100 hover:border-gray-400 dark:border-gray-700 dark:bg-gray-900 dark:hover:border-gray-500'>
          <div className='flex items-center text-gray-400'>
            <svg
              xmlns='http://www.w3.org/2000/svg'
              className='h-4 w-4'
              fill='none'
              viewBox='0 0 24 24'
              stroke='currentColor'
            >
              <path
                strokeLinecap='round'
                strokeLinejoin='round'
                strokeWidth='2'
                d='M21 21l-4.35-4.35M11 19a8 8 0 100-16 8 8 0 000 16z'
              />
            </svg>
          </div>
          <input
            type='text'
            value={hostnameInput}
            onChange={(e) => setHostnameInput(e.target.value)}
            placeholder='Search by Hostname'
            className='w-full flex-1 border-none bg-transparent text-sm text-gray-700 outline-none ring-0 placeholder:text-gray-400 focus:outline-none focus:ring-0 dark:text-gray-100 dark:placeholder:text-gray-500'
          />
        </div>

        <ClientsTable
          clients={clients}
          isLoading={isPending}
          statusFilter={statusFilter}
          onStatusChange={(value: OrionClientStatus | 'all') => setStatusFilter(value)}
          canViewLogs={isAdmin}
          onViewLogs={handleViewClientLogs}
          statusOptions={[
            { value: 'all', label: 'All statuses' },
            { value: 'idle', label: 'Idle' },
            { value: 'busy', label: 'Busy' },
            { value: 'downloading', label: '\u00A0\u00A0Downloading source' },
            { value: 'running', label: '\u00A0\u00A0Running build' },
            { value: 'error', label: 'Error' },
            { value: 'offline', label: 'Lost / Offline' }
          ]}
        />

        {error ? (
          <UIText color='text-muted' size='text-sm'>
            Failed to load Orion clients: {error.message}
          </UIText>
        ) : null}

        {pageCount > 1 ? (
          <div className='flex w-full justify-center pt-2'>
            <Pagination
              pageCount={pageCount}
              currentPage={currentPage}
              showPages={{ narrow: false }}
              onPageChange={(_e: any, page: number) => setCurrentPage(page)}
            />
          </div>
        ) : null}
      </div>
    </>
  )
}

OrionClientPage.getProviders = (page: React.ReactElement, pageProps: any) => {
  return (
    <AuthAppProviders {...pageProps}>
      <AppLayout {...pageProps}>{page}</AppLayout>
    </AuthAppProviders>
  )
}

export default OrionClientPage
