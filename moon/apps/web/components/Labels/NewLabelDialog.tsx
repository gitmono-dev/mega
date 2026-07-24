// components/Labels/NewLabelDialog.tsx
import React, { useState } from 'react'
import { colord, random } from 'colord'

import { Button, Dialog, RefreshIcon, TextField } from '@gitmono/ui'

import { getFontColor } from '@/utils/getFontColor'

const PRESET_COLORS = [
  '#b60205',
  '#d93f0b',
  '#fbca04',
  '#0e8a16',
  '#006b75',
  '#1d76db',
  '#0052cc',
  '#5319e7',
  '#e99695',
  '#f9d0c4',
  '#fef2c0',
  '#c2e0c6',
  '#bfdadc',
  '#c5def5',
  '#bfd4f2',
  '#d4c5f9'
]

interface NewLabelDialogProps {
  isOpen: boolean
  onClose: () => void
  onCreateLabel: (name: string, description: string, color: string) => void
}

export const NewLabelDialog: React.FC<NewLabelDialogProps> = ({ isOpen, onClose, onCreateLabel }) => {
  const [color, setColor] = useState(random().toHex())
  const [name, setName] = useState('')
  const [description, setDescription] = useState('')

  const fontColor = getFontColor(color)
  const normalizedColor = colord(color).isValid() ? colord(color).toHex() : '#000000'

  const generateRandomColor = () => {
    setColor(random().toHex())
  }

  const handleColorInputChange = (value: string) => {
    setColor(value.startsWith('#') ? value : `#${value}`)
  }

  const handleCreateLabel = () => {
    if (name.trim()) {
      onCreateLabel(name, description, normalizedColor)
      setName('')
      setDescription('')
      generateRandomColor()
      onClose()
    }
  }

  return (
    <Dialog.Root open={isOpen} onOpenChange={onClose}>
      <Dialog.Title className='w-full p-4'>New Label</Dialog.Title>
      <Dialog.Content>
        <div className='w-full max-w-md p-4'>
          {/* label preview */}
          <div className='mb-4 flex items-center justify-center'>
            <div
              style={{
                backgroundColor: normalizedColor,
                color: fontColor.toHex(),
                borderRadius: '16px',
                padding: '2px 8px',
                fontSize: '12px',
                fontWeight: '600',
                display: 'inline-block',
                textAlign: 'center'
              }}
            >
              {name || 'label preview'}
            </div>
          </div>

          <div className='mb-4'>
            <TextField label='Name' value={name} onChange={(e) => setName(e)} placeholder='Label name' />
          </div>

          <div className='mb-4'>
            <TextField
              label='Description'
              value={description}
              onChange={(e) => setDescription(e)}
              placeholder='Optionally add a description.'
            />
          </div>

          <div className='mb-6'>
            <label className='mb-1 block text-sm font-medium text-gray-700'>Color</label>
            <div className='mb-3 flex items-center gap-2'>
              <button
                type='button'
                onClick={generateRandomColor}
                title='Randomize color'
                className='flex h-8 w-8 flex-shrink-0 items-center justify-center rounded-md border border-black/10'
                style={{ backgroundColor: normalizedColor, color: fontColor.toHex() }}
              >
                <RefreshIcon className='h-4 w-4' />
              </button>
              <div className='flex flex-grow items-center gap-2 rounded-md border px-2 py-1'>
                <input
                  type='color'
                  value={normalizedColor}
                  onChange={(e) => setColor(e.target.value)}
                  className='h-6 w-6 cursor-pointer border-none bg-transparent p-0'
                  title='Pick a color'
                />
                <input
                  className='flex-grow border-none bg-transparent p-0 text-sm outline-none ring-0 focus:ring-0'
                  value={color}
                  onChange={(e) => handleColorInputChange(e.target.value)}
                  placeholder='#ffffff'
                  spellCheck={false}
                />
              </div>
            </div>

            <div className='grid grid-cols-8 gap-2'>
              {PRESET_COLORS.map((preset) => {
                const selected = normalizedColor.toLowerCase() === preset.toLowerCase()

                return (
                  <button
                    key={preset}
                    type='button'
                    title={preset}
                    onClick={() => setColor(preset)}
                    className={`h-6 w-6 rounded-full border transition-transform hover:scale-110 ${
                      selected ? 'ring-2 ring-blue-500 ring-offset-1' : 'border-black/10'
                    }`}
                    style={{ backgroundColor: preset }}
                  />
                )
              })}
            </div>
          </div>

          <div className='flex justify-end gap-2'>
            <Button onClick={onClose}>Cancel</Button>
            <Button variant='primary' className='bg-[#1f883d]' onClick={handleCreateLabel} disabled={!name.trim()}>
              Create label
            </Button>
          </div>
        </div>
      </Dialog.Content>
    </Dialog.Root>
  )
}
