import React, { useEffect, useState } from 'react'
import { Box, Flex, Tabs } from '@radix-ui/themes'
import copy from 'copy-to-clipboard'
import { usePathname } from 'next/navigation'

import { MONO_API_URL } from '@gitmono/config'
import { Button, cn, Popover, PopoverContent, PopoverPortal, PopoverTrigger } from '@gitmono/ui'
import { CheckIcon, CopyIcon, DownloadIcon } from '@gitmono/ui/Icons'

const CloneTabs = () => {
  const pathname = usePathname()
  const [copied, setCopied] = useState<boolean>(false)
  const [active_tab, setActiveTab] = useState<string>('HTTP')
  const [open, setOpen] = useState(false)
  const [repo_name, setRepo_name] = useState<string>('')

  useEffect(() => {
    if (pathname) {
      const pathParts = pathname?.split('/code/tree/')[1]?.split('/') ?? []
      const filteredParts = pathParts[0] === 'main' ? pathParts.slice(1) : pathParts
      const repoName = filteredParts.join('/')

      setRepo_name(repoName)
    }
  }, [pathname])

  const url = new URL(MONO_API_URL)

  const handleCopy = () => {
    const text = active_tab === 'HTTP' ? `${url.href}${repo_name}.git` : `ssh://git@${url.host}/${repo_name}.git`

    void copy(text).then((ok) => {
      if (!ok) return
      setCopied(true)
      setTimeout(() => setCopied(false), 2000) // Reset after 2 seconds
    })
  }
  const tabContent = [
    {
      value: 'HTTP',
      inputValue: `${url.href}${repo_name}.git`,
      info: 'Clone using the web URL.'
    },
    {
      value: 'SSH',
      inputValue: `ssh://git@${url.host}/${repo_name}.git`,
      info: 'Use a password-protected SSH key.'
    }
  ]

  return (
    <>
      <Popover open={open} onOpenChange={setOpen}>
        <PopoverTrigger>
          <Button variant='base'>
            <Flex gap='3'>
              <DownloadIcon />
              Code
            </Flex>
          </Button>
        </PopoverTrigger>
        <PopoverPortal>
          {open && (
            <PopoverContent
              className={cn(
                'animate-scale-fade shadow-popover dark:border-primary-opaque bg-primary relative flex h-[130px] w-[400px] flex-1 origin-[--radix-hover-card-content-transform-origin] flex-col overflow-hidden rounded-lg border border-transparent dark:shadow-[0px_2px_16px_rgba(0,0,0,1)]'
              )}
              sideOffset={4}
              align='start'
              onOpenAutoFocus={(e) => e.preventDefault()}
              asChild
              addDismissibleLayer
            >
              <Tabs.Root defaultValue={active_tab} onValueChange={setActiveTab}>
                <Tabs.List size='1' className='p-2'>
                  <Tabs.Trigger value='HTTP'>
                    <Button variant={active_tab === 'HTTP' ? 'flat' : 'plain'}>HTTP</Button>
                  </Tabs.Trigger>
                  <Tabs.Trigger value='SSH' style={{ marginLeft: '10px' }}>
                    <Button variant={active_tab === 'SSH' ? 'flat' : 'plain'}>SSH</Button>
                  </Tabs.Trigger>
                </Tabs.List>
                <Box pt='3'>
                  {tabContent?.map((_item) => {
                    return (
                      <Tabs.Content value={_item.value} key={_item.value}>
                        <Flex align='center'>
                          <input
                            value={_item.inputValue}
                            className='bg-quaternary border-primary m-2 w-[350px] border-r border-b p-1'
                            style={{ borderRadius: '5px' }}
                          />
                          <Button onClick={handleCopy} size='sm' variant='text' className='text-secondary'>
                            {copied ? <CheckIcon /> : <CopyIcon />}
                          </Button>
                        </Flex>
                        <div className='text-tertiary ml-2'>{_item.info}</div>
                      </Tabs.Content>
                    )
                  })}
                </Box>
              </Tabs.Root>
            </PopoverContent>
          )}
        </PopoverPortal>
      </Popover>
    </>
  )
}

export default CloneTabs
