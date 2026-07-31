import { memo, useMemo, useState } from 'react'
import { CheckIcon, ChevronDownIcon, ClockIcon, FileDirectoryIcon, SyncIcon, XIcon } from '@primer/octicons-react'
import { format } from 'date-fns'

import { StatusProjectRelativePath } from '@gitmono/types/generated'
import { LoadingSpinner } from '@gitmono/ui/Spinner'

import { usePostRetryBuild } from '@/hooks/SSE/usePostRetryBuild'

import { TERMINAL_BUILD_STATUSES } from '../hooks/logUtils'
import { identifyStatus } from './identifyStatus'
import { BuildDTO, getLatestBuildId, isTaskQueued, Status, TaskInfoDTO } from './store'

/**
 * Format ISO date string to readable format
 * @param isoDate - ISO 8601 date string (e.g., "2025-11-03T13:16:36.477167+00:00")
 * @returns Formatted date string (e.g., "2025-11-04 11:22")
 */
const formatDateTime = (isoDate: string): string => {
  try {
    return format(new Date(isoDate), 'yyyy-MM-dd HH:mm')
  } catch {
    return isoDate
  }
}

export interface TreeRootProps {
  path?: string
  prName?: string
  tasks: TaskInfoDTO[]
  logsAvailableIds: Set<string>
  selectedBuildId: string
  onSelectBuild: (buildId: string, taskId?: string) => void
  totalTasksCount?: number
  cl: string
}

/**
 * Tree Root Component - Top level node showing the path
 */
export const TreeRoot = ({
  path,
  prName,
  tasks,
  logsAvailableIds,
  selectedBuildId,
  onSelectBuild,
  totalTasksCount,
  cl
}: TreeRootProps) => {
  const [isExpanded, setIsExpanded] = useState(true)

  // Show the total number of tasks in dropdown (displayed as "builds")
  const displayCount = totalTasksCount ?? tasks.length

  return (
    <div className='select-none'>
      <div
        onClick={() => setIsExpanded(!isExpanded)}
        className='group flex w-full cursor-pointer items-center gap-2.5 border-b border-gray-200 bg-gradient-to-r from-blue-50 to-transparent px-3 py-3 transition-all hover:from-blue-100 hover:to-blue-50 dark:border-gray-700 dark:from-blue-900/20 dark:hover:from-blue-900/30 dark:hover:to-blue-900/10'
      >
        <div className='flex items-center gap-2'>
          <div
            className='transition-transform duration-200'
            style={{ transform: isExpanded ? 'rotate(0deg)' : 'rotate(-90deg)' }}
          >
            <ChevronDownIcon size={16} className='text-gray-600 dark:text-gray-400' />
          </div>
          <FileDirectoryIcon size={16} className='text-blue-600 dark:text-blue-400' />
        </div>
        <span className='text-sm font-semibold text-gray-800 dark:text-gray-200'>{path || 'Project Root'}</span>
        <span className='ml-auto rounded-full bg-blue-100 px-2 py-0.5 text-xs font-medium text-blue-700 dark:bg-blue-900/40 dark:text-blue-300'>
          {displayCount} {displayCount === 1 ? 'build' : 'builds'}
        </span>
      </div>
      {isExpanded && (
        <div className='bg-gray-50/50 dark:bg-gray-900/20'>
          {tasks.map((t, index) => (
            <Task
              key={t.task_id}
              list={t}
              prName={prName}
              logsAvailableIds={logsAvailableIds}
              selectedBuildId={selectedBuildId}
              onSelectBuild={onSelectBuild}
              isLast={index === tasks.length - 1}
              cl={cl}
            />
          ))}
        </div>
      )}
    </div>
  )
}

/**
 * Task Component - Second level showing task name and builds
 */
export const Task = ({
  list,
  prName,
  logsAvailableIds,
  selectedBuildId,
  onSelectBuild,
  isLast,
  cl
}: {
  list: TaskInfoDTO
  prName?: string
  logsAvailableIds: Set<string>
  selectedBuildId: string
  onSelectBuild: (buildId: string, taskId?: string) => void
  isLast?: boolean
  cl: string
}) => {
  const [isExpanded, setIsExpanded] = useState(true)

  // Prefer the PR name as the task label; fall back to the build target path or
  // repo name so the row never shows an opaque uuid/log-file name.
  const getTaskLabel = () => {
    const fromPr = prName?.trim()

    if (fromPr) return fromPr

    const targetPath = list.targets?.[0]?.target_path?.trim()

    if (targetPath) return targetPath

    return list.task_name || 'Unnamed Task'
  }

  // Get overall task status from targets with styled badge
  const getTaskStatus = () => {
    if (!list.targets || list.targets.length === 0) return null

    const states = list.targets.map((t) => t.state)

    // If any target failed, show failed
    if (states.some((s) => s === 'Failed')) {
      return {
        status: 'Failed',
        icon: <XIcon size={14} className='text-red-600 dark:text-red-400' />,
        badgeClass: 'bg-red-100 text-red-700 dark:bg-red-900/40 dark:text-red-300'
      }
    }

    // If any target interrupted, show interrupted
    if (states.some((s) => s === 'Interrupted')) {
      return {
        status: 'Interrupted',
        icon: <XIcon size={14} className='text-orange-600 dark:text-orange-400' />,
        badgeClass: 'bg-orange-100 text-orange-700 dark:bg-orange-900/40 dark:text-orange-300'
      }
    }

    // If any target is building, show building
    if (states.some((s) => s === 'Building')) {
      return {
        status: 'Building',
        icon: <LoadingSpinner />,
        badgeClass: 'bg-blue-100 text-blue-700 dark:bg-blue-900/40 dark:text-blue-300'
      }
    }

    // If any target is pending, show pending
    if (states.some((s) => s === 'Pending')) {
      return {
        status: 'Pending',
        icon: <LoadingSpinner />,
        badgeClass: 'bg-yellow-100 text-yellow-700 dark:bg-yellow-900/40 dark:text-yellow-300'
      }
    }

    // If any target is uninitialized, show waiting state
    if (states.some((s) => s === 'Uninitialized')) {
      return {
        status: 'Uninitialized',
        icon: <LoadingSpinner />,
        badgeClass: 'bg-gray-100 text-gray-700 dark:bg-gray-700/60 dark:text-gray-300'
      }
    }

    // If all completed, show completed
    if (states.every((s) => s === 'Completed')) {
      return {
        status: 'Completed',
        icon: <CheckIcon size={14} className='text-green-700 dark:text-green-400' />,
        badgeClass: 'bg-green-100 text-green-700 dark:bg-green-900/40 dark:text-green-300'
      }
    }

    return null
  }

  const taskStatus = getTaskStatus()
  const taskLabel = getTaskLabel()

  // Show builds oldest-first so the sequence numbers (#1, #2, ...) read as the
  // chronological attempt order rather than opaque build ids.
  const orderedBuilds = useMemo(
    () => [...(list.build_list ?? [])].sort((a, b) => new Date(a.start_at).getTime() - new Date(b.start_at).getTime()),
    [list.build_list]
  )

  return (
    <div className={`${!isLast ? 'border-b border-gray-200 dark:border-gray-700' : ''}`}>
      <div
        onClick={() => setIsExpanded(!isExpanded)}
        className='group flex w-full cursor-pointer items-start gap-2 bg-white px-3 py-2.5 transition-all hover:bg-gray-100 dark:bg-gray-800/50 dark:hover:bg-gray-700/50'
      >
        <div
          className='mt-0.5 shrink-0 transition-transform duration-200'
          style={{ transform: isExpanded ? 'rotate(0deg)' : 'rotate(-90deg)' }}
        >
          <ChevronDownIcon size={14} className='text-gray-500 dark:text-gray-400' />
        </div>

        <div className='flex min-w-0 flex-1 flex-col gap-1'>
          <span title={taskLabel} className='truncate text-sm font-medium text-gray-800 dark:text-gray-200'>
            {taskLabel}
          </span>
          <div className='flex flex-wrap items-center gap-x-2 gap-y-1'>
            {taskStatus && (
              <span
                className={`inline-flex items-center gap-1 rounded-md px-1.5 py-0.5 text-xs font-medium ${taskStatus.badgeClass}`}
              >
                <span className='flex h-3.5 w-3.5 items-center justify-center'>{taskStatus.icon}</span>
                {taskStatus.status}
              </span>
            )}
            <span className='text-xs text-gray-500 dark:text-gray-400'>{formatDateTime(list.created_at)}</span>
          </div>
        </div>
      </div>

      {isExpanded && list && (
        <div className='bg-gray-50 dark:bg-gray-900/30'>
          {orderedBuilds.map((i, index) => (
            <TaskItem
              key={i.id}
              build={i}
              seq={index + 1}
              hasLogs={logsAvailableIds.has(i.id)}
              isSelected={selectedBuildId === i.id}
              onSelectBuild={(buildId) => onSelectBuild(buildId, list.task_id)}
              isLast={index === orderedBuilds.length - 1}
              cl={cl}
              clId={list.cl_id}
              changes={list.changes}
              isQueued={isTaskQueued(list)}
              isLatestBuild={i.id === getLatestBuildId(list)}
            />
          ))}
        </div>
      )}
    </div>
  )
}

/**
 * TaskItem Component - Build item showing individual builds
 */
const TaskItem = memo(function TaskItem({
  build,
  seq,
  hasLogs,
  isSelected,
  onSelectBuild,
  isLast,
  cl,
  clId,
  changes,
  isQueued,
  isLatestBuild
}: {
  build: BuildDTO
  seq?: number
  hasLogs?: boolean
  isSelected: boolean
  onSelectBuild: (buildId: string) => void
  isLast?: boolean
  cl: string
  clId?: number
  changes?: StatusProjectRelativePath[]
  isQueued?: boolean
  isLatestBuild?: boolean
}) {
  const { mutate: retryBuild, isPending: isRetrying } = usePostRetryBuild(cl)

  const showQueued = Boolean(isQueued) && build.status === 'Building'

  const handleClick = () => {
    onSelectBuild(build.id)
  }

  const handleRetry = (e: React.MouseEvent) => {
    e.stopPropagation()
    if (isRetrying) return
    retryBuild({
      build_id: build.id,
      cl_link: cl,
      cl_id: clId ?? 0,
      changes: changes ?? [],
      targets: []
    })
  }

  const canRetry = Boolean(isLatestBuild) && TERMINAL_BUILD_STATUSES.has(build.status)
  const isHighlighted = hasLogs

  let bgClass = 'bg-white dark:bg-gray-800/30'
  let textColor = 'text-gray-600 dark:text-gray-400'
  let borderColor = 'border-gray-300 dark:border-gray-600'

  if (isSelected) {
    bgClass = 'bg-blue-50 dark:bg-blue-900/20'
    textColor = 'text-blue-700 dark:text-blue-300'
    borderColor = 'border-blue-400 dark:border-blue-500'
  } else if (isHighlighted) {
    textColor = 'text-blue-600 dark:text-blue-400'
  }

  return (
    <div
      onClick={handleClick}
      className={`group flex cursor-pointer items-center gap-2 py-2 pr-3 pl-9 transition-all hover:bg-gray-100 dark:hover:bg-gray-700/30 ${bgClass} ${
        isSelected ? `border-l-2 ${borderColor}` : 'border-l-2 border-transparent'
      } ${!isLast ? 'border-b border-gray-100 dark:border-gray-800' : ''}`}
    >
      <span className='flex h-4 w-4 shrink-0 items-center justify-center'>
        {showQueued ? (
          <ClockIcon size={14} className='text-gray-500 dark:text-gray-400' />
        ) : (
          identifyStatus(build.status || Status.NotFound)
        )}
      </span>

      <span
        title={build.id}
        className={`shrink-0 font-mono text-sm transition-colors ${textColor} group-hover:text-blue-600 dark:group-hover:text-blue-400`}
      >
        {seq != null ? `#${seq}` : build.id}
      </span>

      {build.start_at && (
        <span className='min-w-0 flex-1 truncate font-mono text-xs text-gray-400 dark:text-gray-500'>
          {formatDateTime(build.start_at)}
        </span>
      )}

      {showQueued && (
        <span className='shrink-0 rounded-md bg-gray-100 px-1.5 py-0.5 text-[10px] font-medium text-gray-600 dark:bg-gray-700/60 dark:text-gray-300'>
          Queued
        </span>
      )}

      {canRetry && (
        <button
          type='button'
          onClick={handleRetry}
          disabled={isRetrying}
          title='Retry build'
          className='flex shrink-0 items-center gap-1 rounded-md border border-gray-300 bg-white px-1.5 py-0.5 text-xs font-medium text-gray-600 transition-all hover:border-blue-400 hover:text-blue-600 disabled:cursor-not-allowed disabled:opacity-60 dark:border-gray-600 dark:bg-gray-800 dark:text-gray-300 dark:hover:border-blue-500 dark:hover:text-blue-400'
        >
          <SyncIcon size={12} className={isRetrying ? 'animate-spin' : ''} />
          <span>{isRetrying ? 'Retrying' : 'Retry'}</span>
        </button>
      )}

      {isSelected && <div className='h-1.5 w-1.5 shrink-0 animate-pulse rounded-full bg-blue-500' />}
    </div>
  )
})

export { identifyStatus } from './identifyStatus'
