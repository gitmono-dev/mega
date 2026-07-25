import { Post } from '@gitmono/types'
import { UIText } from '@gitmono/ui'

import { DisplayType } from '.'

interface InlinePostTitleProps {
  post: Post
  display: DisplayType
}

export function InlinePostTitle({ post, display }: InlinePostTitleProps) {
  if (!post.title) return null

  // noop pre-title posts or posts where the user formatted the title in the description
  // this avoids showing the title twice
  if (post.is_title_from_description) return null

  if (display === 'page') {
    return (
      <UIText
        selectable
        element='h2'
        className='text-primary break-anywhere mt-4 -mb-2 text-[22px] leading-snug font-bold'
      >
        {post.title}
      </UIText>
    )
  }

  if (display === 'preview') {
    return (
      <UIText selectable weight='font-semibold' className='mt-1 -mb-3 leading-snug' size='text-base'>
        {post.title}
      </UIText>
    )
  }

  if (display === 'feed') {
    return (
      <UIText selectable className='text-primary mt-1 -mb-4 text-xl leading-snug font-semibold'>
        {post.title}
      </UIText>
    )
  }

  return null
}
