import pluralize from 'pluralize'

import { Button, UserCirclePlusIcon } from '@gitmono/ui'

import { usePendingAccountReviewCount } from '@/hooks/usePendingAccountReviewCount'

/** Top-right indicator for admins when users are waiting for account review. */
export function PendingAccountReviewButton() {
  const { hasPending, pendingCount, isAdmin } = usePendingAccountReviewCount()

  if (!isAdmin || !hasPending) return null

  return (
    <div className='relative'>
      <Button
        variant='plain'
        href='/me/settings/account-review'
        iconOnly={<UserCirclePlusIcon className='text-amber-600 dark:text-amber-400' />}
        accessibilityLabel={`${pendingCount} pending account ${pluralize('review', pendingCount)}`}
        tooltip={`${pendingCount} user${pendingCount === 1 ? '' : 's'} pending review`}
        className='text-tertiary hover:text-primary'
      />
      <span className='pointer-events-none absolute -right-0.5 -top-0.5 flex h-4 min-w-4 items-center justify-center rounded-full bg-amber-500 px-1 font-mono text-[10px] font-bold leading-none text-white'>
        {pendingCount > 99 ? '99+' : pendingCount}
      </span>
    </div>
  )
}
