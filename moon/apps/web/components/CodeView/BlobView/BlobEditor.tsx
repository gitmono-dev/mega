'use client'

import React, { useCallback, useMemo, useRef, useState } from 'react'
import { parsePatchFiles, type FileDiffMetadata } from '@pierre/diffs'
import { FileDiff } from '@pierre/diffs/react'
import toast from 'react-hot-toast'

import { Button } from '@gitmono/ui/Button'
import { Dialog } from '@gitmono/ui/Dialog'

import { useDiffPreview } from '@/hooks/useDiffPreview'
import { useGetCurrentUser } from '@/hooks/useGetCurrentUser'
import { useUpdateBlob } from '@/hooks/useUpdateBlob'
import { getLanguageForFile } from '@/utils/shikiLanguageFallback'

import { MegaCedarAdminPicker } from './MegaCedarAdminPicker'

interface BlobEditorProps {
  fileContent: string
  filePath: string
  fileName: string
  onCancel: () => void
}

type ViewMode = 'edit' | 'preview'

function isMegaCedarJsonFile(name: string, path: string) {
  return name === '.mega_cedar.json' || path.endsWith('/.mega_cedar.json') || path === '.mega_cedar.json'
}

export default function BlobEditor({ fileContent, filePath, fileName, onCancel }: BlobEditorProps) {
  const { data: currentUser } = useGetCurrentUser()

  const updateBlobMutation = useUpdateBlob()
  const diffPreviewMutation = useDiffPreview()
  const [content, setContent] = useState(fileContent)

  const [editedFileName, setEditedFileName] = useState(fileName)
  const [commitMessage, setCommitMessage] = useState(`Update ${fileName}`)

  const [viewMode, setViewMode] = useState<ViewMode>('edit')

  const [skipBuild, setSkipBuild] = useState(false)

  const [diffResult, setDiffResult] = useState<any>(null)
  const [fileDiffMetadata, setFileDiffMetadata] = useState<FileDiffMetadata | null>(null)

  const [isCommitDialogOpen, setIsCommitDialogOpen] = useState(false)

  const lineNumbersRef = useRef<HTMLDivElement>(null)
  const textareaRef = useRef<HTMLTextAreaElement>(null)

  const contentLines = useMemo(() => content.split('\n'), [content])

  const hasChanges = useMemo(
    () => content !== fileContent || editedFileName !== fileName,
    [content, fileContent, editedFileName, fileName]
  )

  const pathSegments = useMemo(() => {
    const segments = filePath.split('/').filter(Boolean)

    return segments.slice(0, -1)
  }, [filePath])

  const fullEditedPath = useMemo(() => {
    const dir = pathSegments.join('/')

    return dir ? `${dir}/${editedFileName}` : editedFileName
  }, [pathSegments, editedFileName])

  const showCedarAdminPicker = isMegaCedarJsonFile(editedFileName, fullEditedPath)

  const detectedLanguage = useMemo(() => getLanguageForFile(editedFileName), [editedFileName])

  const handleCedarContentGenerated = useCallback((generated: string) => {
    setContent(generated)
    setDiffResult(null)
    setFileDiffMetadata(null)
  }, [])

  const handlePreviewClick = useCallback(async () => {
    setViewMode('preview')

    if (!hasChanges) {
      return
    }

    if (!diffResult) {
      try {
        const result = await diffPreviewMutation.mutateAsync({
          path: filePath,
          content: content
        })

        setDiffResult(result)

        if (result?.data?.data) {
          const patches = parsePatchFiles(result.data.data)

          if (patches.length > 0 && patches[0].files.length > 0) {
            let metadata = patches[0].files[0]

            if (!metadata.name) {
              metadata = { ...metadata, name: editedFileName }
            }
            metadata = { ...metadata, lang: detectedLanguage as any }

            setFileDiffMetadata(metadata)
          }
        }
      } catch (error: any) {
        toast.error(error?.message)
      }
    }
  }, [content, filePath, hasChanges, diffPreviewMutation, diffResult, detectedLanguage, editedFileName])

  const handleCommitClick = useCallback(() => {
    if (!hasChanges) {
      return
    }
    setIsCommitDialogOpen(true)
    setSkipBuild(false)
  }, [hasChanges])

  const handleSave = useCallback(async () => {
    const isRename = editedFileName.trim() !== fileName
    const trimmedName = editedFileName.trim()

    if (!trimmedName) {
      toast.error('File name cannot be empty')
      return
    }

    const destinationPath = pathSegments.length
      ? `${pathSegments.join('/')}/${trimmedName}`
      : trimmedName

    try {
      await updateBlobMutation.mutateAsync({
        path: filePath,
        new_path: isRename ? destinationPath : undefined,
        content: content,
        commit_message: commitMessage,
        author_email: currentUser?.email,
        author_username: currentUser?.username,
        mode: 'force_create',
        skip_build: skipBuild
      })

      toast.success(isRename ? 'Rename submitted successfully' : 'Changes submitted successfully')
      setIsCommitDialogOpen(false)
      onCancel()
    } catch (error: any) {
      const msg =
        error?.message ||
        error?.response?.data?.message ||
        'Submit failed. Please try again.'

      toast.error(msg)
    }
  }, [
    updateBlobMutation,
    editedFileName,
    fileName,
    pathSegments,
    filePath,
    content,
    commitMessage,
    currentUser?.email,
    currentUser?.username,
    skipBuild,
    onCancel
  ])

  const handleDialogClose = useCallback((open: boolean) => {
    setIsCommitDialogOpen(open)
    if (!open) {
      setSkipBuild(false)
    }
  }, [])

  const handleTextareaChange = (e: React.ChangeEvent<HTMLTextAreaElement>) => {
    setContent(e.target.value)

    setDiffResult(null)
    setFileDiffMetadata(null)
  }

  const handleScroll = useCallback(() => {
    if (textareaRef.current && lineNumbersRef.current) {
      lineNumbersRef.current.scrollTop = textareaRef.current.scrollTop
    }
  }, [])

  const renderEditView = () => {
    return (
      <div className='flex h-full w-full overflow-hidden font-mono text-sm leading-6'>
        <div
          ref={lineNumbersRef}
          className='border-primary bg-secondary text-quaternary h-full select-none overflow-hidden border-r px-4 text-right'
          style={{ flexShrink: 0 }}
        >
          {contentLines.map((_, index) => (
            // eslint-disable-next-line react/no-array-index-key
            <div key={index} className='leading-6'>
              {index + 1}
            </div>
          ))}
        </div>

        <div className='flex-1 overflow-hidden'>
          <textarea
            ref={textareaRef}
            value={content}
            onChange={handleTextareaChange}
            onScroll={handleScroll}
            className='h-full w-full resize-none overflow-auto border-0 bg-transparent p-0 pl-4 font-mono text-sm leading-6 focus:outline-none'
            spellCheck={false}
            style={{ tabSize: 2 }}
          />
        </div>
      </div>
    )
  }

  const renderPreviewView = () => {
    if (!hasChanges) {
      return (
        <div className='text-tertiary flex h-full items-center justify-center'>
          <div className='text-center'>
            <p className='text-lg font-medium'>No changes</p>
            <p className='mt-2 text-sm'>Please edit the file content first</p>
          </div>
        </div>
      )
    }

    if (diffPreviewMutation.isPending) {
      return (
        <div className='text-tertiary flex h-full items-center justify-center'>
          <div className='text-center'>
            <p className='text-lg font-medium'>Loading...</p>
            <p className='mt-2 text-sm'>Generating diff preview</p>
          </div>
        </div>
      )
    }

    if (!fileDiffMetadata) {
      return (
        <div className='text-tertiary flex h-full items-center justify-center'>
          <div className='text-center'>
            <p className='text-lg font-medium'>Failed to load diff preview</p>
            <p className='mt-2 text-sm'>Please try again</p>
          </div>
        </div>
      )
    }

    return (
      <div className='h-full overflow-auto'>
        <FileDiff
          fileDiff={fileDiffMetadata}
          options={{
            theme: { dark: 'min-dark', light: 'min-light' },
            diffStyle: 'split',
            diffIndicators: 'classic',
            overflow: 'wrap',
            disableFileHeader: true
          }}
          style={{ '--diffs-font-size': '14px' } as React.CSSProperties}
        />
      </div>
    )
  }

  return (
    <div className='flex min-h-0 w-full flex-1 flex-col gap-2'>
      <div className='flex min-h-14 w-full flex-shrink-0 items-center justify-between px-2'>
        <div className='flex max-w-[900px] flex-wrap items-center gap-x-1 gap-y-2 text-gray-700'>
          {pathSegments.map((seg, i) => (
            // eslint-disable-next-line react/no-array-index-key
            <React.Fragment key={i}>
              <span className='font-medium text-blue-600'>{seg}</span>
              <span>/</span>
            </React.Fragment>
          ))}

          <input
            type='text'
            value={editedFileName}
            onChange={(e) => setEditedFileName(e.target.value)}
            placeholder='fileName'
            className='min-w-[180px] rounded border border-gray-300 px-2 py-1 text-sm font-medium text-gray-900 outline-none focus:border-blue-500 focus:ring-2 focus:ring-blue-500'
            disabled={updateBlobMutation.isPending}
          />
        </div>

        <div className='flex gap-2'>
          <Button variant='flat' onClick={onCancel} disabled={updateBlobMutation.isPending}>
            Cancel changes
          </Button>
          <Button onClick={handleCommitClick} disabled={updateBlobMutation.isPending || !hasChanges}>
            Commit changes
          </Button>
        </div>
      </div>

      <div className='flex min-h-0 w-full flex-1 flex-col rounded-xl border border-[#bec7ce]'>
        <div className='flex h-14 w-full flex-shrink-0 items-center rounded-t-xl border-b border-[#d0d9e0] bg-[#f9fbfd] px-4'>
          <div className='inline-flex rounded-md border border-gray-300 bg-white'>
            <button
              onClick={() => setViewMode('edit')}
              className={`rounded-l-md px-4 py-2 text-sm font-medium ${
                viewMode === 'edit' ? 'bg-gray-100 text-gray-900' : 'bg-white text-gray-500 hover:text-gray-700'
              }`}
            >
              Edit
            </button>
            <button
              onClick={handlePreviewClick}
              className={`rounded-r-md px-4 py-2 text-sm font-medium ${
                viewMode === 'preview' ? 'bg-gray-100 text-gray-900' : 'bg-white text-gray-500 hover:text-gray-700'
              }`}
            >
              Preview
            </button>
          </div>
        </div>

        {showCedarAdminPicker && viewMode === 'edit' && (
          <MegaCedarAdminPicker
            fileContent={fileContent}
            onContentGenerated={handleCedarContentGenerated}
            disabled={updateBlobMutation.isPending}
          />
        )}

        <div className='min-h-0 flex-1 overflow-hidden'>
          {viewMode === 'edit' && renderEditView()}
          {viewMode === 'preview' && renderPreviewView()}
        </div>
      </div>

      <Dialog.Root open={isCommitDialogOpen} onOpenChange={handleDialogClose}>
        <Dialog.Content>
          <Dialog.CloseButton />
          <Dialog.Header>
            <Dialog.Title>Commit changes</Dialog.Title>
          </Dialog.Header>

          <div className='flex flex-col gap-4 py-4'>
            <div className='flex flex-col gap-2'>
              <label className='text-sm font-medium text-gray-700'>Commit message *</label>
              <input
                type='text'
                value={commitMessage}
                onChange={(e) => setCommitMessage(e.target.value)}
                placeholder={`update ${fileName}`}
                className='w-full rounded-md border border-gray-300 px-3 py-2 text-sm focus:border-blue-500 focus:outline-none focus:ring-1 focus:ring-blue-500'
                disabled={updateBlobMutation.isPending}
              />
            </div>

            <div className='flex items-center gap-2'>
              <input
                type='checkbox'
                id='skipBuild_editor'
                checked={skipBuild}
                onChange={(e) => setSkipBuild(e.target.checked)}
                className='h-4 w-4 rounded border-gray-300 text-blue-600 focus:ring-blue-500'
                disabled={updateBlobMutation.isPending}
              />
              <label htmlFor='skipBuild_editor' className='text-sm font-medium text-gray-700'>
                Skip automatic build after commit
              </label>
            </div>
          </div>

          <Dialog.Footer>
            <Dialog.TrailingActions>
              <Button variant='flat' onClick={() => handleDialogClose(false)} disabled={updateBlobMutation.isPending}>
                Cancel
              </Button>
              <Button onClick={handleSave} disabled={updateBlobMutation.isPending || !commitMessage.trim()}>
                {updateBlobMutation.isPending ? 'Submitting...' : 'Confirm submission'}
              </Button>
            </Dialog.TrailingActions>
          </Dialog.Footer>
        </Dialog.Content>
      </Dialog.Root>
    </div>
  )
}
