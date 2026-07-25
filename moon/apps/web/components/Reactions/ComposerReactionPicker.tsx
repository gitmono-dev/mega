import { Button, FaceSmilePlusIcon } from '@gitmono/ui'

import { MarkdownEditorRef } from '../MarkdownEditor'
import { SimpleNoteContentRef } from '../SimpleNoteEditor/SimpleNoteContent'
import { ReactionPicker } from './ReactionPicker'

interface ComposerReactionPickerProps {
  open?: boolean
  onOpenChange?: (value: boolean) => void
  editorRef: React.RefObject<MarkdownEditorRef | null> | React.RefObject<SimpleNoteContentRef | null>
  disabled?: boolean
}

export function ComposerReactionPicker({ open, onOpenChange, editorRef, disabled }: ComposerReactionPickerProps) {
  return (
    <ReactionPicker
      open={open}
      onOpenChange={onOpenChange}
      custom
      trigger={
        <Button
          variant='plain'
          type='button'
          iconOnly={<FaceSmilePlusIcon />}
          accessibilityLabel='Add emoji'
          tooltip='Add emoji'
          tooltipShortcut=':'
          disabled={disabled}
        />
      }
      onReactionSelect={(emoji) => editorRef.current?.insertReaction(emoji)}
      onClose={() => editorRef.current?.focus('restore')}
    />
  )
}
