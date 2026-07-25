import React from 'react'
import { ClockIcon, GitMergeIcon, GitPullRequestIcon, XCircleIcon } from '@primer/octicons-react'

import type { QueueStats } from '@gitmono/types/generated'

interface QueueStatsCardProps {
  stats: QueueStats
  isLoading?: boolean
}

export const QueueStatsCard: React.FC<QueueStatsCardProps> = ({ stats, isLoading }) => {
  if (isLoading) {
    return (
      <div className='border-primary bg-primary rounded-lg border p-4 shadow-xs'>
        <div className='animate-pulse'>
          <div className='bg-secondary mb-3 h-4 w-32 rounded'></div>
          <div className='space-y-3'>
            <div className='bg-secondary h-8 w-full rounded'></div>
            <div className='bg-secondary h-8 w-full rounded'></div>
            <div className='bg-secondary h-8 w-full rounded'></div>
          </div>
        </div>
      </div>
    )
  }

  return (
    <div className='flex flex-col gap-3'>
      <div className='border-primary bg-primary rounded-lg border p-4 shadow-xs transition-shadow hover:shadow-md'>
        <div className='mb-3 flex items-center justify-between'>
          <span className='text-tertiary text-xs font-semibold tracking-wide'>Merge Count</span>
        </div>

        <div className='flex items-center gap-3'>
          <div className='flex h-12 w-12 items-center justify-center rounded-full pl-3'>
            <GitMergeIcon size={20} className='text-secondary dark:text-tertiary' />
          </div>
          <div className='flex flex-col pl-3'>
            <span className='text-primary text-2xl font-bold'>{stats.merged_count}</span>
            <span className='text-tertiary text-xs'>Merged in queue</span>
          </div>
        </div>
      </div>

      {(stats.waiting_count > 0 || stats.testing_count > 0 || stats.merging_count > 0 || stats.failed_count > 0) && (
        <div className='border-primary bg-primary rounded-lg border p-4 shadow-xs transition-shadow hover:shadow-md'>
          <div className='space-y-2.5'>
            {/* Merging */}
            {stats.merging_count > 0 && (
              <div className='group flex items-center justify-between rounded-md p-2 transition-colors hover:bg-purple-50 dark:hover:bg-purple-950/30'>
                <div className='flex items-center gap-2.5'>
                  <div className='flex h-8 w-8 items-center justify-center rounded-full bg-purple-100 dark:bg-purple-900/50'>
                    <GitMergeIcon size={14} className='text-purple-600 dark:text-purple-400' />
                  </div>
                  <span className='text-secondary text-sm font-medium'>Merging</span>
                </div>
                <span className='text-lg font-semibold text-purple-600 dark:text-purple-400'>
                  {stats.merging_count}
                </span>
              </div>
            )}

            {/* Waiting */}
            {stats.waiting_count > 0 && (
              <div className='group flex items-center justify-between rounded-md p-2 transition-colors hover:bg-yellow-50 dark:hover:bg-yellow-950/30'>
                <div className='flex items-center gap-2.5'>
                  <div className='flex h-8 w-8 items-center justify-center rounded-full bg-yellow-100 dark:bg-yellow-900/50'>
                    <ClockIcon size={14} className='text-yellow-600 dark:text-yellow-400' />
                  </div>
                  <span className='text-secondary text-sm font-medium'>Waiting</span>
                </div>
                <span className='text-lg font-semibold text-yellow-600 dark:text-yellow-400'>
                  {stats.waiting_count}
                </span>
              </div>
            )}

            {/* Testing */}
            {stats.testing_count > 0 && (
              <div className='group flex items-center justify-between rounded-md p-2 transition-colors hover:bg-blue-50 dark:hover:bg-blue-950/30'>
                <div className='flex items-center gap-2.5'>
                  <div className='flex h-8 w-8 items-center justify-center rounded-full bg-blue-100 dark:bg-blue-900/50'>
                    <GitPullRequestIcon size={14} className='text-blue-600 dark:text-blue-400' />
                  </div>
                  <span className='text-secondary text-sm font-medium'>Testing</span>
                </div>
                <span className='text-lg font-semibold text-blue-600 dark:text-blue-400'>{stats.testing_count}</span>
              </div>
            )}

            {/* Failed */}
            {stats.failed_count > 0 && (
              <div className='group flex items-center justify-between rounded-md p-2 transition-colors hover:bg-red-50 dark:hover:bg-red-950/30'>
                <div className='flex items-center gap-2.5'>
                  <div className='flex h-8 w-8 items-center justify-center rounded-full bg-red-100 dark:bg-red-900/50'>
                    <XCircleIcon size={14} className='text-red-600 dark:text-red-400' />
                  </div>
                  <span className='text-secondary text-sm font-medium'>Failed</span>
                </div>
                <span className='text-lg font-semibold text-red-600 dark:text-red-400'>{stats.failed_count}</span>
              </div>
            )}
          </div>
        </div>
      )}
    </div>
  )
}
