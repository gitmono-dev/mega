'use client'

import React, { useState } from 'react'
import BuildIcon from '@mui/icons-material/Build'
import { DiscussionClosedIcon, HashIcon, ImageIcon, MentionIcon } from '@primer/octicons-react'
import { isWindows } from 'react-device-detect'

import { ArrowUpIcon, Button, ChevronDownIcon, MicrophoneIcon, SparklesIcon, UIText } from '@gitmono/ui'

export function WorkWithChatDialog() {
  const hintText = `'↑↓' to navigate input history, '${isWindows ? 'Ctrl' : '⌘'}↲' to insert a new line`
  const [message, setMessage] = useState(hintText)
  const [isShowingHint, setIsShowingHint] = useState(true)
  const [autoMode] = useState('Auto')

  const handleFocus = () => {
    if (isShowingHint) {
      setMessage('')
      setIsShowingHint(false)
    }
  }

  const handleBlur = () => {
    if (message.trim() === '') {
      setMessage(hintText)
      setIsShowingHint(true)
    }
  }

  const handleChange = (e: React.ChangeEvent<HTMLTextAreaElement>) => {
    const value = e.target.value

    setMessage(value)
    if (isShowingHint && value !== hintText) {
      setIsShowingHint(false)
    }
  }

  return (
    <div className='bg-secondary w-full rounded-lg'>
      {/* Top Icon and Title */}
      <div className='bg-primary mb-0 flex flex-col items-center rounded-t-lg p-4'>
        <div className='mb-3 flex h-16 w-16 items-center justify-center rounded-lg bg-[#52b5f2]'>
          <DiscussionClosedIcon size={24} className='text-black' />
        </div>
        <UIText size='text-lg' weight='font-medium' className='text-secondary'>
          Work with <span className='text-primary'>Chat</span>
        </UIText>
      </div>

      {/* Input box with border and full width */}
      <div className='border-primary bg-tertiary w-full rounded-lg border'>
        {/* @Chat header section with rounded top corners */}
        <div className='bg-secondary flex items-center gap-2 rounded-t-lg px-3 py-2'>
          <div className='flex h-6 w-6 items-center justify-center rounded' style={{ backgroundColor: '#52b5f2' }}>
            <DiscussionClosedIcon size={14} className='text-black' />
          </div>
          <UIText size='text-sm' weight='font-medium' className='text-secondary'>
            @Chat
          </UIText>
          <Button
            variant='plain'
            iconOnly={<BuildIcon style={{ fontSize: 14 }} />}
            accessibilityLabel='Build settings'
            className='text-tertiary h-5 w-5 p-0'
          />
        </div>

        {/* Input area */}
        <div className='relative min-h-[100px] p-3'>
          {/* Text area - takes most of the space */}
          <div className='mb-10'>
            <textarea
              value={message}
              onChange={handleChange}
              onFocus={handleFocus}
              onBlur={handleBlur}
              placeholder=''
              className={`w-full resize-none border-0 bg-transparent text-sm focus:ring-0 focus:outline-hidden ${
                isShowingHint ? 'text-quaternary' : 'text-primary'
              }`}
              style={{
                minHeight: '60px',
                lineHeight: '1.4'
              }}
              rows={3}
            />
          </div>

          {/* Bottom toolbar */}
          <div className='absolute right-3 bottom-3 left-3 flex items-center justify-between'>
            {/* Left side icons */}
            <div className='flex items-center gap-2'>
              <Button
                variant='plain'
                iconOnly={<MentionIcon size={16} />}
                accessibilityLabel='Mention'
                className='text-tertiary hover:text-primary h-6 w-6 p-0'
              />
              <Button
                variant='plain'
                iconOnly={<HashIcon size={16} />}
                accessibilityLabel='Add hashtag'
                className='text-tertiary hover:text-primary h-6 w-6 p-0'
              />
              <Button
                variant='plain'
                iconOnly={<ImageIcon size={16} />}
                accessibilityLabel='Add image'
                className='text-tertiary hover:text-primary h-6 w-6 p-0'
              />
            </div>

            {/* Right side buttons */}
            <div className='flex items-center gap-2'>
              {/* Auto dropdown with green dot */}
              <div className='relative flex items-center'>
                <Button
                  variant='plain'
                  size='sm'
                  className='text-tertiary h-6 px-2 text-xs'
                  rightSlot={<ChevronDownIcon size={12} />}
                >
                  {autoMode}
                </Button>
                <div className='absolute -top-1 -right-1 h-2 w-2 rounded-full bg-green-500' />
              </div>

              {/* Magic wand icon */}
              <Button
                variant='plain'
                iconOnly={<SparklesIcon size={16} />}
                accessibilityLabel='AI assistant'
                className='text-tertiary hover:text-primary h-6 w-6 p-0'
              />

              {/* Microphone icon */}
              <div className='bg-quaternary hover:bg-tertiary flex h-6 w-6 cursor-pointer items-center justify-center rounded-md'>
                <MicrophoneIcon size={16} className='text-secondary' />
              </div>

              {/* Send button */}
              <div
                className='flex h-6 w-6 cursor-pointer items-center justify-center rounded-md hover:opacity-80'
                style={{ backgroundColor: '#d2e6d8' }}
              >
                <ArrowUpIcon size={16} strokeWidth='2.5' style={{ color: '#101011' }} />
              </div>
            </div>
          </div>
        </div>
      </div>
    </div>
  )
}
