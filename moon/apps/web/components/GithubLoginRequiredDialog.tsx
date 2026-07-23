'use client'

import { useEffect, useState } from 'react'

import { Button, Dialog, UIText } from '@gitmono/ui'

import { useGetCurrentUser } from '@/hooks/useGetCurrentUser'
import { useSignoutUser } from '@/hooks/useSignoutUser'

const DISMISS_PREFIX = 'github-login-required-dismissed:'

function needsGithubRelogin(user: {
  github_login?: string | null
  integration?: boolean
  system?: boolean
}): boolean {
  if (user.integration || user.system) return false
  return !user.github_login
}

function dismissKey(userId: string) {
  return `${DISMISS_PREFIX}${userId}`
}

/**
 * When a signed-in user has no `github_login` (older sessions before GitHub
 * identity was linked), show a dismissible dialog explaining that permissions
 * need a GitHub re-login. Closing lets them continue; Sign out takes them to
 * sign-in to complete the link.
 */
export function GithubLoginRequiredDialog() {
  const { data: currentUser } = useGetCurrentUser()
  const signout = useSignoutUser()
  const [open, setOpen] = useState(false)

  useEffect(() => {
    if (!currentUser?.logged_in || !needsGithubRelogin(currentUser)) {
      setOpen(false)
      return
    }

    try {
      if (sessionStorage.getItem(dismissKey(currentUser.id)) === '1') {
        setOpen(false)
        return
      }
    } catch {
      // sessionStorage may be unavailable; still show the dialog
    }

    setOpen(true)
  }, [currentUser])

  if (!currentUser?.logged_in || !needsGithubRelogin(currentUser)) {
    return null
  }

  const handleOpenChange = (next: boolean) => {
    setOpen(next)
    if (!next) {
      try {
        sessionStorage.setItem(dismissKey(currentUser.id), '1')
      } catch {
        // ignore
      }
    }
  }

  return (
    <Dialog.Root open={open} onOpenChange={handleOpenChange} size='base' align='center'>
      <Dialog.Header>
        <Dialog.Title>GitHub login required</Dialog.Title>
        <Dialog.CloseButton />
      </Dialog.Header>

      <Dialog.Content className='gap-3'>
        <UIText element='p' secondary>
          Your account is missing a linked GitHub login. Admin permissions and repository access are keyed by GitHub
          identity (for example <code className='rounded bg-black/5 px-1 dark:bg-white/10'>.mega_cedar.json</code>
          ), so some features may not work until you reconnect.
        </UIText>
        <UIText element='p' secondary>
          Sign out, then sign in again with GitHub to sync your login. You can close this dialog and continue for now if
          you prefer.
        </UIText>
      </Dialog.Content>

      <Dialog.Footer>
        <Dialog.LeadingActions>
          <Button variant='plain' onClick={() => handleOpenChange(false)}>
            Continue anyway
          </Button>
        </Dialog.LeadingActions>
        <Dialog.TrailingActions>
          <Button
            variant='primary'
            disabled={signout.isPending}
            onClick={() => signout.mutate()}
          >
            {signout.isPending ? 'Signing out…' : 'Sign out & re-login'}
          </Button>
        </Dialog.TrailingActions>
      </Dialog.Footer>
    </Dialog.Root>
  )
}
