import { useEffect, useMemo, useState } from 'react'
import * as Collapsible from '@radix-ui/react-collapsible'

import { CheckType, ConditionResult } from '@gitmono/types'
import { AlertIcon, ArrowDownIcon, ArrowUpIcon, CheckIcon, LoadingSpinner, WarningTriangleIcon } from '@gitmono/ui'

import { useGetClUpdateStatus } from '@/hooks/CL/useGetClUpdateStatus'
import { usePostClUpdateBranch } from '@/hooks/CL/usePostClUpdateBranch'

import { MergeCheckItem } from './components/MergeCheckItem'
import type { AdditionalCheckItem, AdditionalCheckStatus, TaskData } from './types/mergeCheck.config'
import { ADDITIONAL_CHECK_LABELS } from './types/mergeCheck.config'

interface ChecksSectionProps {
  checks: TaskData[]
  onStatusChange: (hasFailures: boolean) => void
  additionalChecks?: AdditionalCheckItem[]
  clLink: string
}

interface CheckStatusProps {
  hasFailures: boolean
  failureCount: number
  inProgressCount: number
  successCount: number
  open: boolean
  isOutdated: boolean
  onUpdateBranch: () => void
  isUpdating: boolean
}

interface CheckGroupProps {
  title: string
  checks: TaskData[]
}

interface CheckListProps {
  groupedChecks: {
    failing: TaskData[]
    pending: TaskData[]
    success: TaskData[]
  }
}

const CheckStatus = ({
  hasFailures,
  failureCount,
  inProgressCount,
  successCount,
  open,
  isOutdated,
  onUpdateBranch,
  isUpdating
}: CheckStatusProps) => {
  let statusInfo: string[] = []

  if (failureCount > 0) {
    statusInfo.push(`${failureCount} failed`)
  }
  if (inProgressCount > 0) {
    statusInfo.push(`${inProgressCount} in progress`)
  }
  if (successCount > 0) {
    statusInfo.push(`${successCount} successful`)
  }
  if (statusInfo.length === 0) {
    statusInfo.push('No checks have run yet')
  }

  return (
    <div className='flex w-full items-center px-3 py-0'>
      {hasFailures ? (
        <AlertIcon className='mr-3 h-5 w-5 text-yellow-600 dark:text-yellow-500' />
      ) : (
        <CheckIcon className='mr-3 h-5 w-5 text-green-700 dark:text-green-400' />
      )}
      <div>
        <p className='font-semibold'>{hasFailures ? 'Some checks were not successful' : 'All checks have passed'}</p>
        <p className='text-tertiary text-sm'>{statusInfo.join(', ')}</p>
      </div>
      <div className='ml-auto flex items-center gap-2'>
        {isOutdated && (
          <button
            onClick={(e) => {
              e.stopPropagation()
              onUpdateBranch()
            }}
            disabled={isUpdating}
            className='border-primary bg-secondary hover:bg-tertiary text-secondary flex-shrink-0 rounded-md border px-3 py-1.5 text-sm font-medium disabled:cursor-not-allowed disabled:opacity-50'
          >
            {isUpdating ? (
              <span className='flex items-center gap-1.5'>
                <LoadingSpinner />
                Updating...
              </span>
            ) : (
              'Update branch'
            )}
          </button>
        )}
        <button>{open ? <ArrowUpIcon /> : <ArrowDownIcon />}</button>
      </div>
    </div>
  )
}

const CheckGroup = ({ title, checks }: CheckGroupProps) => (
  <div className='mb-2'>
    <h4 className='text-tertiary px-2 py-1 text-xs font-bold uppercase'>
      {title} ({checks.length})
    </h4>
    <div>
      {checks.map((check) => (
        <MergeCheckItem key={check.build_id} check={check} />
      ))}
    </div>
  </div>
)

const CheckList = ({ groupedChecks }: CheckListProps) => (
  <div className='mt-2 max-h-[300px] overflow-y-auto border-t pt-2'>
    {groupedChecks.failing.length > 0 && <CheckGroup title='Failing' checks={groupedChecks.failing} />}
    {groupedChecks.pending.length > 0 && <CheckGroup title='In progress' checks={groupedChecks.pending} />}
    {groupedChecks.success.length > 0 && <CheckGroup title='Successful' checks={groupedChecks.success} />}
  </div>
)

interface AdditionalCheckItemProps {
  check: AdditionalCheckItem
}

const getStatusIcon = (status: AdditionalCheckStatus, advisory = false) => {
  if (advisory && status === 'FAILED') {
    return <WarningTriangleIcon className='h-4 w-4 text-amber-600 dark:text-amber-400' />
  }
  switch (status) {
    case 'PASSED':
      return <CheckIcon className='h-4 w-4 text-green-600' />
    case 'FAILED':
      return <AlertIcon className='h-4 w-4 text-red-600' />
    default:
      return <LoadingSpinner />
  }
}

const AdditionalCheckItemComponent = ({ check }: AdditionalCheckItemProps) => {
  const isClaAdvisory = check.type === CheckType.ClaSign && check.result === ConditionResult.FAILED

  return (
    <div className='border-primary flex items-start border-b px-2 py-2 last:border-b-0'>
      <div className='mr-3 mt-0.5 flex-shrink-0'>{getStatusIcon(check.result, isClaAdvisory)}</div>
      <div className='min-w-0 flex-1'>
        <div className='flex items-center justify-between'>
          <h5 className='text-primary text-sm font-medium'>{ADDITIONAL_CHECK_LABELS[check.type]}</h5>
          <span
            className={`rounded-full px-2 py-1 text-xs font-medium ${
              check.result === 'PASSED'
                ? 'bg-green-100 text-green-800 dark:bg-green-950 dark:text-green-200'
                : isClaAdvisory
                  ? 'bg-amber-100 text-amber-800 dark:bg-amber-950 dark:text-amber-200'
                  : check.result === 'FAILED'
                    ? 'bg-red-100 text-red-800 dark:bg-red-950 dark:text-red-200'
                    : 'bg-secondary text-secondary'
            }`}
          >
            {isClaAdvisory ? 'advisory' : check.result.toLowerCase()}
          </span>
        </div>
        {check.result === 'FAILED' && (
          <ul
            className={`mt-1 list-inside list-disc text-sm ${
              isClaAdvisory ? 'text-amber-700 dark:text-amber-300' : 'text-red-600 dark:text-red-400'
            }`}
          >
            <li className='list-inside'>{isClaAdvisory ? `${check.message} (does not block merge)` : check.message}</li>
          </ul>
        )}
      </div>
    </div>
  )
}

interface AdditionalChecksSectionProps {
  additionalChecks: AdditionalCheckItem[]
}

const AdditionalChecksSection = ({ additionalChecks }: AdditionalChecksSectionProps) => {
  if (!additionalChecks || additionalChecks.length === 0) {
    return null
  }

  return (
    <div className='mt-2 border-t pt-2'>
      <h4 className='text-tertiary mb-2 px-2 py-1 text-xs font-bold uppercase'>
        Additional Checks ({additionalChecks.length})
      </h4>
      <div className='space-y-1'>
        {additionalChecks.map((check) => (
          <AdditionalCheckItemComponent key={check.message} check={check} />
        ))}
      </div>
    </div>
  )
}

export function ChecksSection({ checks, onStatusChange, additionalChecks, clLink }: ChecksSectionProps) {
  // Get update status
  const { data: updateStatus } = useGetClUpdateStatus(clLink, true, 30000)

  // Update branch mutation
  const { mutate: updateBranch, isPending: isUpdating } = usePostClUpdateBranch()

  const summary = useMemo(() => {
    return checks.reduce(
      (acc, check) => {
        acc[check.status] = (acc[check.status] || 0) + 1
        return acc
      },
      {} as Record<TaskData['status'], number>
    )
  }, [checks])

  const failureCount = summary.Failure || 0
  const inProgressCount = summary.Pending || 0
  const successCount = summary.Success || 0
  const hasFailures = failureCount > 0

  useEffect(() => {
    onStatusChange(hasFailures)
  }, [hasFailures, onStatusChange])

  const groupedChecks = useMemo(() => {
    const failing = checks.filter((c) => c.status === 'Failure')
    const pending = checks.filter((c) => c.status === 'Pending')
    const success = checks.filter((c) => c.status === 'Success')

    return { failing, pending, success }
  }, [checks])

  const [open, setOpen] = useState(false)

  const isOutdated = updateStatus?.data?.outdated || false

  const handleUpdateBranch = () => {
    updateBranch(clLink)
  }

  return (
    <>
      <Collapsible.Root open={open} onOpenChange={setOpen}>
        {/* CheckStatus section */}
        <Collapsible.Trigger className='hover:bg-tertiary flex w-full cursor-pointer rounded-md'>
          <CheckStatus
            hasFailures={hasFailures}
            failureCount={failureCount}
            inProgressCount={inProgressCount}
            successCount={successCount}
            open={open}
            isOutdated={isOutdated}
            onUpdateBranch={handleUpdateBranch}
            isUpdating={isUpdating}
          />
        </Collapsible.Trigger>

        {/* CheckList & AdditionalChecks section */}
        <Collapsible.Content>
          <CheckList groupedChecks={groupedChecks} />
          <AdditionalChecksSection additionalChecks={additionalChecks || []} />
        </Collapsible.Content>
      </Collapsible.Root>
    </>
  )
}
