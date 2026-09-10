'use client'

import { useMemo, useState } from 'react'
import { toast } from 'react-hot-toast'

import { MONO_API_URL } from '@gitmono/config'
import { UIText } from '@gitmono/ui'
import { Button } from '@gitmono/ui/Button'
import { Dialog } from '@gitmono/ui/Dialog'
import { TextField } from '@gitmono/ui/TextField'

import { putWithProgress, resolveMonoUploadUrl, sha256HexOfFile } from '@/hooks/OrionClient/orionImageUpload'
import { usePresignOrionImage } from '@/hooks/OrionClient/usePresignOrionImage'
import { useRegisterOrionImage } from '@/hooks/OrionClient/useRegisterOrionImage'

type ImageInfoSidecar = {
  built_at?: string
  rust?: string
  buck2?: string
  python?: string
  kernel?: string
}

type UploadPhase = 'idle' | 'hashing' | 'presigning' | 'uploading_image' | 'uploading_info' | 'registering'

interface UploadOrionImageDialogProps {
  open: boolean
  onOpenChange: (open: boolean) => void
}

function defaultImageName(fileName: string) {
  const base = fileName.replace(/\.qcow2$/i, '').trim()

  return base || 'debian-13-buck2'
}

function phaseLabel(phase: UploadPhase, progress: number): string {
  const pct = Math.round(progress * 100)

  switch (phase) {
    case 'hashing':
      return `Hashing image… ${pct}%`
    case 'presigning':
      return 'Requesting upload URL…'
    case 'uploading_image':
      return `Uploading image… ${pct}%`
    case 'uploading_info':
      return `Uploading image-info.json… ${pct}%`
    case 'registering':
      return 'Registering catalog entry…'
    default:
      return ''
  }
}

export function UploadOrionImageDialog({ open, onOpenChange }: UploadOrionImageDialogProps) {
  const [qcow2File, setQcow2File] = useState<File | null>(null)
  const [infoFile, setInfoFile] = useState<File | null>(null)
  const [imageName, setImageName] = useState('')
  const [label, setLabel] = useState('')
  const [phase, setPhase] = useState<UploadPhase>('idle')
  const [progress, setProgress] = useState(0)

  const { mutateAsync: presign } = usePresignOrionImage()
  const { mutateAsync: registerImage } = useRegisterOrionImage()

  const busy = phase !== 'idle'
  const statusText = useMemo(() => phaseLabel(phase, progress), [phase, progress])

  const reset = () => {
    setQcow2File(null)
    setInfoFile(null)
    setImageName('')
    setLabel('')
    setPhase('idle')
    setProgress(0)
  }

  const onUpload = async () => {
    if (!qcow2File || busy) return

    try {
      setPhase('hashing')
      setProgress(0)
      const hex = await sha256HexOfFile(qcow2File, setProgress)
      const digest = `sha256:${hex}`
      const resolvedName = imageName.trim() || defaultImageName(qcow2File.name)

      let infoMeta: ImageInfoSidecar = {}

      if (infoFile) {
        try {
          infoMeta = JSON.parse(await infoFile.text()) as ImageInfoSidecar
        } catch {
          throw new Error('image-info.json is not valid JSON')
        }
      }

      setPhase('presigning')
      setProgress(0)
      const urls = await presign({
        digest,
        image_name: resolvedName,
        with_info: Boolean(infoFile)
      })

      setPhase('uploading_image')
      setProgress(0)
      await putWithProgress(resolveMonoUploadUrl(urls.qcow2_put_url, MONO_API_URL), qcow2File, setProgress, {
        withCredentials: true
      })

      if (infoFile && urls.info_put_url) {
        setPhase('uploading_info')
        setProgress(0)
        await putWithProgress(resolveMonoUploadUrl(urls.info_put_url, MONO_API_URL), infoFile, setProgress, {
          withCredentials: true
        })
      }

      setPhase('registering')
      setProgress(0)
      await registerImage({
        digest,
        object_key: urls.object_key,
        info_object_key: urls.info_object_key ?? null,
        image_name: resolvedName,
        built_at: infoMeta.built_at ?? null,
        rust: infoMeta.rust ?? null,
        buck2: infoMeta.buck2 ?? null,
        python: infoMeta.python ?? null,
        kernel: infoMeta.kernel ?? null,
        size_bytes: qcow2File.size,
        label: label.trim() || null
      })

      reset()
      onOpenChange(false)
    } catch (error) {
      const message = error instanceof Error ? error.message : 'Upload failed'

      toast.error(message)
      setPhase('idle')
      setProgress(0)
    }
  }

  return (
    <Dialog.Root
      open={open}
      onOpenChange={(next) => {
        if (busy) return
        if (!next) reset()
        onOpenChange(next)
      }}
      size='lg'
    >
      <Dialog.Header>
        <Dialog.Title>Upload image</Dialog.Title>
        <Dialog.Description>
          Upload a qcow2 to object storage and register it in the Orion catalog. Optionally include image-info.json for
          toolchain metadata.
        </Dialog.Description>
      </Dialog.Header>

      <Dialog.Content className='flex flex-col gap-4'>
        <label className='flex flex-col gap-1.5'>
          <UIText size='text-sm' weight='font-medium'>
            Image file (.qcow2)
          </UIText>
          <input
            type='file'
            accept='.qcow2,application/octet-stream'
            disabled={busy}
            onChange={(e) => {
              const file = e.target.files?.[0] ?? null

              setQcow2File(file)
              if (file && !imageName.trim()) {
                setImageName(defaultImageName(file.name))
              }
            }}
          />
          {qcow2File ? (
            <UIText size='text-xs' color='text-muted'>
              {qcow2File.name} · {(qcow2File.size / (1024 * 1024)).toFixed(1)} MiB
            </UIText>
          ) : null}
        </label>

        <label className='flex flex-col gap-1.5'>
          <UIText size='text-sm' weight='font-medium'>
            image-info.json (optional)
          </UIText>
          <input
            type='file'
            accept='.json,application/json'
            disabled={busy}
            onChange={(e) => setInfoFile(e.target.files?.[0] ?? null)}
          />
          {infoFile ? (
            <UIText size='text-xs' color='text-muted'>
              {infoFile.name}
            </UIText>
          ) : null}
        </label>

        <TextField
          label='Image name'
          placeholder='debian-13-buck2'
          value={imageName}
          onChange={setImageName}
          disabled={busy}
        />

        <TextField
          label='Label (optional)'
          placeholder='release tag or note'
          value={label}
          onChange={setLabel}
          disabled={busy}
        />

        {statusText ? (
          <UIText size='text-sm' color='text-muted'>
            {statusText}
          </UIText>
        ) : null}
      </Dialog.Content>

      <Dialog.Footer>
        <Dialog.TrailingActions>
          <Button
            variant='flat'
            disabled={busy}
            onClick={() => {
              reset()
              onOpenChange(false)
            }}
          >
            Cancel
          </Button>
          <Button variant='primary' disabled={!qcow2File} loading={busy} onClick={() => void onUpload()}>
            Upload
          </Button>
        </Dialog.TrailingActions>
      </Dialog.Footer>
    </Dialog.Root>
  )
}
