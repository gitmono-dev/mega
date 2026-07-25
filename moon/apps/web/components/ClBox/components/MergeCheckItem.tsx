import { AlertIcon, CheckCircleIcon, WarningTriangleIcon } from '@gitmono/ui'

import { TaskData } from '../types/mergeCheck.config'

const statusMap = {
  Pending: { Icon: WarningTriangleIcon, className: 'text-yellow-600 dark:text-yellow-500' },
  Success: { Icon: CheckCircleIcon, className: 'text-green-600 dark:text-green-400' },
  Failure: { Icon: AlertIcon, className: 'text-red-600 dark:text-red-400' },
  Warning: { Icon: WarningTriangleIcon, className: 'text-yellow-600 dark:text-yellow-500' }
}

export function MergeCheckItem({ check }: { check: TaskData }) {
  const { Icon, className } = statusMap[check.status]

  return (
    <div className='hover:bg-tertiary flex items-center rounded-md p-2'>
      <Icon className={`h-5 w-5 shrink-0 ${className}`} />
      <div className='ml-3 grow'>
        <span className='text-primary font-semibold'>{check.repo_name}</span>
        {check.arguments && <span className='text-tertiary ml-2 text-sm'>{check.arguments}</span>}
      </div>
      <button className='text-tertiary hover:text-primary'></button>
    </div>
  )
}
