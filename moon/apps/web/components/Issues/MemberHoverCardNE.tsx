import React, { useState } from 'react'
import * as HoverCard from '@radix-ui/react-hover-card'
import { AnimatePresence, m } from 'framer-motion'

import { OrganizationMember as Member } from '@gitmono/types/generated'
import { ANIMATION_CONSTANTS, Button, ChatBubbleIcon } from '@gitmono/ui'

import { MemberAvatar } from '@/components/MemberAvatar'

// import { useScope } from '@/contexts/scope'

export interface OutPut {
  id: string
  role: string
  created_at: Date
  deactivated: boolean
  is_organization_member: boolean
  user: User
  status: null
}

export interface User {
  id: string
  avatar_url: string
  avatar_urls: AvatarUrls
  cover_photo_url: null
  email: string
  username: string
  display_name: string
  system: boolean
  integration: boolean
  notifications_paused: boolean
  notification_pause_expires_at: null
  timezone: null
  logged_in: boolean
  type_name: string
}

export interface AvatarUrls {
  xs: string
  sm: string
  base: string
  lg: string
  xl: string
  xxl: string
}

export function MemberHovercard({
  member,
  username,
  role,
  children,
  side = 'bottom',
  align = 'start',
  forceOpen = false,
  onMouseOver,
  onMouseOut,
  onBtnHandler
}: {
  member: Member
  username: string
  role?: string
  children?: React.ReactNode
  side?: 'top' | 'bottom' | 'left' | 'right'
  align?: 'start' | 'end' | 'center'
  forceOpen?: boolean
  onMouseOver?: () => void
  onMouseOut?: () => void
  onBtnHandler?: (member: Member) => void
}) {
  // const { scope } = useScope()
  const [open, setOpen] = useState(forceOpen)
  const isIntegration = role === 'app'
  // TODO:need an api to fetch the user info by the username
  // I will pass an obj of member type to handle the problem

  if (!username || isIntegration) return children

  return (
    <HoverCard.Root open={open} onOpenChange={setOpen} openDelay={200} closeDelay={200}>
      <HoverCard.Trigger asChild>{children}</HoverCard.Trigger>

      <AnimatePresence>
        {open && (
          <HoverCard.Portal>
            <HoverCard.Content
              hideWhenDetached
              side={side}
              align={align}
              sideOffset={4}
              collisionPadding={8}
              className='relative'
              onMouseOver={onMouseOver}
              onMouseOut={onMouseOut}
            >
              <m.div
                {...ANIMATION_CONSTANTS}
                className='border-primary-opaque bg-elevated w-[312px] origin-[var(--radix-hover-card-content-transform-origin)] overflow-hidden rounded-lg border shadow max-md:hidden dark:shadow-[0px_0px_0px_0.5px_rgba(0,0,0,1),_0px_4px_4px_rgba(0,0,0,0.24)]'
              >
                {member && (
                  <div>
                    <div className='flex h-6 items-center gap-4 border border-b-gray-300 px-4 py-4 text-sm text-gray-500'>
                      {/* these are fixed, we need fetch the member from backend in this cmp */}
                      <ChatBubbleIcon />
                      <span>{member?.user.email}</span>
                    </div>
                    <div className='flex items-center justify-between px-6 py-6'>
                      {member && <MemberAvatar member={member} size='xl' />}
                      <Button
                        onClick={(e: React.SyntheticEvent) => {
                          e.stopPropagation()
                          e.preventDefault()
                          onBtnHandler?.(member)
                        }}
                      >
                        Follow
                      </Button>
                    </div>
                    <div className='flex flex-col px-6 pb-6 text-sm text-gray-500'>
                      <div>
                        <span className='text-black'>{member.user.username} </span>
                        <span>{member.user.display_name}</span>
                      </div>
                      {/* <span>Stay foolish</span> */}
                      {/* <span>China</span> */}
                      {/* <span>Commit timeline</span> */}
                    </div>
                  </div>
                )}
              </m.div>
            </HoverCard.Content>
          </HoverCard.Portal>
        )}
      </AnimatePresence>
    </HoverCard.Root>
  )
}
