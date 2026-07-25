'use client'

import * as React from 'react'
import { useMemo } from 'react'
import { add, format, sub } from 'date-fns'
import { DayPicker, useDayPicker, type DayButtonProps, type MonthCaptionProps } from 'react-day-picker'

import { Button } from '../Button'
import { ChevronLeftIcon, ChevronRightIcon } from '../Icons'
import { UIText } from '../Text'
import { cn } from '../utils'

export type CalendarProps = React.ComponentProps<typeof DayPicker>

function CalendarDayButton({ day, modifiers, className, ...buttonProps }: DayButtonProps) {
  const buttonVariant = useMemo(() => {
    if (modifiers.selected) return 'important'
    if (modifiers.today) return 'flat'
    if (modifiers.hidden) return 'none'
    return 'plain'
  }, [modifiers])

  return (
    <Button
      {...buttonProps}
      className={cn(
        'm-0 h-8 w-8 p-0',
        modifiers.outside && 'opacity-60',
        modifiers.selected && 'focus:ring-0',
        className
      )}
      variant={buttonVariant}
    >
      {day.date.getDate()}
    </Button>
  )
}

function CalendarMonthCaption({ calendarMonth }: MonthCaptionProps) {
  const { previousMonth, nextMonth, goToMonth } = useDayPicker()
  const displayMonth = calendarMonth.date

  return (
    <div className='relative flex'>
      <div className='flex-none'>
        <Button
          accessibilityLabel='Previous month'
          onClick={() => goToMonth(sub(displayMonth, { months: 1 }))}
          disabled={!previousMonth}
          iconOnly={<ChevronLeftIcon />}
        />
      </div>
      <div className='grow place-self-center text-center'>
        <UIText size='text-sm' weight='font-medium'>
          {format(displayMonth, 'MMMM yyyy')}
        </UIText>
      </div>
      <div className='flex-none'>
        <Button
          accessibilityLabel='Next month'
          onClick={() => goToMonth(add(displayMonth, { months: 1 }))}
          disabled={!nextMonth}
          iconOnly={<ChevronRightIcon />}
        />
      </div>
    </div>
  )
}

// eslint-disable-next-line react/prop-types
export function Calendar({ className, classNames, showOutsideDays = true, ...props }: CalendarProps) {
  return (
    <DayPicker
      showOutsideDays={showOutsideDays}
      className={className}
      classNames={{
        months: 'flex flex-col sm:flex-row space-y-4 sm:space-x-4 sm:space-y-0',
        month: 'space-y-2',
        month_grid: 'w-full border-collapse space-y-1',
        weekdays: 'flex',
        weekday: 'text-muted-foreground rounded-md w-8 m-0.5 font-normal text-[0.8rem]',
        week: 'flex w-full',
        day: cn(
          'focus:ring-0 relative p-0 m-0.5 text-center text-sm focus-within:relative focus-within:z-20 [&:has([aria-selected])]:bg-accent [&:has([aria-selected].outside)]:bg-accent/50 [&:has([aria-selected].range_end)]:rounded-r-md',
          props.mode === 'range'
            ? '[&:has(>.range_end)]:rounded-r-md [&:has(>.range_start)]:rounded-l-md first:[&:has([aria-selected])]:rounded-l-md last:[&:has([aria-selected])]:rounded-r-md'
            : '[&:has([aria-selected])]:rounded-md'
        ),
        ...classNames
      }}
      components={{
        DayButton: CalendarDayButton,
        MonthCaption: CalendarMonthCaption
      }}
      {...props}
    />
  )
}
