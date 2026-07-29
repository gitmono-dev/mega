'use client'

import React from 'react'

import { Tooltip, UIText } from '@gitmono/ui'

/** Truncate long cell text; show a tooltip/popup with the full value when clipped (or when popupText is richer). */
export function TruncatedText({
  text,
  popupText,
  className,
  mono = false
}: {
  text: string
  /** Optional longer text for the popup (defaults to `text`). */
  popupText?: string
  className?: string
  mono?: boolean
}) {
  const ref = React.useRef<HTMLSpanElement>(null)
  const [truncated, setTruncated] = React.useState(false)
  const full = popupText && popupText !== text ? popupText : text

  const measure = React.useCallback(() => {
    const el = ref.current

    if (!el) return
    setTruncated(el.scrollWidth > el.clientWidth + 1)
  }, [])

  React.useLayoutEffect(() => {
    measure()
  }, [text, measure])

  React.useEffect(() => {
    const el = ref.current

    if (!el || typeof ResizeObserver === 'undefined') return
    const ro = new ResizeObserver(() => measure())

    ro.observe(el)
    return () => ro.disconnect()
  }, [measure])

  const content = (
    <span
      ref={ref}
      className={['block min-w-0 truncate', mono ? 'font-mono text-xs' : '', className].filter(Boolean).join(' ')}
    >
      {text}
    </span>
  )

  const needsPopup = Boolean(text && text !== '—' && (truncated || (popupText && popupText !== text)))

  if (!needsPopup) {
    return content
  }

  return (
    <Tooltip
      label={
        <UIText size='text-xs' className='max-w-xs break-all whitespace-pre-wrap'>
          {full}
        </UIText>
      }
      delayDuration={200}
      side='top'
      align='start'
    >
      <button type='button' className='block max-w-full min-w-0 cursor-default text-left'>
        {content}
      </button>
    </Tooltip>
  )
}
