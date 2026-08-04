import React, { useCallback, useMemo, useState } from 'react'
import { FeedMergedIcon } from '@primer/octicons-react'
import { useQueryClient } from '@tanstack/react-query'
import { useRouter } from 'next/router'

import { CheckType, ConditionResult } from '@gitmono/types'
import { LoadingSpinner } from '@gitmono/ui'

import { useGetMergeBox } from '@/components/ClBox/hooks/useGetMergeBox'
import { useGetClReviewers } from '@/hooks/CL/useGetClReviewers'
import { usePostClReviewerApprove } from '@/hooks/CL/usePostClReviewerApprove'
import { useGetCurrentUser } from '@/hooks/useGetCurrentUser'
import { megaUserHandlesMatch } from '@/utils/megaUser'
import { legacyApiClient } from '@/utils/queryClient'

import { ChecksSection } from './ChecksSection'
import { DraftStatusBanner } from './components/DraftStatusBanner'
import { useMergeChecks } from './hooks/useMergeChecks'
import { MergeSection } from './MergeSection'
import { ReviewerSection } from './ReviewerSection'

export const MergeBox = React.memo<{ prId: string; status?: string; author?: string }>(({ prId, status, author }) => {
  const { checks } = useMergeChecks(prId)
  const [hasCheckFailures, setHasCheckFailures] = useState(true)
  const route = useRouter()
  const { link } = route.query
  const id = typeof link === 'string' ? link : ''

  const { mutate: reviewApprove } = usePostClReviewerApprove()
  const queryClient = useQueryClient()
  const { reviewers, isLoading: isReviewerLoading } = useGetClReviewers(id)

  // At least one approval is enough when reviewers are assigned.
  const required: number = useMemo(() => (reviewers.length > 0 ? 1 : 0), [reviewers])
  const actual: number = useMemo(() => reviewers.filter((i) => i.approved).length, [reviewers])
  const isAllReviewerApproved: boolean = useMemo(() => actual >= required, [actual, required])

  let isNowUserApprove: boolean | undefined = undefined
  const { data } = useGetCurrentUser()
  const find_user = reviewers.find((i) => megaUserHandlesMatch(i.campsite_user_id || i.username, data))

  if (find_user) {
    isNowUserApprove = find_user.approved
  }

  const { mergeBoxData, isLoading: isAdditionLoading } = useGetMergeBox(prId)

  const handleApprove = useCallback(async () => {
    reviewApprove(
      {
        link: id,
        data: {
          approved: true
        }
      },
      {
        onSuccess: () => {
          queryClient.invalidateQueries({
            queryKey: legacyApiClient.v1.getApiClReviewers().requestKey(id)
          })
        }
      }
    )
  }, [reviewApprove, id, queryClient])

  const additionalChecks = mergeBoxData?.merge_requirements?.conditions ?? []

  const claCondition = additionalChecks.find((c) => c.type === CheckType.ClaSign)
  const claCheck = claCondition ? claCondition.result === ConditionResult.PASSED : true

  const isClAuthor = megaUserHandlesMatch(author, data)

  return (
    <div className='flex'>
      <FeedMergedIcon size={24} className='text-tertiary ml-1' />
      {isReviewerLoading && isAdditionLoading ? (
        <div className='flex h-[400px] items-center justify-center'>
          <LoadingSpinner />
        </div>
      ) : (
        <div className='border-primary bg-primary ml-3 w-full divide-y rounded-lg border'>
          <ReviewerSection required={required} actual={actual} />
          <ChecksSection
            checks={checks}
            onStatusChange={setHasCheckFailures}
            additionalChecks={additionalChecks}
            clLink={id}
          />
          {status === 'Draft' && <DraftStatusBanner link={id} />}

          <MergeSection
            isNowUserApprove={isNowUserApprove}
            isAllReviewerApproved={isAllReviewerApproved}
            hasCheckFailures={hasCheckFailures}
            onApprove={handleApprove}
            clStatus={status}
            clLink={id}
            claCheck={claCheck}
            isClAuthor={isClAuthor}
          />
        </div>
      )}
    </div>
  )
})

MergeBox.displayName = 'MergeBox'
