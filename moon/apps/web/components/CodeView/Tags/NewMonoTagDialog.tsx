import React, { useState } from 'react'

import {
  Button,
  LazyLoadingSpinner,
  RadioGroup,
  RadioGroupItem,
  SelectCommandContainer,
  SelectCommandEmpty,
  SelectCommandGroup,
  SelectCommandInput,
  SelectCommandItem,
  SelectCommandList,
  SelectCommandSeparator,
  UIText
} from '@gitmono/ui'
import { Dialog } from '@gitmono/ui/Dialog'

import { useCreateMonoTag } from '@/hooks/useCreateMonoTag'
import { useGetCurrentUser } from '@/hooks/useGetCurrentUser'
import { useGetLatestCommit } from '@/hooks/useGetLatestCommit'
import { useGetTreeCommitInfo } from '@/hooks/useGetTreeCommitInfo'

interface Props {
  open: boolean
  onOpenChange: (open: boolean) => void
  onCreated?: (tag: any) => void
}

export default function NewMonoTagDialog({ open, onOpenChange, onCreated }: Props) {
  const [name, setName] = useState('')
  const [message, setMessage] = useState('')
  const [target, setTarget] = useState('')
  const [targetMode, setTargetMode] = useState<'branch' | 'commit'>('branch')
  const [pickerQuery, setPickerQuery] = useState('')

  const { mutateAsync, isPending } = useCreateMonoTag()
  const { data: currentUser } = useGetCurrentUser()
  const { data: latestCommit, isLoading: latestLoading } = useGetLatestCommit('/')
  const { data: treeCommitResp, isLoading: listLoading, isFetching: listFetching } = useGetTreeCommitInfo('/')

  const commitOptions = React.useMemo(() => {
    const items = treeCommitResp?.data ?? []
    // Dedupe by commit_id and sort by date desc (string compare fallback)
    const map = new Map<string, { id: string; message: string; date: string }>()

    for (const it of items) {
      const id = it.commit_id

      if (!id) continue
      if (!map.has(id)) {
        map.set(id, { id, message: it.commit_message || '', date: it.date || '' })
      }
    }
    let arr = Array.from(map.values())
    // filter by query
    const q = pickerQuery.trim().toLowerCase()

    if (q) {
      arr = arr.filter((c) => c.id.toLowerCase().includes(q) || c.message.toLowerCase().includes(q))
    }
    arr.sort((a, b) => (a.date < b.date ? 1 : a.date > b.date ? -1 : 0))
    return arr
  }, [treeCommitResp, pickerQuery])

  const canSubmit = name.trim().length > 0 && !isPending

  async function onCreate() {
    const resolvedTarget = targetMode === 'commit' ? target || latestCommit?.oid || undefined : undefined

    try {
      const result = await mutateAsync({
        name: name.trim(),
        message: message || undefined,
        target: resolvedTarget,
        path_context: '/',
        tagger_email: currentUser?.email || undefined,
        tagger_name: currentUser?.display_name || currentUser?.username || undefined
      })

      if (onCreated && result?.data) {
        onCreated(result.data)
      }
      onOpenChange(false)
      setName('')
      setMessage('')
      setTarget('')
      setTargetMode('branch')
    } catch (e: any) {
      // error message
      alert(e?.message || 'Tag creation failed, please try again later')
    }
  }

  return (
    <Dialog.Root open={open} onOpenChange={onOpenChange}>
      <Dialog.Content>
        <Dialog.CloseButton />
        <Dialog.Header>
          <Dialog.Title>Create tag</Dialog.Title>
        </Dialog.Header>
        <div className='flex flex-col gap-3 py-2'>
          <div className='flex flex-col gap-2'>
            <label className='text-quaternary text-sm'>Name</label>
            <input
              className='border-primary bg-primary text-primary focus:border-accent focus:ring-accent/20 rounded-md border px-2 py-1 text-sm outline-hidden transition-colors focus:ring-2'
              value={name}
              onChange={(e: React.ChangeEvent<HTMLInputElement>) => setName(e.target.value)}
              placeholder='v1.0.0'
            />
          </div>
          <div className='flex flex-col gap-2'>
            <label className='text-quaternary text-sm'>Message (optional)</label>
            <textarea
              className='border-primary bg-primary text-primary focus:border-accent focus:ring-accent/20 rounded-md border px-2 py-1 text-sm outline-hidden transition-colors focus:ring-2'
              value={message}
              onChange={(e: React.ChangeEvent<HTMLTextAreaElement>) => setMessage(e.target.value)}
              placeholder='Release notes'
              rows={4}
            />
          </div>
          <div className='flex flex-col gap-2'>
            <label className='text-quaternary text-sm'>Target</label>
            <RadioGroup
              value={targetMode}
              onValueChange={(v) => setTargetMode(v as 'branch' | 'commit')}
              className='flex flex-col gap-2'
            >
              <RadioGroupItem value='branch'>Branch (default HEAD)</RadioGroupItem>
              <RadioGroupItem value='commit'>Recent commit</RadioGroupItem>
            </RadioGroup>

            {targetMode === 'commit' && (
              <div className='border-primary bg-secondary mt-2 flex flex-col gap-2 rounded-md border p-2'>
                <div className='flex items-center gap-2'>
                  <input
                    className='border-primary bg-primary text-primary focus:border-accent focus:ring-accent/20 flex-1 rounded-md border px-2 py-1 text-sm outline-hidden transition-colors focus:ring-2'
                    value={target}
                    onChange={(e: React.ChangeEvent<HTMLInputElement>) => setTarget(e.target.value)}
                    placeholder='Commit SHA'
                  />
                  <Button
                    variant='base'
                    onClick={() => {
                      if (latestCommit?.oid) setTarget(latestCommit.oid)
                    }}
                    disabled={latestLoading || !latestCommit?.oid}
                  >
                    Use latest
                  </Button>
                </div>
                {/* Inline commit history selector */}
                <div className='border-primary bg-primary rounded-md border p-0'>
                  <SelectCommandContainer className='flex max-h-[280px] flex-col'>
                    <div className='flex items-center gap-2 p-2'>
                      <SelectCommandInput value={pickerQuery} onValueChange={(v) => setPickerQuery(v)} />
                    </div>
                    <SelectCommandSeparator alwaysRender />
                    <SelectCommandList>
                      {listLoading || listFetching ? (
                        <div className='flex items-center justify-center py-6'>
                          <LazyLoadingSpinner />
                        </div>
                      ) : (
                        <>
                          <SelectCommandEmpty>No commits</SelectCommandEmpty>
                          <SelectCommandGroup className='py-1'>
                            {commitOptions.map((c) => (
                              <SelectCommandItem
                                key={c.id}
                                value={c.id}
                                title={`${c.id.substring(0, 8)} · ${c.message}`}
                                onSelect={() => setTarget(c.id)}
                              >
                                <div className='flex min-w-0 flex-col'>
                                  <span className='truncate'>
                                    {c.id.substring(0, 12)} · {c.message}
                                  </span>
                                </div>
                              </SelectCommandItem>
                            ))}
                          </SelectCommandGroup>
                        </>
                      )}
                    </SelectCommandList>
                  </SelectCommandContainer>
                </div>
                <div>
                  {latestLoading ? (
                    <UIText quaternary size='text-[12px]'>
                      Loading latest commit…
                    </UIText>
                  ) : latestCommit?.oid ? (
                    <UIText quaternary size='text-[12px]'>
                      {latestCommit.oid.substring(0, 8)} · {latestCommit.short_message}
                    </UIText>
                  ) : (
                    <UIText quaternary size='text-[12px]'>
                      No recent commit info
                    </UIText>
                  )}
                </div>
              </div>
            )}
          </div>
        </div>
        <Dialog.Footer>
          <Dialog.TrailingActions>
            <Button disabled={!canSubmit} onClick={onCreate} loading={isPending}>
              Create tag
            </Button>
          </Dialog.TrailingActions>
        </Dialog.Footer>
      </Dialog.Content>
    </Dialog.Root>
  )
}
