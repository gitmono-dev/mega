import { CheckIcon, XIcon } from '@primer/octicons-react'

import { LoadingSpinner } from '@gitmono/ui/Spinner'

import { Status } from './store'

export const identifyStatus = (status: Status[keyof Status]) => {
  switch (status) {
    case Status.Completed:
      return <CheckIcon size={14} className='text-green-700 dark:text-green-400' />
    case Status.Failed:
      return <XIcon size={14} className='text-red-600 dark:text-red-400' />
    case Status.Interrupted:
      return <XIcon size={14} className='text-orange-600 dark:text-orange-400' />
    case Status.Building:
      return <LoadingSpinner />
    case Status.Pending:
      return <LoadingSpinner />
    case Status.Uninitialized:
      return <LoadingSpinner />
    case Status.NotFound:
      return <LoadingSpinner />

    default:
      return <XIcon size={14} className='text-red-600 dark:text-red-400' />
  }
}
