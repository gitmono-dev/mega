'use client'

import React, { KeyboardEvent, useEffect, useRef, useState } from 'react'
import { useParams } from 'next/navigation'

interface PathInputProps {
  pathState: [string, React.Dispatch<React.SetStateAction<string>>]
  nameState: [string, React.Dispatch<React.SetStateAction<string>>]
}

export default function PathInput({ pathState, nameState }: PathInputProps) {
  const [, setPath] = pathState
  const [, setName] = nameState
  const params = useParams()

  // Store fixed base path in ref (no re-renders needed)
  const fixedBasePathRef = useRef<string[]>([])

  const [userSegments, setUserSegments] = useState<string[]>([])
  const [current, setCurrent] = useState<string>('')

  // update pathState
  const updatePath = (b: string[], u: string[], c: string) => {
    const full = [...b, ...u, c].filter(Boolean).join('/')

    setPath(full)
    setName(c) // Update name when path changes
  }

  useEffect(() => {
    const raw = params?.path
    let newBase: string[] = []

    if (Array.isArray(raw)) {
      newBase = raw.filter(Boolean)
    } else if (typeof raw === 'string') {
      newBase = raw.split('/').filter(Boolean)
    }
    newBase.unshift('')

    fixedBasePathRef.current = newBase
    updatePath(newBase, userSegments, current)
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [params?.path])

  const handleChange = (e: React.ChangeEvent<HTMLInputElement>) => {
    const value = e.target.value

    if (value.includes('/')) {
      const rawParts = value.split('/')
      const parts = rawParts.filter(Boolean)
      const last = rawParts[rawParts.length - 1] ?? ''

      if (parts.length > 0) {
        const nextUser = [...userSegments, ...parts]

        setUserSegments(nextUser)
        setCurrent(last)
        updatePath(fixedBasePathRef.current, nextUser, last)
      } else {
        setCurrent('')
        updatePath(fixedBasePathRef.current, userSegments, '')
      }
    } else {
      setCurrent(value)
      updatePath(fixedBasePathRef.current, userSegments, value)
    }
  }

  const handleKeyDown = (e: KeyboardEvent<HTMLInputElement>) => {
    if (e.key === 'Backspace' && current === '') {
      e.preventDefault()
      if (userSegments.length > 0) {
        const nextUser = userSegments.slice(0, -1)

        setUserSegments(nextUser)
        updatePath(fixedBasePathRef.current, nextUser, '')
      }
      // basePath is now protected - cannot be deleted
    }
  }

  return (
    <div className='text-primary flex max-w-[900px] flex-wrap items-center gap-x-1 gap-y-2'>
      {[...fixedBasePathRef.current, ...userSegments].map((seg, i, arr) => (
        // eslint-disable-next-line react/no-array-index-key
        <React.Fragment key={i}>
          <span className='text-accent font-medium'>{seg}</span>
          {i < arr.length - 1 && <span className='text-secondary'>/</span>}
        </React.Fragment>
      ))}
      {[...fixedBasePathRef.current, ...userSegments].length > 0 && <span className='text-secondary'>/</span>}

      <input
        type='text'
        value={current}
        placeholder='Name your file...'
        onChange={handleChange}
        onKeyDown={handleKeyDown}
        className='border-primary bg-primary text-primary placeholder:text-tertiary focus:border-accent focus:ring-accent/20 min-w-[140px] rounded border px-2 py-1 outline-hidden transition-colors focus:ring-2'
        aria-label='file-path-input'
      />
    </div>
  )
}
