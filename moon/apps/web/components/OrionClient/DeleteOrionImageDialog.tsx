'use client'

import { Button } from '@gitmono/ui/Button'
import { Dialog } from '@gitmono/ui/Dialog'

import { useDeleteOrionImage } from '@/hooks/OrionClient/useDeleteOrionImage'
import type { OrionVmImage } from '@/hooks/OrionClient/useGetOrionImages'

function shortDigest(digest?: string | null) {
  if (!digest) return '—'
  const hex = digest.replace(/^sha256:/, '').replace(/^sha512:/, '')

  return hex.length > 12 ? `${hex.slice(0, 12)}…` : hex
}

interface DeleteOrionImageDialogProps {
  image: OrionVmImage | null
  open: boolean
  onOpenChange: (open: boolean) => void
}

export function DeleteOrionImageDialog({ image, open, onOpenChange }: DeleteOrionImageDialogProps) {
  const { mutate: deleteImage, isPending } = useDeleteOrionImage()

  const label = image?.image_name?.trim() || shortDigest(image?.digest)
  const digestHint = image?.image_name ? ` (${shortDigest(image.digest)})` : ''

  const onDelete = () => {
    if (!image) return

    deleteImage(image.id, {
      onSuccess: () => {
        onOpenChange(false)
      }
    })
  }

  return (
    <Dialog.Root
      open={open}
      onOpenChange={(next) => {
        if (isPending) return
        onOpenChange(next)
      }}
    >
      <Dialog.Header>
        <Dialog.Title>Delete image</Dialog.Title>
        <Dialog.Description>
          Are you sure you want to delete {label}
          {digestHint}? This removes the catalog entry and its objects from storage.
        </Dialog.Description>
      </Dialog.Header>

      <Dialog.Footer>
        <Dialog.TrailingActions>
          <Button variant='flat' disabled={isPending} onClick={() => onOpenChange(false)}>
            Cancel
          </Button>
          <Button autoFocus variant='destructive' disabled={!image} loading={isPending} onClick={onDelete}>
            Delete
          </Button>
        </Dialog.TrailingActions>
      </Dialog.Footer>
    </Dialog.Root>
  )
}
