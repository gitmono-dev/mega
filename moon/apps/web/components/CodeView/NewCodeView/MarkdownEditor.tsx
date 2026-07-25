'use client'

import { useEffect, useState } from 'react'
import { EditorContent, useEditor } from '@tiptap/react'
import Markdown from 'react-markdown'

import { StarterKit } from '@gitmono/editor/extensions'

interface MarkdownEditorProps {
  contentState: [string, React.Dispatch<React.SetStateAction<string>>]
  disabled?: boolean
}
export default function MarkdownEditor({ contentState, disabled = false }: MarkdownEditorProps) {
  const [content, setContent] = contentState
  const [lineCount, setLineCount] = useState(1)
  const [isPreview, setIsPreview] = useState(false)

  const textEditor = useEditor({
    extensions: StarterKit(),
    editable: !disabled,
    onUpdate: ({ editor }) => {
      const text = editor.getText().replace(/\n\n/g, '\n')

      setContent(text)
      setLineCount(text.split('\n').length || 1)
    },
    editorProps: {
      attributes: {
        class: 'max-w-full focus:outline-hidden font-mono text-sm leading-6 h-full'
      }
    }
  })

  useEffect(() => {
    if (textEditor) {
      textEditor.setEditable(!disabled)
    }
  }, [disabled, textEditor])

  const lineNumbers = Array.from({ length: lineCount }, (_, i) => i + 1)

  return (
    <div className={`border-primary flex h-full w-full flex-col rounded-xl border ${disabled ? 'opacity-60' : ''}`}>
      <div className='border-b-primary bg-secondary flex h-14 w-full items-center rounded-t-xl border p-4'>
        <div className='border-primary bg-primary inline-flex rounded-md border'>
          <button
            onClick={() => setIsPreview(false)}
            disabled={disabled}
            className={`rounded-l-md px-4 py-2 text-sm font-medium transition-colors ${
              !isPreview ? 'bg-tertiary text-primary' : 'bg-primary text-tertiary hover:text-secondary'
            } ${disabled ? 'cursor-not-allowed opacity-50' : ''}`}
          >
            Edit
          </button>
          <button
            onClick={() => setIsPreview(true)}
            disabled={disabled}
            className={`rounded-r-md px-4 py-2 text-sm font-medium transition-colors ${
              isPreview ? 'bg-tertiary text-primary' : 'bg-primary text-tertiary hover:text-secondary'
            } ${disabled ? 'cursor-not-allowed opacity-50' : ''}`}
          >
            Preview
          </button>
        </div>
        {disabled && <span className='text-tertiary ml-4 text-sm italic'>Folders don&apos;t need content</span>}
      </div>

      <div className={`flex flex-1 overflow-x-auto ${disabled ? 'bg-tertiary' : ''}`}>
        {isPreview ? (
          <div className='prose h-full w-full max-w-none overflow-y-auto px-8 pt-6 pb-4'>
            <Markdown>{content}</Markdown>
          </div>
        ) : (
          <div className='flex h-full w-full font-mono text-sm leading-6'>
            <div
              className='border-primary bg-secondary text-quaternary flex flex-col rounded-bl-xl border-r py-2 pr-4 text-right select-none'
              style={{ paddingLeft: '1rem' }}
            >
              {lineNumbers.map((n) => (
                <div key={n} className='h-6'>
                  {n}
                </div>
              ))}
            </div>
            <div
              className={`flex h-full flex-1 flex-col ${disabled ? 'cursor-not-allowed' : 'cursor-text'}`}
              onClick={() => {
                if (!disabled && textEditor) {
                  textEditor.commands.focus()
                }
              }}
            >
              <EditorContent
                editor={textEditor}
                className='h-full w-full [&_.ProseMirror]:h-full [&_.ProseMirror]:px-4 [&_.ProseMirror]:py-2 [&_.ProseMirror]:outline-hidden [&_.ProseMirror_p]:m-0 [&_.ProseMirror_p]:h-6'
              />
            </div>
          </div>
        )}
      </div>
    </div>
  )
}
