import { ComponentPropsWithoutRef } from 'react'
import { uniqueBy } from 'remeda'
import { useDebounce } from 'use-debounce'

import { LinkIssue, Mention } from '@gitmono/editor/extensions'
import { OrganizationMember, SyncOrganizationMember } from '@gitmono/types/generated'
import { GitCommitIcon, UIText } from '@gitmono/ui'
import { ChatBubblePlusIcon } from '@gitmono/ui/Icons'

import { AppBadge } from '@/components/AppBadge'
import { GuestBadge } from '@/components/GuestBadge'
import { MemberAvatar } from '@/components/MemberAvatar'
import { SuggestionItem, SuggestionRoot, useSuggestionEmpty, useSuggestionQuery } from '@/components/SuggestionList'
import { useGetIssueIssueSuggester } from '@/hooks/issues/useGetIssueIssueSuggester'
import { useGetOauthApplications } from '@/hooks/useGetOauthApplications'
import { useSyncedMembers } from '@/hooks/useSyncedMembers'

type Props = Pick<ComponentPropsWithoutRef<typeof SuggestionRoot>, 'editor'> & {
  defaultMentions?: (OrganizationMember | SyncOrganizationMember)[]
  modal?: boolean
}

export function MentionList({ editor, defaultMentions, modal }: Props) {
  useGetIssueIssueSuggester({ query: '' })

  return (
    <>
      <SuggestionRoot
        modal={modal}
        editor={editor}
        char='@'
        allow={({ state, range }) => {
          const $from = state.doc.resolve(range.from)
          const type = state.schema.nodes[Mention.name]
          const allow = !!$from.parent.type.contentMatch.matchType(type)

          return allow
        }}
        minScore={0.2}
        contentClassName='p-0 max-h-[min(480px,var(--radix-popover-content-available-height))] w-[min(350px,var(--radix-popover-content-available-width))]'
        listClassName='p-1'
      >
        <InnerMentionList editor={editor} defaultMentions={defaultMentions} />
      </SuggestionRoot>
      <SuggestionRoot
        modal={modal}
        editor={editor}
        char='$'
        allow={({ state, range }) => {
          const $from = state.doc.resolve(range.from)
          const type = state.schema.nodes[LinkIssue.name]
          const allow = !!$from.parent.type.contentMatch.matchType(type)

          return allow
        }}
        minScore={0.2}
        contentClassName='p-0 max-h-[min(265px,var(--radix-popover-content-available-height))] w-[min(450px,var(--radix-popover-content-available-width))]'
        listClassName='p-1'
        disableEmptyState={true}
      >
        <InnerIssueList editor={editor} />
      </SuggestionRoot>
    </>
  )
}

function InnerMentionList({ editor, defaultMentions }: Pick<Props, 'editor' | 'defaultMentions'>) {
  const { members: allMembers } = useSyncedMembers()
  const { data: apps = [] } = useGetOauthApplications()
  const isEmptySearch = useSuggestionEmpty()
  const members = isEmptySearch && defaultMentions && defaultMentions.length > 0 ? defaultMentions : allMembers
  const dedupedMembers = uniqueBy(members, (m) => m.id)

  const items = [
    ...dedupedMembers,
    ...apps
      .filter((app) => app.mentionable)
      .map((app) => ({
        id: app.id,
        role: 'app',
        user: {
          id: app.id,
          display_name: app.name,
          username: app.name,
          avatar_urls: app.avatar_urls,
          integration: true,
          notifications_paused: false
        }
      }))
  ]

  return items.map((member) => (
    <SuggestionItem
      key={member.id}
      editor={editor}
      value={member.user.username}
      keywords={[member.user.display_name !== member.user.username ? member.user.display_name : '']}
      // downrank guest members
      scoreModifier={member.role === 'guest' ? 0.3 : 1}
      onSelect={({ editor, range }) =>
        editor.commands.insertMention({
          range,
          id: member.id,
          label: member.user.display_name,
          username: member.user.username,
          role: member.role === 'app' ? 'app' : 'member'
        })
      }
    >
      <MemberAvatar
        displayStatus
        member={{
          user: member.user
        }}
        size='sm'
      />
      <div className='flex flex-1 items-center justify-between'>
        <UIText className='truncate'>{member.user.display_name}</UIText>
        {member.role === 'guest' && <GuestBadge />}
        {member.role === 'app' && <AppBadge />}
      </div>
    </SuggestionItem>
  ))
}

function InnerIssueList({ editor }: Pick<Props, 'editor'>) {
  const query = useSuggestionQuery()
  const [debouncedQuery] = useDebounce(query, 400)

  const { data: issueSuggestions } = useGetIssueIssueSuggester({ query: debouncedQuery })

  if (!issueSuggestions?.data || issueSuggestions.data.length === 0) {
    return (
      <div className='flex items-center gap-2 p-2 text-sm'>
        <UIText>No results</UIText>
      </div>
    )
  }

  return issueSuggestions.data.map((item) => {
    let suggestionType = ''

    switch (item.type) {
      case 'issue_closed':
      case 'issue_open':
        suggestionType = 'issue'
        break
      case 'change_list_closed':
      case 'change_list':
        suggestionType = 'change_list'
        break
      default:
        break
    }

    return (
      <SuggestionItem
        key={item.link}
        editor={editor}
        value={item.link}
        forceMount={true}
        onSelect={({ editor, range }) =>
          editor.commands.insertIssue({
            range,
            id: item.link,
            label: item.link,
            suggestionType: suggestionType
          })
        }
      >
        <span className='h-5 w-5'>{suggestionType === 'change_list' ? <GitCommitIcon /> : <ChatBubblePlusIcon />}</span>
        <div className='flex flex-1 items-center justify-between'>
          <UIText className='max-w-[300px] truncate'>{item.title}</UIText>
          <UIText className='justify-self-end truncate text-sm text-gray-500'>{item.link}</UIText>
        </div>
      </SuggestionItem>
    )
  })
}
