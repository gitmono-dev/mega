import { useEffect, useMemo } from 'react'
import { EditorOptions, ReactNodeViewRenderer, useEditor } from '@tiptap/react'

import { ActiveEditorComment, BlurAtTopOptions, getNoteExtensions, PostNoteAttachmentOptions } from '@gitmono/editor'

import { InlineResourceMentionRenderer } from '@/components/InlineResourceMentionRenderer'
import { useControlClickLink } from '@/components/MarkdownEditor/ControlClickLink'
import { LinkUnfurlRenderer } from '@/components/Post/LinkUnfurlRenderer'
import { MediaGalleryRenderer } from '@/components/Post/MediaGalleryRenderer'
import { NoteAttachmentRenderer } from '@/components/Post/Notes/Attachments/NoteAttachmentRenderer'
import { DragAndDrop } from '@/components/Post/Notes/DragAndDrop'
import { InlineRelativeTimeRenderer } from '@/components/RichTextRenderer/handlers/RelativeTime'
import { useCurrentUserOrOrganizationHasFeature } from '@/hooks/useCurrentUserOrOrganizationHasFeature'
import { notEmpty } from '@/utils/notEmpty'

interface SimpleNoteEditorOptions {
  content: string
  onOpenAttachment?: PostNoteAttachmentOptions['onOpenAttachment']
  autofocus?: boolean
  editable?: 'all' | 'viewer' | 'none'
  editorProps?: EditorOptions['editorProps']
  onHoverComment?(comment: ActiveEditorComment | null): void
  onActiveComment?(comment: ActiveEditorComment | null): void
  onBlurAtTop?: BlurAtTopOptions['onBlur']
}

export function useSimpleNoteEditor({
  content,
  autofocus,
  editable,
  editorProps,
  onHoverComment,
  onActiveComment,
  onOpenAttachment,
  onBlurAtTop
}: SimpleNoteEditorOptions) {
  const linkOptions = useControlClickLink()
  const hasRelativeTime = useCurrentUserOrOrganizationHasFeature('relative_time')

  const extensions = useMemo(() => {
    return [
      ...getNoteExtensions({
        history: {
          enabled: true
        },
        dropcursor: {
          class: 'text-blue-500',
          width: 2
        },
        link: linkOptions,
        linkUnfurl: {
          addNodeView() {
            return ReactNodeViewRenderer(LinkUnfurlRenderer)
          }
        },
        taskItem: {
          canEdit() {
            return editable !== 'none'
          },
          onReadOnlyChecked() {
            return editable === 'viewer'
          }
        },
        postNoteAttachment: {
          onOpenAttachment,
          addNodeView() {
            return ReactNodeViewRenderer(NoteAttachmentRenderer)
          }
        },
        mediaGallery: {
          onOpenAttachment,
          addNodeView() {
            return ReactNodeViewRenderer(MediaGalleryRenderer)
          }
        },
        resourceMention: {
          addNodeView() {
            return ReactNodeViewRenderer(InlineResourceMentionRenderer, { contentDOMElementTag: 'span' })
          }
        },
        comment: {
          enabled: true,
          onCommentHovered: onHoverComment,
          onCommentActivated: onActiveComment
        },
        codeBlockHighlighted: {
          highlight: true
        },
        blurAtTop: {
          enabled: !!onBlurAtTop,
          onBlur: onBlurAtTop
        },
        relativeTime: {
          disabled: !hasRelativeTime,
          addNodeView() {
            return ReactNodeViewRenderer(InlineRelativeTimeRenderer, { contentDOMElementTag: 'span' })
          }
        }
      }),
      DragAndDrop
    ].filter(notEmpty)
  }, [editable, linkOptions, onActiveComment, onBlurAtTop, onHoverComment, onOpenAttachment, hasRelativeTime])

  const allEditable = editable === 'all'

  const editor = useEditor(
    {
      immediatelyRender: true,
      shouldRerenderOnTransaction: false,
      editorProps: {
        attributes: {
          class: 'new-posts prose select-text focus:outline-hidden w-full relative note min-w-full px-4',
          style: "overflow-anchor: ''"
        },
        ...editorProps
      },
      extensions,
      autofocus: !!autofocus,
      content,
      editable: allEditable
    },
    [extensions]
  )

  useEffect(() => {
    if (editor.isEditable !== allEditable) {
      editor.setEditable(allEditable)
    }
  }, [editor, allEditable])

  return editor
}
