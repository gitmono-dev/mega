import React, { useState } from 'react'

import { Dialog } from '@gitmono/ui'

import { useUpdateClStatus } from '@/components/ClView/hook/useUpdateClStatus'

interface ConvertToDraftDialogProps {
  trigger: React.ReactNode
  link: string
}

export const ConvertToDraftDialog: React.FC<ConvertToDraftDialogProps> = ({ trigger, link }) => {
  const [open, setOpen] = useState(false)
  const { mutate: updateClStatus, isPending } = useUpdateClStatus()

  const handleOpenChange = (nextOpen: boolean) => {
    setOpen(nextOpen)
  }

  const handleConfirm = () => {
    updateClStatus(
      { link, status: 'draft' },
      {
        onSuccess: () => {
          setOpen(false)
        }
      }
    )
  }

  const handleConfirmClick = (event: React.MouseEvent<HTMLButtonElement>) => {
    event.stopPropagation()
    handleConfirm()
  }

  const handleTriggerClick = (event: React.MouseEvent) => {
    event.stopPropagation()
    setOpen(true)
  }

  return (
    <>
      <span onClick={handleTriggerClick}>{trigger}</span>
      <Dialog.Root open={open} onOpenChange={handleOpenChange} size='lg' align='top'>
        <Dialog.Header className='flex items-center rounded-b-none border-b bg-white p-4 text-sm'>
          <Dialog.Title className='text-sm font-semibold text-[#22262b]'>
            Convert this pull request to draft?
          </Dialog.Title>
          <Dialog.CloseButton />
        </Dialog.Header>
        <Dialog.Content className='flex min-h-[70px] justify-center bg-[#fff8c5] px-4 py-6'>
          <div className='flex items-center text-xs text-[#22262b]'>
            People who are already subscribed will not be unsubscribed.
          </div>
        </Dialog.Content>
        <Dialog.Footer
          variant='secondary'
          className='flex justify-center rounded-t-none border-t bg-[#f6f8fa] px-4 pt-3 pb-3'
        >
          <button
            type='button'
            onClick={handleConfirmClick}
            disabled={isPending}
            className='w-full rounded-md border border-[#dce2e7] bg-[#f6f8fa] px-4 py-2 text-center text-sm font-semibold text-[#cf222e] transition-colors hover:bg-[#cf222e] hover:text-[#f6f8fa] disabled:cursor-not-allowed disabled:opacity-60'
          >
            Convert to draft
          </button>
        </Dialog.Footer>
      </Dialog.Root>
    </>
  )
}
