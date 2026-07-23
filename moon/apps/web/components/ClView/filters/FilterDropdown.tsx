import React, { useRef, useState } from 'react'

import { Button, ChevronDownIcon, LazyLoadingSpinner, SearchIcon } from '@gitmono/ui'
import { DropdownMenu } from '@gitmono/ui/DropdownMenu'
import { MenuItem } from '@gitmono/ui/Menu'

interface FilterDropdownProps {
  name: string
  items: MenuItem[]
  selectedItems?: MenuItem[]
  hasSearch?: boolean
  isChosen?: boolean
  onOpenChange?: (open: boolean) => void
  trigger?: React.ReactNode
}

export function FilterDropdown({
  name,
  items,
  selectedItems = [],
  hasSearch = true,
  isChosen = true,
  onOpenChange,
  trigger
}: FilterDropdownProps) {
  const [query, setQuery] = useState('')
  const [open, setOpen] = useState<boolean>(false)
  const ref = useRef<HTMLInputElement>(null)
  const isSearching = query.length > 0

  const handleOpenChange = (isOpen: boolean) => {
    setOpen(isOpen)
    if (onOpenChange) {
      onOpenChange(isOpen)
    }
    if (!isOpen) {
      setQuery('')
    }
  }

  const DropdownSearch = () => (
    <div className='flex flex-1 flex-row items-center gap-2'>
      <span className='text-tertiary flex h-5 w-5 items-center justify-center'>
        {isSearching ? <LazyLoadingSpinner fallback={<SearchIcon />} /> : <SearchIcon />}
      </span>
      <input
        ref={ref}
        className='flex-1 border-none bg-transparent p-0 text-sm outline-none ring-0 focus:ring-0'
        placeholder={`filter by ${name}`}
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

  // Prevent Radix from closing the menu when focusing/clicking the search input
  const searchItem: MenuItem | null = hasSearch
    ? {
        type: 'item' as const,
        label: <DropdownSearch />,
        onSelect: (e: Event) => e.preventDefault()
      }
    : null

  const defaultTrigger = (
    <Button size='sm' variant={'plain'} tooltipShortcut={name}>
      <div className='flex items-center justify-center'>
        {name} <ChevronDownIcon />
      </div>
    </Button>
  )

  if (isChosen) {
    return (
      <DropdownMenu
        open={open}
        onOpenChange={handleOpenChange}
        key={name}
        align='end'
        desktop={{ width: 'w-72 max-h-[50vh] overflow-auto bg-primary' }}
        items={[
          {
            type: 'item',
            disabled: true,
            label: <p>Filter by {name}</p>
          },
          ...(searchItem ? [searchItem] : []),
          { type: 'separator' as const },
          ...items
        ]}
        trigger={trigger || defaultTrigger}
      />
    )
  } else {
    const unselectedItems = items.filter(
      (item) =>
        !selectedItems.find((selected) => 'label' in item && 'label' in selected && selected.label === item.label)
    )

    return (
      <DropdownMenu
        key={name}
        align='end'
        open={open}
        onOpenChange={handleOpenChange}
        desktop={{ width: 'w-72 max-h-[50vh] overflow-auto bg-primary' }}
        items={[
          {
            type: 'item',
            label: <p>Filter by {name}</p>,
            disabled: true
          },
          ...(searchItem ? [searchItem] : []),
          { type: 'separator' as const },
          { type: 'heading' as const, label: 'Group assignees' },
          ...selectedItems,
          { type: 'separator' as const },
          { type: 'heading' as const, label: 'Suggestions' },
          ...unselectedItems
        ]}
        trigger={trigger || defaultTrigger}
      />
    )
  }
}
