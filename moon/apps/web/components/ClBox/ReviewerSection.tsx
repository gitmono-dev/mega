import { AlertIcon, CheckCircleIcon } from '@gitmono/ui'

interface ReviewerSectionProps {
  required: number
  actual: number
}

export function ReviewerSection({ required, actual }: ReviewerSectionProps) {
  const isApproved = actual >= required

  if (isApproved) {
    return (
      <div className='flex items-center p-3 text-green-700 dark:text-green-400'>
        <CheckCircleIcon className='mr-3 h-5 w-5' />
        <div>
          <span className='font-semibold'>A required reviewer has approved</span>
        </div>
      </div>
    )
  }

  return (
    <div className='text-primary flex items-center p-3'>
      <AlertIcon className='mr-3 h-5 w-5 text-yellow-600 dark:text-yellow-500' />
      <div>
        <div className='font-semibold'>Review required</div>
        <div className='text-tertiary ml-auto text-sm'>
          {`At least ${required} reviewer${required > 1 ? 's' : ''} required with write access, now has ${actual}`}
        </div>
      </div>
    </div>
  )
}
