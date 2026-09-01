import { atom } from 'jotai'
import { atomFamily } from 'jotai/utils'

import { StatusProjectRelativePath, TargetState } from '@gitmono/types/generated'

export type BuildStatus = 'Pending' | 'Completed' | 'Failed' | 'Building' | 'Interrupted' | 'Uninitialized'

export interface BuildDTO {
  args: string[]
  created_at: string
  end_at?: string
  exit_code?: number
  id: string
  output_file: string
  repo: string
  retry_count: number
  start_at: string
  status: BuildStatus
  target: string
  task_id: string
}

export interface TargetDTO {
  builds: BuildDTO[]
  end_at?: string
  error_summary?: string
  id: string
  start_at: string
  state: TargetState
  target_path: string
}

export interface TaskInfoDTO {
  build_list: BuildDTO[]
  changes: StatusProjectRelativePath[]
  cl_id: string
  created_at: string
  targets: TargetDTO[]
  task_id: string
  task_name: string
  template: string
}

export enum Status {
  Pending = 'Pending',
  Completed = 'Completed',
  Failed = 'Failed',
  Building = 'Building',
  Interrupted = 'Interrupted',
  Uninitialized = 'Uninitialized',
  NotFound = 'NotFound'
}

/**
 * A task is "queued" when it has been accepted but no worker has picked it up
 * yet: its target(s) are still `Uninitialized` and none is `Building`.
 */
export const isTaskQueued = (task: TaskInfoDTO): boolean => {
  const states = task.targets?.map((t) => t.state) ?? []

  if (states.length === 0) return false

  const isBuilding = states.some((s) => s === 'Building')

  return !isBuilding && states.some((s) => s === 'Uninitialized')
}

/**
 * Id of the task's most recent build (by start time). Only this build is eligible
 * for retry; superseded builds are read-only history.
 */
export const getLatestBuildId = (task: TaskInfoDTO): string | undefined => {
  let latest: BuildDTO | undefined

  task.build_list?.forEach((build) => {
    if (!latest || new Date(build.start_at).getTime() > new Date(latest.start_at).getTime()) {
      latest = build
    }
  })

  return latest?.id
}

/** Most recent build across all tasks (by start time). */
export const getLatestBuildIdFromTasks = (tasks: TaskInfoDTO[]): string | undefined => {
  let latest: BuildDTO | undefined

  tasks.forEach((task) => {
    task.build_list?.forEach((build) => {
      if (!latest || new Date(build.start_at).getTime() > new Date(latest.start_at).getTime()) {
        latest = build
      }
    })
  })

  return latest?.id
}

export const getAllBuildIds = (tasks: TaskInfoDTO[]): Set<string> => {
  const ids = new Set<string>()

  tasks.forEach((task) => {
    task.build_list?.forEach((build) => {
      if (build.id) ids.add(build.id)
    })
  })

  return ids
}

export const findTaskIdByBuildId = (tasks: TaskInfoDTO[], buildId: string): string | undefined => {
  return tasks.find((task) => task.build_list?.some((build) => build.id === buildId))?.task_id
}

/**
 * Collect the build ids that are still queued (waiting for a worker). These have
 * no logs yet, so callers should avoid fetching logs for them and instead show a
 * "waiting" placeholder.
 */
export const getQueuedBuildIds = (tasks: TaskInfoDTO[]): Set<string> => {
  const ids = new Set<string>()

  tasks.forEach((task) => {
    if (!isTaskQueued(task)) return

    task.build_list?.forEach((build) => {
      if (build.status === 'Building') ids.add(build.id)
    })
  })

  return ids
}

export const buildIdAtomFamily = atomFamily((_cl: string) => atom(''))
export const logsAtomFamily = atomFamily((_cl: string) => atom<Record<string, string>>({}))

export const statusAtom = atom<Record<string, Status>>({})
export const loadingAtom = atom(true)
export const tabAtom = atom<'conversation' | 'check' | 'filechange'>('conversation')
