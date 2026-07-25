import { PropsWithChildren } from 'react'
import { ChecklistIcon, CommentDiscussionIcon, FileDiffIcon } from '@primer/octicons-react'
import { UnderlineNav } from '@primer/react'
import { useAtom } from 'jotai'

import { tabAtom } from '../ClView/components/Checks/cpns/store'

export const TabLayout = ({ children }: PropsWithChildren) => {
  const [tab, setTab] = useAtom(tabAtom)

  return (
    <>
      <UnderlineNav aria-label='Change list sections' hideIconsBreakpoint={null}>
        <UnderlineNav.Item
          aria-current={tab === 'conversation' ? 'page' : undefined}
          onSelect={(event) => {
            event.preventDefault()
            setTab('conversation')
          }}
          icon={CommentDiscussionIcon}
        >
          Conversation
        </UnderlineNav.Item>
        <UnderlineNav.Item
          aria-current={tab === 'check' ? 'page' : undefined}
          onSelect={(event) => {
            event.preventDefault()
            setTab('check')
          }}
          icon={ChecklistIcon}
        >
          Checks
        </UnderlineNav.Item>
        <UnderlineNav.Item
          aria-current={tab === 'filechange' ? 'page' : undefined}
          onSelect={(event) => {
            event.preventDefault()
            setTab('filechange')
          }}
          icon={FileDiffIcon}
        >
          Files Changed
        </UnderlineNav.Item>
      </UnderlineNav>
      {children}
    </>
  )
}
