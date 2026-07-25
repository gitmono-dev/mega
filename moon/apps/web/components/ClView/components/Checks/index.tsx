import { memo, useState } from 'react'

import { LoadingSpinner } from '@gitmono/ui'

import { useGetClTask } from '@/hooks/SSE/useGetClTask'

import { getQueuedBuildIds, TaskInfoDTO } from './cpns/store'
import { TreeRoot } from './cpns/Task'
import { useBuildSelection } from './hooks/useBuildSelection'
import { useLeftPanelScroll } from './hooks/useLeftPanelScroll'
import { useLogCache } from './hooks/useLogCache'
import { useMountedLogPanels } from './hooks/useMountedLogPanels'
import { useResizablePanels } from './hooks/useResizablePanels'
import { CachedLogPanel } from './LogViewer'

const LogLoadingState = ({ label = 'Loading logs...' }: { label?: string }) => (
  <div className='text-tertiary flex h-full items-center justify-center'>
    <div className='flex items-center gap-3'>
      <LoadingSpinner />
      <span>{label}</span>
    </div>
  </div>
)

const Checks = ({ cl, path, prName }: { cl: string; path?: string; prName?: string }) => {
  const { data: tasks, isError: isTasksError, isLoading: isTasksLoading } = useGetClTask(cl)
  const { buildId, selectBuild, selectedTaskId, selectTask } = useBuildSelection(cl, tasks)
  const { logsMap, logsAvailableIds, currentLogStatus, isQueued, retryLog } = useLogCache(cl, buildId, tasks)
  const mountedPanelIds = useMountedLogPanels(buildId, logsMap)

  const {
    containerRef,
    leftPanelRef,
    rightPanelRef,
    logContainerRef,
    leftWidth,
    isDragging,
    logViewerHeight,
    handleMouseDown,
    defaultLeftWidthPercent
  } = useResizablePanels()

  const { preserveScroll } = useLeftPanelScroll(cl, buildId, leftPanelRef)

  const handleSelectBuild = (nextBuildId: string, taskId?: string) => {
    preserveScroll()
    selectBuild(nextBuildId, taskId)
  }

  const [isDropdownOpen, setIsDropdownOpen] = useState(false)

  const getTaskFileName = (task: TaskInfoDTO) => {
    if (!task.targets || task.targets.length === 0) return task.task_name || 'Unnamed Task'

    const firstTarget = task.targets[0]

    if (!firstTarget.builds || firstTarget.builds.length === 0) return task.task_name || 'Unnamed Task'

    const firstBuild = firstTarget.builds[0]

    if (!firstBuild.output_file) return task.task_name || 'Unnamed Task'

    const parts = firstBuild.output_file.split('/')

    return parts[parts.length - 1] || 'Unnamed Task'
  }

  const getTaskStatus = (task: TaskInfoDTO) => {
    if (!task.targets || task.targets.length === 0) return null

    const states = task.targets.map((t) => t.state)

    if (states.some((s: string) => s === 'Failed')) {
      return { status: 'Failed', color: 'text-red-600 dark:text-red-400' }
    }

    if (states.some((s: string) => s === 'Interrupted')) {
      return { status: 'Interrupted', color: 'text-orange-600 dark:text-orange-400' }
    }

    if (states.some((s: string) => s === 'Building')) {
      return { status: 'Building', color: 'text-blue-600 dark:text-blue-400' }
    }

    if (states.some((s: string) => s === 'Pending')) {
      return { status: 'Pending', color: 'text-yellow-600 dark:text-yellow-400' }
    }

    if (states.some((s: string) => s === 'Uninitialized')) {
      return { status: 'Uninitialized', color: 'text-gray-600 dark:text-gray-400' }
    }

    if (states.every((s: string) => s === 'Completed')) {
      return { status: 'Completed', color: 'text-green-600 dark:text-green-400' }
    }

    return null
  }

  if (isTasksLoading) {
    return (
      <div className='bg-secondary' style={{ height: `calc(100vh - 104px)` }}>
        <div className='border-primary bg-primary flex h-[60px] items-center border-b px-4'>
          <span>
            <h2 className='text-tertiary text-bold fz-[14px]'>[] tasks status interface</h2>
          </span>
        </div>
        <div className='text-tertiary flex h-full items-center justify-center'>
          <div className='flex items-center gap-3'>
            <LoadingSpinner />
            <span>Loading tasks...</span>
          </div>
        </div>
      </div>
    )
  }

  if (isTasksError) {
    return (
      <div className='bg-secondary' style={{ height: `calc(100vh - 104px)` }}>
        <div className='border-primary bg-primary flex h-[60px] items-center border-b px-4'>
          <span>
            <h2 className='text-tertiary text-bold fz-[14px]'>[] tasks status interface</h2>
          </span>
        </div>
        <div className='flex h-full items-center justify-center text-red-500 dark:text-red-400'>
          <span>Failed to fetch tasks</span>
        </div>
      </div>
    )
  }

  const validTasks = tasks?.filter((t) => t.build_list && t.build_list.length > 0) || []
  const selectedTask = validTasks.find((t) => t.task_id === selectedTaskId)
  const tasksToDisplay = selectedTask ? [selectedTask] : validTasks

  if (!tasks || tasks.length === 0 || validTasks.length === 0) {
    return (
      <div className='bg-secondary' style={{ height: `calc(100vh - 104px)` }}>
        <div className='border-primary bg-primary flex h-[60px] items-center border-b px-4'>
          <span>
            <h2 className='text-tertiary text-bold fz-[14px]'>[] tasks status interface</h2>
          </span>
        </div>
        <div className='text-tertiary flex h-full items-center justify-center'>
          <span>No tasks available</span>
        </div>
      </div>
    )
  }

  const queuedBuildIds = getQueuedBuildIds(tasks)

  const renderLogContent = () => {
    if (!buildId) {
      return (
        <div className='text-tertiary flex h-full items-center justify-center'>
          <span>Select a build to view logs</span>
        </div>
      )
    }

    if (queuedBuildIds.has(buildId) || isQueued) {
      return (
        <div className='text-tertiary flex h-full items-center justify-center'>
          <div className='flex items-center gap-3'>
            <LoadingSpinner />
            <span>Waiting for an available worker — logs will appear once the build starts</span>
          </div>
        </div>
      )
    }

    const viewerHeight = logViewerHeight > 0 ? logViewerHeight : 'auto'
    const hasCachedCurrentBuild = Boolean(logsMap[buildId])
    const isLoadingCurrentBuild =
      !hasCachedCurrentBuild && (currentLogStatus === 'loading' || currentLogStatus === 'idle')

    if (mountedPanelIds.length > 0) {
      return (
        <div ref={logContainerRef} className='relative h-full select-text [&_span]:select-text'>
          {mountedPanelIds.map((id) => (
            <CachedLogPanel key={id} text={logsMap[id]} height={viewerHeight} visible={id === buildId} />
          ))}
          {isLoadingCurrentBuild ? (
            <div className='bg-secondary absolute inset-0 z-10'>
              <LogLoadingState />
            </div>
          ) : null}
        </div>
      )
    }

    if (isLoadingCurrentBuild || currentLogStatus === 'loading') {
      return <LogLoadingState />
    }

    if (currentLogStatus === 'error') {
      return (
        <div className='flex h-full flex-col items-center justify-center gap-3 text-red-500 dark:text-red-400'>
          <span>Failed to fetch logs</span>
          <button
            type='button'
            onClick={retryLog}
            className='rounded-md border border-red-300 px-3 py-1 text-sm hover:bg-red-50 dark:border-red-700 dark:hover:bg-red-900/20'
          >
            Retry
          </button>
        </div>
      )
    }

    if (currentLogStatus === 'empty') {
      return (
        <div className='text-tertiary flex h-full items-center justify-center'>
          <span>No logs available</span>
        </div>
      )
    }

    return (
      <div className='text-tertiary flex h-full items-center justify-center'>
        <span>Select a build to view logs</span>
      </div>
    )
  }

  return (
    <div className='bg-secondary' style={{ height: `calc(100vh - 104px)` }}>
      <div className='border-primary bg-primary flex h-[60px] items-center border-b px-4'>
        <div className='flex flex-1 items-center gap-2'>
          <h2 className='text-tertiary text-bold fz-[14px]'>[] tasks status interface</h2>

          {validTasks.length > 1 && (
            <div className='relative' style={{ minWidth: '240px', maxWidth: '320px' }}>
              <button
                onClick={(e) => {
                  e.stopPropagation()
                  setIsDropdownOpen(!isDropdownOpen)
                }}
                className='flex w-full items-center justify-between rounded-lg border border-gray-300 bg-white px-2.5 py-1.5 text-sm transition-all hover:border-blue-400 hover:bg-gray-50 focus:border-blue-500 focus:ring-2 focus:ring-blue-500/20 focus:outline-hidden dark:border-gray-600 dark:bg-gray-800 dark:hover:border-blue-500 dark:hover:bg-gray-700'
              >
                <div className='flex flex-1 items-center gap-2 overflow-hidden'>
                  {selectedTask ? (
                    <>
                      <span className='truncate font-medium text-gray-800 dark:text-gray-200'>
                        {getTaskFileName(selectedTask)}
                      </span>
                      {getTaskStatus(selectedTask) && (
                        <span className={`shrink-0 text-xs ${getTaskStatus(selectedTask)?.color}`}>
                          • {getTaskStatus(selectedTask)?.status}
                        </span>
                      )}
                    </>
                  ) : (
                    <span className='text-gray-500 dark:text-gray-400'>Select a task...</span>
                  )}
                </div>
                <svg
                  className={`ml-2 h-4 w-4 shrink-0 text-gray-500 transition-transform dark:text-gray-400 ${
                    isDropdownOpen ? 'rotate-180' : ''
                  }`}
                  fill='none'
                  stroke='currentColor'
                  viewBox='0 0 24 24'
                >
                  <path strokeLinecap='round' strokeLinejoin='round' strokeWidth={2} d='M19 9l-7 7-7-7' />
                </svg>
              </button>

              {isDropdownOpen && (
                <>
                  <div className='fixed inset-0 z-10' onClick={() => setIsDropdownOpen(false)} />
                  <div className='absolute top-full left-0 z-20 mt-1 max-h-80 w-full overflow-y-auto rounded-lg border border-gray-300 bg-white shadow-lg dark:border-gray-600 dark:bg-gray-800'>
                    {validTasks.map((task) => {
                      const fileName = getTaskFileName(task)
                      const status = getTaskStatus(task)
                      const isSelected = task.task_id === selectedTaskId
                      const targetsCount = task.targets?.length || 0

                      return (
                        <div
                          key={task.task_id}
                          onClick={(e) => {
                            e.stopPropagation()
                            preserveScroll()
                            selectTask(task.task_id)
                            setIsDropdownOpen(false)
                          }}
                          className={`flex cursor-pointer items-center justify-between px-3 py-2.5 transition-colors ${
                            isSelected ? 'bg-blue-50 dark:bg-blue-900/30' : 'hover:bg-gray-100 dark:hover:bg-gray-700'
                          }`}
                        >
                          <div className='flex flex-1 flex-col gap-1 overflow-hidden'>
                            <div className='flex items-center gap-2'>
                              <span
                                className={`truncate text-sm font-medium ${
                                  isSelected ? 'text-blue-700 dark:text-blue-300' : 'text-gray-800 dark:text-gray-200'
                                }`}
                              >
                                {fileName}
                              </span>
                              {status && <span className={`shrink-0 text-xs ${status.color}`}>• {status.status}</span>}
                            </div>
                            <span className='truncate text-xs text-gray-500 dark:text-gray-400'>
                              {targetsCount} {targetsCount === 1 ? 'target' : 'targets'}
                            </span>
                          </div>
                        </div>
                      )
                    })}
                  </div>
                </>
              )}
            </div>
          )}
        </div>
      </div>

      <div ref={containerRef} className='flex' style={{ height: `calc(100vh - 164px)` }}>
        <div
          ref={leftPanelRef}
          data-build-list-scroll
          className='border-primary h-full overflow-y-auto border-r [overflow-anchor:none]'
          style={{ width: leftWidth ?? `${defaultLeftWidthPercent * 100}%`, flexShrink: 0 }}
        >
          <TreeRoot
            path={path}
            prName={prName}
            tasks={tasksToDisplay}
            logsAvailableIds={logsAvailableIds}
            selectedBuildId={buildId}
            onSelectBuild={handleSelectBuild}
            totalTasksCount={validTasks.length}
            cl={cl}
          />
        </div>
        <div
          onMouseDown={handleMouseDown}
          className='border-primary h-full w-1 shrink-0 cursor-col-resize transition-colors hover:bg-blue-400'
          style={{ backgroundColor: isDragging ? '#60a5fa' : undefined }}
        />
        <div ref={rightPanelRef} className='flex-1' style={{ display: isDragging ? 'none' : 'block' }}>
          {renderLogContent()}
        </div>
        {isDragging && (
          <div className='text-tertiary flex flex-1 items-center justify-center'>
            <span>Resizing...</span>
          </div>
        )}
      </div>
    </div>
  )
}

export default memo(Checks)
