import { Button } from '@gitmono/ui/Button'

import { MemberAvatar } from '@/components/MemberAvatar'
import { ProfileDropdown } from '@/components/NavigationSidebar/ProfileDropdown'
import { SidebarAppVersion } from '@/components/Sidebar/SidebarAppVersion'
import { StatusPicker } from '@/components/StatusPicker'
import { useScope } from '@/contexts/scope'
import { useGetCurrentUser } from '@/hooks/useGetCurrentUser'

export function SidebarProfile() {
  const { scope } = useScope()
  const { data: currentUser } = useGetCurrentUser()

  return (
    <div className='flex items-center gap-px'>
      <div className='flex items-center gap-1'>
        <ProfileDropdown
          trigger={
            <Button
              round
              variant='plain'
              href={`/${scope}/people/${currentUser?.username}`}
              accessibilityLabel='Profile and settings'
              tooltip='Profile and settings'
              iconOnly={currentUser && <MemberAvatar displayStatus member={{ user: currentUser }} size='sm' />}
            />
          }
          align='start'
          side='top'
        />
        <StatusPicker />
      </div>

      <div className='flex-1' />

      <SidebarAppVersion />
    </div>
  )
}
