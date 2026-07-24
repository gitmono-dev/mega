import React, { useEffect, useState } from 'react'
import { useQueryClient } from '@tanstack/react-query'
import { useRouter } from 'next/router'

import { QueueStatus } from '@gitmono/types/generated'
import { CheckCircleIcon, LoadingSpinner, Tooltip, WarningTriangleIcon } from '@gitmono/ui'

import { useScope } from '@/contexts/scope'
import { useGetMergeQueueStatus } from '@/hooks/MergeQueue/useGetMergeQueueStatus'
import { usePostMergeQueueAdd } from '@/hooks/MergeQueue/usePostMergeQueueAdd'
import { legacyApiClient } from '@/utils/queryClient'

interface MergeSectionProps {
  isNowUserApprove?: boolean
  isAllReviewerApproved: boolean
  hasCheckFailures: boolean
  onApprove: () => void
  clLink: string
  clStatus?: string
  claCheck?: boolean
  isClAuthor?: boolean
}

const STATUS_POLL_INTERVAL_MS = 3000

export const MergeSection = React.memo<MergeSectionProps>(
  ({
    isAllReviewerApproved,
    hasCheckFailures: _hasCheckFailures,
    isNowUserApprove,
    onApprove,
    clLink,
    clStatus,
    claCheck = true,
    isClAuthor = false
  }) => {
    const router = useRouter()
    const { scope } = useScope()
    const queryClient = useQueryClient()

    const [trackQueueStatus, setTrackQueueStatus] = useState(false)
    const [hasFetchedInitialStatus, setHasFetchedInitialStatus] = useState(false)

    useEffect(() => {
      setTrackQueueStatus(false)
      setHasFetchedInitialStatus(false)
    }, [clLink])

    const shouldFetchStatus = !hasFetchedInitialStatus || trackQueueStatus
    const { data: queueStatusData } = useGetMergeQueueStatus(clLink, undefined, {
      enabled: shouldFetchStatus,
      refetchInterval: trackQueueStatus ? STATUS_POLL_INTERVAL_MS : false
    })
    const { mutate: addToQueue, isPending: isAddingToQueue } = usePostMergeQueueAdd()

    const queueItem = queueStatusData?.data?.item
    const queueStatus = queueItem?.status
    const isTerminalStatus = queueStatus === QueueStatus.Merged || queueStatus === QueueStatus.Failed

    useEffect(() => {
      if (!hasFetchedInitialStatus && queueStatusData) {
        setHasFetchedInitialStatus(true)

        if (queueStatusData.data?.in_queue && !isTerminalStatus) {
          setTrackQueueStatus(true)
        }
      }
    }, [hasFetchedInitialStatus, queueStatusData, isTerminalStatus])

    useEffect(() => {
      if (trackQueueStatus && isTerminalStatus) {
        setTrackQueueStatus(false)
        queryClient.invalidateQueries({
          queryKey: legacyApiClient.v1.getApiClDetail().requestKey(clLink)
        })
      }
    }, [trackQueueStatus, isTerminalStatus, clLink, queryClient])

    const inQueue = queueStatusData?.data?.in_queue ?? false

    let statusNode: React.ReactNode

    // CLA is advisory-only for now: show a hint but do not block merge.
    const isMergeable = isAllReviewerApproved
    const isDraft = clStatus?.toLowerCase() === 'draft'
    const showClaHint = claCheck === false

    if (isDraft) {
      statusNode = (
        <div className='flex items-center text-yellow-700'>
          <WarningTriangleIcon className='mr-3 h-5 w-5' />
          <span className='font-semibold'>CL has not yet prepared for the review</span>
        </div>
      )
    } else if (!isAllReviewerApproved) {
      statusNode = (
        <div className='flex items-center text-yellow-700'>
          <WarningTriangleIcon className='mr-3 h-5 w-5' />
          <span className='font-semibold'>Merging is blocked - Waiting for reviewers approval</span>
        </div>
      )
    } else {
      statusNode = (
        <div className='flex items-center text-green-700'>
          <CheckCircleIcon className='mr-3 h-5 w-5' />
          <span className='font-semibold'>Ready to merge - All reviewers approved</span>
        </div>
      )
    }

    const handleAddToQueue = () => {
      addToQueue(
        { cl_link: clLink },
        {
          onSuccess: () => {
            setTrackQueueStatus(true)
          }
        }
      )
    }

    const handleViewQueue = () => {
      router.push(`/${scope}/queue/main`)
    }

    return (
      <div className='p-3'>
        {statusNode}

        {showClaHint && (
          <div className='mb-3 mt-2 flex items-start gap-2 rounded-md bg-amber-50 p-2 text-sm text-amber-800 dark:bg-amber-900/20 dark:text-amber-200'>
            <WarningTriangleIcon className='mt-0.5 h-4 w-4 flex-shrink-0' />
            <span>
              CLA is not signed yet. This is a reminder only and does not block merging.
              {isClAuthor && ' Please sign the CLA when you can.'}
            </span>
          </div>
        )}

        {/* Queue Status Info */}
        {inQueue && queueItem && (
          <div className='mb-3 mt-2 rounded-md bg-blue-50 p-2 text-sm'>
            <div className='flex items-center justify-between'>
              <span className='text-blue-800'>
                In merge queue • Status: <strong>{queueItem.status}</strong>
              </span>
              <button onClick={handleViewQueue} className='text-blue-600 underline hover:text-blue-800'>
                View Queue
              </button>
            </div>
          </div>
        )}

        {!inQueue && (
          <div className='ClBox-MergeSection flex items-center justify-center gap-4' style={{ marginTop: '12px' }}>
            {!isDraft && (
              <button
                onClick={onApprove}
                disabled={isNowUserApprove === undefined || isNowUserApprove}
                className='w-full rounded-md bg-green-600 px-4 py-2 font-bold text-white duration-500 hover:bg-green-800 disabled:cursor-not-allowed disabled:bg-gray-400'
              >
                Approve
              </button>
            )}

            {showClaHint && isClAuthor && (
              <button
                onClick={() => router.push(`/me/settings/cla/sign`)}
                className='w-full rounded-md bg-blue-600 px-4 py-2 font-bold text-white duration-500 hover:bg-blue-800'
              >
                Sign CLA
              </button>
            )}

            {isDraft ? (
              <Tooltip
                label={
                  <div className='rounded-md bg-[#25292e] px-3 py-1 text-xs text-white'>
                    Merging is blocked due to failing merge requirements
                  </div>
                }
                side='top'
              >
                <button
                  onClick={handleAddToQueue}
                  disabled
                  className='w-full rounded-md bg-purple-600 px-4 py-2 font-bold text-white duration-500 hover:bg-purple-800 disabled:cursor-not-allowed disabled:bg-gray-400'
                >
                  Add to Queue
                </button>
              </Tooltip>
            ) : (
              <button
                onClick={handleAddToQueue}
                disabled={isAddingToQueue || !isMergeable}
                className='w-full rounded-md bg-purple-600 px-4 py-2 font-bold text-white duration-500 hover:bg-purple-800 disabled:cursor-not-allowed disabled:bg-gray-400'
              >
                {isAddingToQueue ? <LoadingSpinner /> : 'Add to Queue'}
              </button>
            )}
          </div>
        )}
      </div>
    )
  }
)

MergeSection.displayName = 'MergeSection'
