import { useState } from 'react'

import type { UserApprovalStatusRes } from '@gitmono/types'
import { Avatar, Button, Table, TableRow, UIText } from '@gitmono/ui'

import * as SettingsSection from '@/components/SettingsSection'
import { useAdminUserApprovals } from '@/hooks/AccountReview/useAdminUserApprovals'
import { useApproveUser } from '@/hooks/AccountReview/useApproveUser'
import { useRejectUser } from '@/hooks/AccountReview/useRejectUser'

type FilterTab = 'all' | 'pending' | 'approved' | 'rejected'

const FILTER_TABS: { id: FilterTab; label: string }[] = [
  { id: 'pending', label: 'Pending' },
  { id: 'approved', label: 'Approved' },
  { id: 'rejected', label: 'Rejected' },
  { id: 'all', label: 'All' }
]

function statusLabel(status: string) {
  switch (status) {
    case 'pending':
      return 'Pending'
    case 'approved':
      return 'Approved'
    case 'rejected':
      return 'Rejected'
    default:
      return status
  }
}

function statusClassName(status: string) {
  switch (status) {
    case 'pending':
      return 'bg-amber-100 text-amber-800 dark:bg-amber-900/30 dark:text-amber-400'
    case 'approved':
      return 'bg-green-100 text-green-800 dark:bg-green-900/30 dark:text-green-400'
    case 'rejected':
      return 'bg-red-100 text-red-800 dark:bg-red-900/30 dark:text-red-400'
    default:
      return 'bg-neutral-100 text-neutral-800 dark:bg-neutral-900/30 dark:text-neutral-400'
  }
}

function formatDate(unixSeconds: number) {
  return new Date(unixSeconds * 1000).toLocaleString(undefined, {
    year: 'numeric',
    month: 'short',
    day: 'numeric',
    hour: '2-digit',
    minute: '2-digit'
  })
}

export function AccountReviewSection() {
  const [filter, setFilter] = useState<FilterTab>('pending')
  const { data, isPending, isError } = useAdminUserApprovals(filter)
  const approve = useApproveUser()
  const reject = useRejectUser()

  const users = data?.data?.items ?? []
  const pendingCount = filter === 'pending' ? users.length : users.filter((user) => user.status === 'pending').length

  return (
    <SettingsSection.Section className='shadow-sm'>
      <SettingsSection.Header>
        <SettingsSection.Title>Account review</SettingsSection.Title>
      </SettingsSection.Header>

      <SettingsSection.Description>
        Review newly registered users. Only approved users can access the home page.
        {filter === 'pending' && pendingCount > 0 && (
          <>
            <br />
            {pendingCount} user{pendingCount === 1 ? '' : 's'} pending review.
          </>
        )}
      </SettingsSection.Description>

      <SettingsSection.Separator />

      <div className='border-b px-1 pt-1'>
        <nav className='flex'>
          {FILTER_TABS.map((tab) => (
            <button
              key={tab.id}
              type='button'
              onClick={() => setFilter(tab.id)}
              className={`hover:text-primary relative border-none p-3 text-sm transition before:absolute before:inset-x-4 before:bottom-0 before:block before:h-0.5 before:transition hover:before:bg-black/25 dark:hover:before:bg-white/20 ${
                filter === tab.id ? 'text-primary before:!bg-black dark:before:!bg-white' : 'text-tertiary'
              }`}
            >
              {tab.label}
            </button>
          ))}
        </nav>
      </div>

      {isPending ? (
        <SettingsSection.Body>
          <UIText tertiary>Loading…</UIText>
        </SettingsSection.Body>
      ) : isError ? (
        <SettingsSection.Body>
          <UIText tertiary>Unable to load user approvals. Check Mono API connectivity.</UIText>
        </SettingsSection.Body>
      ) : users.length === 0 ? (
        <SettingsSection.Body>
          <UIText tertiary>No users match this filter.</UIText>
        </SettingsSection.Body>
      ) : (
        <div className='flex flex-col'>
          <Table>
            {users.map((user) => (
              <ReviewRow
                key={user.username}
                user={user}
                approving={approve.isPending}
                rejecting={reject.isPending}
                onApprove={() => approve.mutate(user.username)}
                onReject={() => reject.mutate(user.username)}
              />
            ))}
          </Table>
        </div>
      )}
    </SettingsSection.Section>
  )
}

interface ReviewRowProps {
  user: UserApprovalStatusRes
  approving: boolean
  rejecting: boolean
  onApprove: () => void
  onReject: () => void
}

function ReviewRow({ user, approving, rejecting, onApprove, onReject }: ReviewRowProps) {
  const displayName = user.display_name || user.username

  return (
    <TableRow>
      <div className='flex-1 text-sm'>
        <div className='flex items-center'>
          <div className='h-10 w-10 flex-shrink-0'>
            <Avatar name={displayName} size='lg' />
          </div>
          <div className='ml-4 min-w-0'>
            <div className='flex flex-wrap items-center gap-2'>
              <UIText weight='font-medium' selectable>
                {displayName}
              </UIText>
              <span
                className={`inline-flex rounded-full px-2 py-0.5 text-xs font-medium ${statusClassName(user.status)}`}
              >
                {statusLabel(user.status)}
              </span>
            </div>
            <UIText tertiary>
              @{user.username} · {user.email}
            </UIText>
            <UIText tertiary className='text-xs'>
              Registered {formatDate(user.registered_at)}
              {user.reviewed_at ? ` · Reviewed ${formatDate(user.reviewed_at)}` : ''}
              {user.reviewed_by ? ` by @${user.reviewed_by}` : ''}
            </UIText>
          </div>
        </div>
      </div>
      <div className='flex w-full items-center justify-end gap-1.5 sm:w-auto'>
        {user.status !== 'rejected' && (
          <Button onClick={onReject} variant='flat' fullWidth disabled={rejecting || approving}>
            Decline
          </Button>
        )}
        {user.status !== 'approved' && (
          <Button onClick={onApprove} fullWidth variant='important' disabled={approving || rejecting}>
            Approve
          </Button>
        )}
      </div>
    </TableRow>
  )
}
