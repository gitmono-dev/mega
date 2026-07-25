import React, { useRef, useState } from 'react'

import { SyncOrganizationMember } from '@gitmono/types'
import { Button, CheckIcon, ChevronDownIcon, LazyLoadingSpinner, SearchIcon } from '@gitmono/ui'
import { Calendar } from '@gitmono/ui/Calendar'
import { DropdownMenu } from '@gitmono/ui/DropdownMenu'
import { MenuItem } from '@gitmono/ui/Menu'
import { Popover, PopoverContent, PopoverPortal, PopoverTrigger } from '@gitmono/ui/Popover'

import { MemberAvatar } from '@/components/MemberAvatar'

const mockBranches = ['main', 'develop', 'feature/commits-view', 'bugfix/layout']

const DropdownItemWithAvatar = ({ member, classname }: { member: SyncOrganizationMember; classname?: string }) => {
  return (
    <div className={`p-2] flex items-center gap-2 rounded-md border-l-4 border-transparent ${classname || ''}`}>
      <MemberAvatar size='sm' member={member} />
      <span className='text-sm font-semibold'>{member.user.display_name}</span>
      <span className='ml-1 text-xs text-gray-500'>{member.user.username}</span>
    </div>
  )
}

interface BranchDropdownProps {
  value: string
  onChange: (branch: string) => void
  onClose?: (branch: string) => void
}

export function BranchDropdown({ value, onChange, onClose }: BranchDropdownProps) {
  const [query, setQuery] = useState('')
  const [open, setOpen] = useState(false)
  const ref = useRef<HTMLInputElement>(null)
  const isSearching = query.length > 0

  const handleOpenChange = (isOpen: boolean) => {
    setOpen(isOpen)
    if (!isOpen) {
      setQuery('')
      if (onClose) {
        onClose(value)
      }
    }
  }

  const filteredBranches = mockBranches.filter((branch) => branch.toLowerCase().includes(query.toLowerCase()))

  const branchItems: MenuItem[] = filteredBranches.map((branch) => ({
    type: 'item' as const,
    label: (
      <div className='flex items-center gap-2'>
        <div className='h-4 w-4'>{value === branch && <CheckIcon />}</div>
        <span className='flex-1'>{branch}</span>
      </div>
    ),
    className: 'overflow-hidden',
    onSelect: (e: Event) => {
      e.preventDefault()
      const newValue = branch === value ? '' : branch

      onChange(newValue)
    }
  }))

  const DropdownSearch = () => (
    <div className='flex flex-1 flex-row items-center gap-2'>
      <span className='text-tertiary flex h-5 w-5 items-center justify-center'>
        {isSearching ? <LazyLoadingSpinner fallback={<SearchIcon />} /> : <SearchIcon />}
      </span>
      <input
        ref={ref}
        className='flex-1 border-none bg-transparent p-0 text-sm ring-0 outline-hidden focus:ring-0'
        placeholder='Filter branches'
        role='searchbox'
        autoComplete='off'
        autoCorrect='off'
        spellCheck={false}
        type='text'
        value={query}
        onChange={(e) => setQuery(e.target.value)}
        onKeyDown={(e) => {
          if (e.key === 'Escape') {
            setQuery('')
            ref.current?.blur()
          } else if (e.key === 'Enter') {
            e.preventDefault()
            e.stopPropagation()
          }
        }}
      />
    </div>
  )

  return (
    <DropdownMenu
      open={open}
      onOpenChange={handleOpenChange}
      key='branch'
      align='start'
      desktop={{ width: 'w-72 max-h-[50vh] overflow-auto bg-white' }}
      items={[
        {
          type: 'item',
          disabled: true,
          label: <p>Switch branches</p>
        },
        {
          type: 'item' as const,
          label: <DropdownSearch />,
          onSelect: (e: Event) => e.preventDefault()
        },
        { type: 'separator' as const },
        ...branchItems
      ]}
      trigger={
        <Button size='sm' variant='plain' tooltipShortcut='Branch'>
          <div className='flex items-center justify-center'>
            {value || 'main'} <ChevronDownIcon />
          </div>
        </Button>
      }
    />
  )
}

interface AuthorDropdownProps {
  members: any[]
  value: string
  onChange: (author: string) => void
  onClose?: (author: string) => void
}

export function AuthorDropdown({ members, value, onChange, onClose }: AuthorDropdownProps) {
  const [query, setQuery] = useState('')
  const [open, setOpen] = useState(false)
  const ref = useRef<HTMLInputElement>(null)
  const isSearching = query.length > 0

  const handleOpenChange = (isOpen: boolean) => {
    setOpen(isOpen)
    if (!isOpen) {
      setQuery('')
      if (onClose) {
        onClose(value)
      }
    }
  }

  const filteredMembers = members.filter(
    (member) =>
      member.user.display_name.toLowerCase().includes(query.toLowerCase()) ||
      member.user.username.toLowerCase().includes(query.toLowerCase())
  )

  const authorItems: MenuItem[] = filteredMembers.map((member) => ({
    type: 'item' as const,
    label: (
      <div className='flex items-center gap-1'>
        <div className='h-4 w-4'>{value === member.user.username && <CheckIcon />}</div>
        <DropdownItemWithAvatar member={member} classname='text-sm' />
      </div>
    ),
    className: 'overflow-hidden',
    onSelect: (e: Event) => {
      e.preventDefault()
      const newValue = member.user.username === value ? '' : member.user.username

      onChange(newValue)
    }
  }))

  const DropdownSearch = () => (
    <div className='flex flex-1 flex-row items-center gap-2'>
      <span className='text-tertiary flex h-5 w-5 items-center justify-center'>
        {isSearching ? <LazyLoadingSpinner fallback={<SearchIcon />} /> : <SearchIcon />}
      </span>
      <input
        ref={ref}
        className='flex-1 border-none bg-transparent p-0 text-sm ring-0 outline-hidden focus:ring-0'
        placeholder='Filter by author'
        role='searchbox'
        autoComplete='off'
        autoCorrect='off'
        spellCheck={false}
        type='text'
        value={query}
        onChange={(e) => setQuery(e.target.value)}
        onKeyDown={(e) => {
          if (e.key === 'Escape') {
            setQuery('')
            ref.current?.blur()
          } else if (e.key === 'Enter') {
            e.preventDefault()
            e.stopPropagation()
          }
        }}
      />
    </div>
  )

  return (
    <DropdownMenu
      open={open}
      onOpenChange={handleOpenChange}
      key='author'
      align='end'
      desktop={{ width: 'w-72 max-h-[50vh] overflow-auto bg-white' }}
      items={[
        {
          type: 'item',
          disabled: true,
          label: <p>Filter by author</p>
        },
        {
          type: 'item' as const,
          label: <DropdownSearch />,
          onSelect: (e: Event) => e.preventDefault()
        },
        { type: 'separator' as const },
        ...authorItems
      ]}
      trigger={
        <Button size='sm' variant='plain' tooltipShortcut='Author'>
          <div className='flex items-center justify-center'>
            {value || 'Author'} <ChevronDownIcon />
          </div>
        </Button>
      }
    />
  )
}

export interface DateRangeValue {
  from?: Date
  to?: Date
}

interface TimeDropdownProps {
  members: any[]
  value: DateRangeValue
  onChange: (range: DateRangeValue) => void
  onClose?: (range: DateRangeValue) => void
}

export function TimeDropdown({ value, onChange, onClose }: TimeDropdownProps) {
  const [open, setOpen] = useState(false)

  const handleOpenChange = (isOpen: boolean) => {
    setOpen(isOpen)
    if (!isOpen && onClose) {
      onClose(value)
    }
  }

  const handleSelect = (range: { from?: Date; to?: Date } | undefined) => {
    onChange({
      from: range?.from,
      to: range?.to
    })
  }

  return (
    <Popover open={open} onOpenChange={handleOpenChange}>
      <PopoverTrigger asChild>
        <Button size='sm' variant='plain' tooltipShortcut='Date'>
          <div className='flex items-center justify-center'>
            All time <ChevronDownIcon />
          </div>
        </Button>
      </PopoverTrigger>
      <PopoverPortal>
        <PopoverContent align='end' className='bg-elevated z-50 rounded-lg border p-3 shadow-lg'>
          <div className='mb-2 text-sm font-medium text-gray-600'>Filter by date range</div>
          <Calendar
            mode='range'
            selected={{ from: value.from, to: value.to }}
            onSelect={handleSelect}
            numberOfMonths={1}
            className='rounded-md'
          />
          {(value.from || value.to) && (
            <div className='mt-2 flex justify-end'>
              <Button size='sm' variant='plain' onClick={() => onChange({ from: undefined, to: undefined })}>
                Clear
              </Button>
            </div>
          )}
        </PopoverContent>
      </PopoverPortal>
    </Popover>
  )
}
