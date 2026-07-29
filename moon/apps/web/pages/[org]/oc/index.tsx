'use client'

import React from 'react'
import { Pagination, ThemeProvider as PrimerThemeProvider } from '@primer/react'
import { useTheme } from 'next-themes'
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
import {
  domainFromClientHostname,
  OrionClient,
  OrionClientStatus,
  RunnersTable,
  VmTerminal
} from '@/components/OrionClient'
import AuthAppProviders from '@/components/Providers/AuthAppProviders'
import { useAdminCheck } from '@/hooks/admin/useAdminCheck'
import { usePostOrionClientsInfo } from '@/hooks/OrionClient/OrionClientsInfo'
import { useGetRunnerList } from '@/hooks/OrionClient/useGetRunnerList'
import { useGetRunnerStatus } from '@/hooks/OrionClient/useGetRunnerStatus'
import { usePostStartRunner } from '@/hooks/OrionClient/usePostStartRunner'
import { useRunnerLogsSSE } from '@/hooks/OrionClient/useRunnerLogsSSE'
import { PageWithLayout } from '@/utils/types'

function formatUptime(totalSecs: number): string {
  const secs = Math.max(0, Math.floor(totalSecs))
  const h = Math.floor(secs / 3600)
  const m = Math.floor((secs % 3600) / 60)
  const s = secs % 60

  if (h > 0) return `${h}h ${m}m ${s}s`
  if (m > 0) return `${m}m ${s}s`
  return `${s}s`
}

/** True when the log panel is only showing a VM wait placeholder (no real log lines yet). */
function isVmWaitingLog(logs: string): boolean {
  const trimmed = logs.trim()

  if (!trimmed) return false
  if (!/^Waiting for VM\b/im.test(trimmed)) return false
  // Still waiting if every non-empty line is a wait notice.
  return trimmed
    .split('\n')
    .map((l) => l.trim())
    .filter(Boolean)
    .every((l) => /^Waiting for VM\b/i.test(l))
}

function vmWaitingLabel(logs: string): string {
  const line = logs
    .split('\n')
    .map((l) => l.trim())
    .find((l) => /^Waiting for VM\b/i.test(l))

  return (line || 'Waiting for VM to become available').replace(/[.…]+$/u, '')
}

type LogPanelSource = 'runner' | 'client'
type TerminalPanelSource = 'runner' | 'client'

const OrionClientPage: PageWithLayout<any> = () => {
  const { resolvedTheme } = useTheme()
  const [statusFilter, setStatusFilter] = React.useState<OrionClientStatus | 'all'>('all')
  const [currentPage, setCurrentPage] = React.useState<number>(1)
  /** Stream key for scheduler logs: VM id (after Start Runner) or domain host (from client list). */
  const [activeLogKey, setActiveLogKey] = React.useState<string | null>(null)
  const [activePhase, setActivePhase] = React.useState<string | null>(null)
  const [activeDomain, setActiveDomain] = React.useState<string | null>(null)
  const [logSource, setLogSource] = React.useState<LogPanelSource | null>(null)
  const [logClientId, setLogClientId] = React.useState<string | null>(null)
  const [activeTerminalKey, setActiveTerminalKey] = React.useState<string | null>(null)
  const [terminalSource, setTerminalSource] = React.useState<TerminalPanelSource | null>(null)
  const [terminalClientId, setTerminalClientId] = React.useState<string | null>(null)
  const [terminalDomain, setTerminalDomain] = React.useState<string | null>(null)
  const [copyFeedback, setCopyFeedback] = React.useState(false)
  const logPanelRef = React.useRef<HTMLDivElement>(null)
  const terminalPanelRef = React.useRef<HTMLDivElement>(null)
  const logsScrollRef = React.useRef<HTMLDivElement>(null)
  const logsPreRef = React.useRef<HTMLPreElement>(null)
  const logsFollowRef = React.useRef(true)
  const runnerLogsRef = React.useRef('')

  const perPage = 50
  const showingLogs = Boolean(activeLogKey)
  const showingTerminal = Boolean(activeTerminalKey)
  const showingOverlay = showingLogs || showingTerminal

  const { data: adminCheck } = useAdminCheck()
  const isAdmin = adminCheck?.data?.is_admin || false

  const { mutate: startRunner, isPending: isStartingRunner } = usePostStartRunner()
  const {
    data: runnerList,
    isLoading: isLoadingRunners,
    error: runnerListError,
    refetch: refetchRunners
  } = useGetRunnerList(isAdmin)
  const runnerStatusVmId = logSource === 'runner' ? activeLogKey : null
  const { data: runnerStatus } = useGetRunnerStatus(runnerStatusVmId, activePhase)
  const { logs: runnerLogs, status: runnerLogsStatus, error: runnerLogsError } = useRunnerLogsSSE(activeLogKey)

  runnerLogsRef.current = runnerLogs

  const { mutate, isPending, error } = usePostOrionClientsInfo()
  const [clientsPage, setClientsPage] = React.useState<PostOrionClientsInfoData | null>(null)

  const copyLogsToClipboard = React.useCallback(async (text: string) => {
    if (!text) return false

    try {
      await navigator.clipboard.writeText(text)
      setCopyFeedback(true)
      window.setTimeout(() => setCopyFeedback(false), 1500)
      return true
    } catch {
      return false
    }
  }, [])

  React.useEffect(() => {
    if (!logsFollowRef.current) return

    const el = logsScrollRef.current

    if (!el) return

    // Defer until after the <pre> text paints, otherwise scrollHeight is stale.
    const id = window.requestAnimationFrame(() => {
      if (!logsFollowRef.current || !logsScrollRef.current) return
      logsScrollRef.current.scrollTop = logsScrollRef.current.scrollHeight
    })

    return () => window.cancelAnimationFrame(id)
  }, [runnerLogs])

  const requestPayload = React.useMemo<PageParamsOrionClientQuery>(() => {
    const additional: PageParamsOrionClientQuery['additional'] = {}

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
  }, [currentPage, perPage, statusFilter])

  const handleRefresh = React.useCallback(() => {
    if (showingOverlay) return

    if (isAdmin) {
      void refetchRunners()
    }

    mutate(requestPayload, {
      onSuccess: (data) => {
        setClientsPage(data)
      }
    })
  }, [isAdmin, mutate, refetchRunners, requestPayload, showingOverlay])

  React.useEffect(() => {
    if (!runnerStatus) return
    setActivePhase(runnerStatus.phase)
    if (runnerStatus.domain) {
      setActiveDomain(runnerStatus.domain)
    }
  }, [runnerStatus])

  // Do not refresh the client list while a panel is open.
  React.useEffect(() => {
    if (showingOverlay) return
    if (runnerStatus?.phase === 'running') {
      handleRefresh()
    }
  }, [runnerStatus?.phase, handleRefresh, showingOverlay])

  const clearTerminalPanel = React.useCallback(() => {
    setActiveTerminalKey(null)
    setTerminalSource(null)
    setTerminalClientId(null)
    setTerminalDomain(null)
  }, [])

  const clearLogPanel = React.useCallback(() => {
    setActiveLogKey(null)
    setLogSource(null)
    setActiveDomain(null)
    setLogClientId(null)
    setActivePhase(null)
  }, [])

  const openLogPanel = React.useCallback(
    (
      key: string,
      source: LogPanelSource,
      opts?: { domain?: string | null; clientId?: string | null; phase?: string | null }
    ) => {
      clearTerminalPanel()
      setActiveLogKey(key)
      setLogSource(source)
      setActiveDomain(opts?.domain ?? null)
      setLogClientId(opts?.clientId ?? null)
      setActivePhase(opts?.phase ?? null)
      logsFollowRef.current = true
      requestAnimationFrame(() => {
        logsScrollRef.current?.focus({ preventScroll: true })
        logPanelRef.current?.scrollIntoView({ behavior: 'smooth', block: 'start' })
      })
    },
    [clearTerminalPanel]
  )

  const openTerminalPanel = React.useCallback(
    (key: string, source: TerminalPanelSource, opts?: { domain?: string | null; clientId?: string | null }) => {
      clearLogPanel()
      setActiveTerminalKey(key)
      setTerminalSource(source)
      setTerminalDomain(opts?.domain ?? null)
      setTerminalClientId(opts?.clientId ?? null)
      requestAnimationFrame(() => {
        terminalPanelRef.current?.scrollIntoView({ behavior: 'smooth', block: 'start' })
      })
    },
    [clearLogPanel]
  )

  const handleCloseLogs = React.useCallback(() => {
    clearLogPanel()
  }, [clearLogPanel])

  const handleCloseTerminal = React.useCallback(() => {
    clearTerminalPanel()
  }, [clearTerminalPanel])

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

  const handleConnectTerminal = React.useCallback(
    (client: OrionClient) => {
      const domain = domainFromClientHostname(client.hostname)

      if (!domain) {
        return
      }

      openTerminalPanel(domain, 'client', { domain, clientId: client.client_id })
    },
    [openTerminalPanel]
  )

  const handleViewRunnerLogs = React.useCallback(
    (runner: { vm_id: string; domain?: string | null; phase?: string | null }) => {
      openLogPanel(runner.vm_id, 'runner', {
        domain: runner.domain ?? null,
        phase: runner.phase ?? null
      })
    },
    [openLogPanel]
  )

  const handleConnectRunnerTerminal = React.useCallback(
    (runner: { vm_id: string; domain?: string | null }) => {
      openTerminalPanel(runner.vm_id, 'runner', { domain: runner.domain ?? null })
    },
    [openTerminalPanel]
  )

  const handleLogsKeyDown = React.useCallback(
    async (e: React.KeyboardEvent<HTMLDivElement>) => {
      const meta = e.metaKey || e.ctrlKey

      if (!meta) return

      if (e.key === 'a' || e.key === 'A') {
        e.preventDefault()
        const pre = logsPreRef.current

        if (!pre) return
        const selection = window.getSelection()
        const range = document.createRange()

        range.selectNodeContents(pre)
        selection?.removeAllRanges()
        selection?.addRange(range)
        return
      }

      if (e.key === 'c' || e.key === 'C') {
        const selection = window.getSelection()?.toString() ?? ''
        const text = selection || runnerLogsRef.current

        if (!text) return
        e.preventDefault()
        await copyLogsToClipboard(text)
      }
    },
    [copyLogsToClipboard]
  )

  // Fetch client list only while panels are closed.
  React.useEffect(() => {
    if (showingOverlay) return

    mutate(requestPayload, {
      onSuccess: (data) => {
        setClientsPage(data)
      }
    })
  }, [mutate, requestPayload, showingOverlay])

  const total = clientsPage?.total ?? 0

  const pageCount = React.useMemo(() => {
    return Math.max(1, Math.ceil(total / perPage))
  }, [perPage, total])

  React.useEffect(() => {
    setCurrentPage(1)
  }, [statusFilter])

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

  const statusOptions = React.useMemo(
    () => [
      { value: 'all' as const, label: 'All clients' },
      { value: 'idle' as const, label: 'Idle' },
      { value: 'busy' as const, label: 'Busy' },
      { value: 'downloading' as const, label: 'Downloading' },
      { value: 'running' as const, label: 'Building' },
      { value: 'error' as const, label: 'Error' },
      { value: 'offline' as const, label: 'Offline' }
    ],
    []
  )

  return (
    <>
      <Head>
        <title>Orion Client</title>
      </Head>
      {/* AppLayout main is overflow-hidden; this page must own scrolling when the list is visible. */}
      <div
        className={`flex h-full min-h-0 flex-col gap-4 p-4 ${showingOverlay ? 'overflow-hidden' : 'overflow-y-auto'}`}
      >
        <div className='flex min-w-0 flex-col gap-2'>
          <div className='flex flex-wrap items-center justify-between gap-3'>
            <div>
              <h1 className='text-xl font-semibold'>Orion</h1>
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
              {!showingOverlay ? (
                <Button
                  variant='plain'
                  iconOnly={<RefreshIcon />}
                  accessibilityLabel='Refresh'
                  onClick={handleRefresh}
                  disabled={isPending || (isAdmin && isLoadingRunners)}
                  tooltip='Refresh'
                />
              ) : null}
            </div>
          </div>

          {showingTerminal && activeTerminalKey ? (
            <div
              ref={terminalPanelRef}
              className='min-w-0 rounded-md border border-gray-200 bg-gray-50 p-3 dark:border-gray-700 dark:bg-gray-900'
            >
              <div className='flex items-start justify-between gap-2'>
                <div className='min-w-0'>
                  <UIText weight='font-semibold' size='text-sm'>
                    {terminalSource === 'client' && terminalClientId
                      ? `Client ${terminalClientId}`
                      : `Runner ${activeTerminalKey}`}
                  </UIText>
                  <UIText size='text-xs' color='text-muted' className='mt-0.5 block'>
                    Terminal for {terminalDomain ?? activeTerminalKey}
                  </UIText>
                </div>
                <Button variant='plain' size='sm' onClick={handleCloseTerminal}>
                  Close
                </Button>
              </div>
              <div className='mt-3 min-w-0'>
                <VmTerminal key={activeTerminalKey} streamKey={activeTerminalKey} height={420} />
              </div>
            </div>
          ) : null}

          {showingLogs ? (
            <div
              ref={logPanelRef}
              className='min-w-0 rounded-md border border-gray-200 bg-gray-50 p-3 dark:border-gray-700 dark:bg-gray-900'
            >
              <div className='flex items-start justify-between gap-2'>
                <div className='min-w-0'>
                  <UIText weight='font-semibold' size='text-sm'>
                    {logSource === 'client' && logClientId ? `Client ${logClientId}` : `Runner ${activeLogKey}`}
                  </UIText>
                  {logSource === 'client' ? (
                    <UIText size='text-xs' color='text-muted' className='mt-0.5 block'>
                      Streaming scheduler logs for domain {activeDomain ?? activeLogKey}
                    </UIText>
                  ) : null}
                </div>
                <div className='flex items-center gap-2'>
                  {isAdmin && activeLogKey ? (
                    <Button
                      variant='plain'
                      size='sm'
                      onClick={() =>
                        openTerminalPanel(activeLogKey, logSource === 'client' ? 'client' : 'runner', {
                          domain: activeDomain,
                          clientId: logClientId
                        })
                      }
                    >
                      Open terminal
                    </Button>
                  ) : null}
                  <Button variant='plain' size='sm' onClick={handleCloseLogs}>
                    Close
                  </Button>
                </div>
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
                {runnerStatus?.image_name || runnerStatus?.image_digest ? (
                  <UIText size='text-sm' color='text-muted'>
                    Image: {runnerStatus.image_name ?? 'unknown'}
                    {runnerStatus.image_digest
                      ? ` (${runnerStatus.image_digest.replace(/^sha256:/, '').slice(0, 12)})`
                      : ''}
                  </UIText>
                ) : null}
                {runnerStatus?.image_built_at ? (
                  <UIText size='text-sm' color='text-muted'>
                    Built: {runnerStatus.image_built_at}
                  </UIText>
                ) : null}
                {runnerStatus?.image_cpus != null ||
                runnerStatus?.image_memory_mb != null ||
                runnerStatus?.image_disk_gb != null ? (
                  <UIText size='text-sm' color='text-muted'>
                    Resources:{' '}
                    {[
                      runnerStatus.image_cpus != null ? `${runnerStatus.image_cpus} vCPU` : null,
                      runnerStatus.image_memory_mb != null
                        ? `${Math.round(runnerStatus.image_memory_mb / 1024)} GiB RAM`
                        : null,
                      runnerStatus.image_disk_gb != null ? `${runnerStatus.image_disk_gb} GiB disk` : null
                    ]
                      .filter(Boolean)
                      .join(' / ')}
                  </UIText>
                ) : null}
                {runnerStatus?.toolchain_rust || runnerStatus?.toolchain_buck2 || runnerStatus?.toolchain_python ? (
                  <UIText size='text-sm' color='text-muted'>
                    Toolchains:{' '}
                    {[
                      runnerStatus.toolchain_rust ? `rust ${runnerStatus.toolchain_rust}` : null,
                      runnerStatus.toolchain_buck2 ? `buck2 ${runnerStatus.toolchain_buck2}` : null,
                      runnerStatus.toolchain_python ? `python ${runnerStatus.toolchain_python}` : null
                    ]
                      .filter(Boolean)
                      .join(' · ')}
                  </UIText>
                ) : null}
                {runnerStatus?.kernel ? (
                  <UIText size='text-sm' color='text-muted'>
                    Kernel: {runnerStatus.kernel}
                  </UIText>
                ) : null}
                {runnerStatus?.uptime_secs != null ? (
                  <UIText size='text-sm' color='text-muted'>
                    Uptime: {formatUptime(runnerStatus.uptime_secs)}
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
                  <div className='flex items-center gap-2'>
                    {runnerLogsStatus === 'connecting' || /Waiting for VM\b/i.test(runnerLogs) ? (
                      <span className='text-tertiary inline-flex items-center gap-1.5 text-xs'>
                        <span
                          className='border-tertiary inline-block size-3 animate-spin rounded-full border-2 border-t-transparent'
                          aria-hidden
                        />
                        Waiting…
                      </span>
                    ) : (
                      <UIText size='text-xs' color='text-muted'>
                        {runnerLogsStatus === 'streaming'
                          ? 'Live'
                          : runnerLogsStatus === 'error'
                            ? 'Disconnected'
                            : 'Idle'}
                      </UIText>
                    )}
                    {runnerLogs ? (
                      <Button
                        variant='plain'
                        size='sm'
                        onClick={() => {
                          void copyLogsToClipboard(runnerLogs)
                        }}
                      >
                        {copyFeedback ? 'Copied' : 'Copy'}
                      </Button>
                    ) : null}
                  </div>
                </div>
                {runnerLogsError ? (
                  <UIText size='text-sm' className='mb-1 text-red-600'>
                    {runnerLogsError}
                  </UIText>
                ) : null}
                <div
                  ref={logsScrollRef}
                  tabIndex={0}
                  role='log'
                  aria-label='Orion runner logs'
                  onKeyDown={handleLogsKeyDown}
                  onWheel={(e) => {
                    // Stop auto-follow as soon as the user scrolls up.
                    if (e.deltaY < 0) {
                      logsFollowRef.current = false
                    }
                  }}
                  onScroll={(e) => {
                    const el = e.currentTarget
                    const distanceFromBottom = el.scrollHeight - el.scrollTop - el.clientHeight

                    logsFollowRef.current = distanceFromBottom < 40
                  }}
                  style={{ height: 320, maxHeight: 320, overflowY: 'auto', overflowX: 'auto' }}
                  className='w-full cursor-text rounded border border-gray-200 bg-black/90 outline-hidden select-text focus:ring-2 focus:ring-blue-500/40 dark:border-gray-700'
                >
                  {isVmWaitingLog(runnerLogs) ? (
                    <div className='flex h-full min-h-[280px] flex-col items-center justify-center gap-3 p-6 text-green-100'>
                      <span
                        className='inline-block size-8 animate-spin rounded-full border-2 border-green-400/30 border-t-green-300'
                        aria-hidden
                      />
                      <p className='flex items-center gap-0 font-mono text-xs tracking-wide'>
                        <span>{vmWaitingLabel(runnerLogs)}</span>
                        <span className='inline-flex w-4 justify-start' aria-hidden>
                          <span className='animate-pulse'>…</span>
                        </span>
                      </p>
                    </div>
                  ) : (
                    <pre
                      ref={logsPreRef}
                      className='m-0 block w-full min-w-0 p-3 font-mono text-xs leading-5 break-words whitespace-pre-wrap text-green-100 select-text'
                    >
                      {runnerLogs ||
                        (runnerLogsStatus === 'connecting'
                          ? 'Waiting for log stream…'
                          : 'No log lines yet. Logs appear while the runner is running.')}
                    </pre>
                  )}
                </div>
                <UIText size='text-xs' color='text-muted' className='mt-1 block'>
                  Scroll inside the box to browse. ⌘/Ctrl+A select all, ⌘/Ctrl+C copy. Scroll to bottom to resume live
                  follow.
                </UIText>
              </div>
            </div>
          ) : null}

          {!showingOverlay ? <div className='border-b' /> : null}
        </div>

        {!showingOverlay ? (
          <>
            <RunnersTable
              runners={isAdmin ? (runnerList?.runners ?? []) : []}
              clients={clients}
              isLoading={isPending || (isAdmin && isLoadingRunners)}
              errorMessage={[runnerListError?.message, error?.message].filter(Boolean).join(' · ') || null}
              statusFilter={statusFilter}
              onStatusChange={(value: OrionClientStatus | 'all') => setStatusFilter(value)}
              statusOptions={statusOptions}
              canManage={isAdmin}
              onViewRunnerLogs={handleViewRunnerLogs}
              onConnectRunnerTerminal={handleConnectRunnerTerminal}
              onViewClientLogs={handleViewClientLogs}
              onConnectClientTerminal={handleConnectTerminal}
            />

            {pageCount > 1 ? (
              <div className='flex w-full justify-center pt-2'>
                <PrimerThemeProvider colorMode={resolvedTheme === 'dark' ? 'dark' : 'light'}>
                  <Pagination
                    pageCount={pageCount}
                    currentPage={currentPage}
                    showPages={{ narrow: false }}
                    onPageChange={(_e: any, page: number) => setCurrentPage(page)}
                  />
                </PrimerThemeProvider>
              </div>
            ) : null}
          </>
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
