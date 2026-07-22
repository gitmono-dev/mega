import { useQuery } from '@tanstack/react-query'

import {
  BuildEventDTO,
  BuildTargetDTO,
  RequestParams,
  StatusProjectRelativePath,
  TargetState
} from '@gitmono/types/generated'

import { BuildDTO, BuildStatus, TargetDTO, TaskInfoDTO } from '@/components/ClView/components/Checks/cpns/store'
import { orionApiClient } from '@/utils/queryClient'

const toBuildStatus = (build: BuildEventDTO): BuildStatus => {
  if (!build.end_at) return 'Building'
  if (build.exit_code === 0) return 'Completed'
  return 'Failed'
}

const toTargetState = (state: string): TargetState => {
  switch (state) {
    case TargetState.Pending:
    case TargetState.Building:
    case TargetState.Completed:
    case TargetState.Failed:
    case TargetState.Interrupted:
    case TargetState.Uninitialized:
      return state
    default:
      return TargetState.Uninitialized
  }
}

export function useGetClTask(cl: string, params?: RequestParams) {
  // NOTE(P3): Each task currently triggers 2 extra API calls (buildEvents + targets).
  // A future Orion `GET /cl/{link}/tasks-full` aggregate endpoint would remove this N+1.
  return useQuery<TaskInfoDTO[], Error>({
    queryKey: [...orionApiClient.task.getTaskByClV2().requestKey(cl), params],
    enabled: Boolean(cl),
    // Poll while any task is still queued (Uninitialized) or actively building,
    // so the tree transitions automatically once a worker picks it up, the build
    // finishes, or the server times it out (-> Interrupted). Stops when idle.
    refetchInterval: (query) => {
      const data = query.state.data

      if (!data) return false

      const hasActive = data.some(
        (task) =>
          task.targets?.some((t) => t.state === 'Uninitialized' || t.state === 'Building') ||
          task.build_list?.some((b) => b.status === 'Building')
      )

      return hasActive ? 5000 : false
    },
    queryFn: async () => {
      const tasks = (await orionApiClient.task.getTaskByClV2().request(cl, params)) ?? []

      const hydratedTasks = await Promise.all(
        tasks.map(async (task): Promise<TaskInfoDTO> => {
          const [buildEvents, targets] = await Promise.all([
            orionApiClient.buildEvents
              .getBuildEventsByTaskIdV2()
              .request(task.id)
              .catch(() => [] as BuildEventDTO[]),
            orionApiClient.targets
              .getTargetsByTaskIdV2()
              .request(task.id)
              .catch(() => [] as BuildTargetDTO[])
          ])

          const buildList: BuildDTO[] = buildEvents.map((build) => ({
            args: [],
            created_at: build.start_at,
            end_at: build.end_at ?? undefined,
            exit_code: build.exit_code ?? undefined,
            id: build.id,
            output_file: build.log_output_file,
            repo: task.repo_name,
            retry_count: build.retry_count,
            start_at: build.start_at,
            status: toBuildStatus(build),
            target: '',
            task_id: build.task_id
          }))

          const mappedTargets: TargetDTO[] = targets.map((target) => ({
            builds: buildList,
            id: target.id,
            start_at: task.created_at,
            state: toTargetState(target.latest_state),
            target_path: target.path
          }))

          return {
            build_list: buildList,
            changes: (task.changes ?? []) as StatusProjectRelativePath[],
            cl_id: 0,
            created_at: task.created_at,
            targets: mappedTargets,
            task_id: task.id,
            task_name: task.repo_name,
            template: ''
          }
        })
      )

      return hydratedTasks
    }
  })
}
