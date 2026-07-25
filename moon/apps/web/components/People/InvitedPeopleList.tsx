import { useMemo, useState } from 'react'

import { OrganizationInvitation } from '@gitmono/types/generated'
import { Avatar } from '@gitmono/ui/Avatar'
import { Badge } from '@gitmono/ui/Badge'
import { Button } from '@gitmono/ui/Button'
import { Link } from '@gitmono/ui/Link'
import { UIText } from '@gitmono/ui/Text'
import { cn } from '@gitmono/ui/utils'

import { IndexPageLoading } from '@/components/IndexPages/components'
import { InfiniteLoader } from '@/components/InfiniteLoader'
import { PeopleIndexEmptyState } from '@/components/People/PeopleIndexEmptyState'
import { PEOPLE_LIST_NAVIGATION_CONTAINER_ID } from '@/components/People/PeopleList'
import { RemoveInvitationDialog } from '@/components/People/RemoveInvitationDialog'
import { useCanHover } from '@/hooks/useCanHover'
import { useGetOrganizationInvitations } from '@/hooks/useGetOrganizationInvitations'
import { useListNavigation } from '@/hooks/useListNavigation'
import { useViewerIsAdmin } from '@/hooks/useViewerIsAdmin'
import { flattenInfiniteData } from '@/utils/flattenInfiniteData'

import { getInvitationDOMId } from './PeopleIndex'

export function InvitedPeopleList() {
  const getOrganizationInvitations = useGetOrganizationInvitations()
  const invitations = useMemo(
    () => flattenInfiniteData(getOrganizationInvitations.data),
    [getOrganizationInvitations.data]
  )
  const isInitialLoading = getOrganizationInvitations.isLoading
  const hasInvitations = !!invitations?.length

  const { selectItem } = useListNavigation({
    items: invitations || [],
    getItemDOMId: getInvitationDOMId
  })

  return (
    <>
      {isInitialLoading && <IndexPageLoading />}
      {!isInitialLoading && !hasInvitations && <PeopleIndexEmptyState />}
      {!isInitialLoading && hasInvitations && (
        <ul id={PEOPLE_LIST_NAVIGATION_CONTAINER_ID} className='-mx-2 flex flex-col gap-px'>
          {invitations.map((invitation, itemIndex) => (
            <PeopleIndexInvitationRow
              key={invitation.id}
              invitation={invitation}
              onFocus={() => selectItem({ itemIndex })}
              onPointerMove={() => selectItem({ itemIndex, scroll: false })}
            />
          ))}
        </ul>
      )}

      <InfiniteLoader
        hasNextPage={getOrganizationInvitations.hasNextPage}
        isError={getOrganizationInvitations.isError}
        isFetching={getOrganizationInvitations.isFetching}
        isFetchingNextPage={getOrganizationInvitations.isFetchingNextPage}
        fetchNextPage={getOrganizationInvitations.fetchNextPage}
      />
    </>
  )
}

function PeopleIndexInvitationRow({
  invitation,
  onFocus,
  onPointerMove
}: {
  invitation: OrganizationInvitation
  onFocus?: React.FocusEventHandler<HTMLAnchorElement>
  onPointerMove?: React.PointerEventHandler<HTMLAnchorElement>
}) {
  const [dialogIsOpen, setDialogIsOpen] = useState(false)
  const canHover = useCanHover()
  const viewerIsAdmin = useViewerIsAdmin()

  return (
    <li
      className={cn(
        'group relative flex items-center gap-3 rounded-md py-2 pr-1.5 pl-3',
        'data-[state="open"]:bg-tertiary',
        {
          'focus-within:bg-tertiary': canHover
        }
      )}
    >
      <Link
        href='#'
        id={getInvitationDOMId(invitation)}
        onFocus={onFocus}
        onPointerMove={onPointerMove}
        className='absolute inset-0 z-0 rounded-lg focus:ring-0'
      />

      <div className='flex flex-1 items-center gap-3'>
        <Avatar name={invitation.email} size='sm' />

        <div className='line-clamp-1 flex min-w-0 flex-1 items-center gap-1.5'>
          <UIText weight='font-medium' className='line-clamp-1 shrink'>
            {invitation.email}
          </UIText>
          <Badge>{invitation.role}</Badge>
        </div>
      </div>

      {viewerIsAdmin && (
        <div className='opacity-0 group-focus-within:opacity-100 group-hover:opacity-100'>
          <RemoveInvitationDialog invitation={invitation} onOpenChange={setDialogIsOpen} open={dialogIsOpen} />
          <Button size='sm' variant='plain' onClick={() => setDialogIsOpen(true)}>
            Remove invitation
          </Button>
        </div>
      )}
    </li>
  )
}
