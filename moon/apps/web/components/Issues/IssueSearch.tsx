import React from 'react'
import { XIcon } from '@primer/octicons-react'

import { Button, Link, SearchIcon } from '@gitmono/ui'

import { useScope } from '@/contexts/scope'

interface IssueSearchProps {
  filterQuery?: string
  onClearFilters?: () => void
}

export default function IssueSearch({ filterQuery, onClearFilters }: IssueSearchProps) {
  return (
    <>
      <div className='flex min-h-[35px] items-center gap-2 bg-transparent'>
        <div className='border-primary bg-primary group flex min-h-[35px] flex-1 items-center rounded-md border px-3 shadow-xs transition-all focus-within:border-blue-500 focus-within:shadow-md focus-within:ring-2 focus-within:ring-blue-100 hover:border-gray-400 dark:focus-within:ring-blue-900/50 dark:hover:border-gray-600'>
          <div className='text-quaternary flex items-center'>
            <SearchIcon className='w-4' />
          </div>

          <input
            type='text'
            value={filterQuery || ''}
            readOnly
            placeholder='Filter issues by author, labels, or assignee...'
            className='text-quaternary placeholder:text-quaternary w-full flex-1 border-none bg-transparent text-sm ring-0 outline-hidden focus:ring-0 focus:outline-hidden'
          />
          {filterQuery && (
            <button
              onClick={onClearFilters}
              className='text-quaternary hover:bg-secondary hover:text-secondary flex items-center justify-center rounded-md p-1 transition-all'
              title='Clear filters'
            >
              <XIcon className='h-4 w-4' />
            </button>
          )}
        </div>

        <LabelsButton />
        <NewIssueButton />
      </div>
    </>
  )
}

const LabelsButton = () => {
  const { scope } = useScope()

  return (
    <Link href={`/${scope}/labels`}>
      <Button variant='primary' size={'base'}>
        Labels
      </Button>
    </Link>
  )
}

export const NewIssueButton = () => {
  const { scope } = useScope()

  return (
    <Link href={`/${scope}/issue/new`}>
      <Button variant='primary' className='bg-[#1f883d]' size={'base'}>
        New Issue
      </Button>
    </Link>
  )
}
