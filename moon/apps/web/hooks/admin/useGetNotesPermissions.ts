import { useMemo } from 'react'
import { useQuery } from '@tanstack/react-query'

import type { Note } from '@gitmono/types'

import { useAdminCheck } from '@/hooks/admin/useAdminCheck'
import { useGetCurrentUser } from '@/hooks/useGetCurrentUser'
import { legacyApiClient } from '@/utils/queryClient'

export interface NotePermission {
  noteId: string
  hasRead: boolean
  hasWrite: boolean
  isAdmin: boolean
}

interface UseGetNotesPermissionsOptions {
  notes: Note[]
  enabled?: boolean
}

export function useGetNotesPermissions({ notes, enabled = true }: UseGetNotesPermissionsOptions) {
  const { data: currentUser } = useGetCurrentUser()
  const { data: adminCheck } = useAdminCheck()
  const isSystemAdmin = adminCheck?.data?.is_admin || false

  const noteIds = useMemo(() => notes.map((note) => note.id), [notes])

  return useQuery({
    queryKey: ['notes-permissions', noteIds, currentUser?.username, isSystemAdmin],
    queryFn: async (): Promise<Record<string, NotePermission>> => {
      // If user is system admin, return all permissions directly
      if (isSystemAdmin) {
        return noteIds.reduce(
          (acc, noteId) => {
            acc[noteId] = {
              noteId,
              hasRead: true,
              hasWrite: true,
              isAdmin: true
            }
            return acc
          },
          {} as Record<string, NotePermission>
        )
      }

      // Batch query permissions for all notes
      const permissionsPromises = notes.map(async (note) => {
        const isAuthor = !!note.viewer_is_author

        try {
          let response

          if (isSystemAdmin) {
            // Admin users: use admin endpoint
            response = await legacyApiClient.v1
              .getApiAdminUsersPermissionsByResourceId()
              .request(currentUser?.username || '', 'note', note.id)
          } else {
            // Regular users: use /me endpoint
            response = await legacyApiClient.v1.getApiPermissionsMeByResourceId().request('note', note.id)
          }

          if (response?.data) {
            return {
              noteId: note.id,
              // Authors always retain access even when group ACL has no grant.
              hasRead: response.data.has_read || isAuthor,
              hasWrite: response.data.has_write || isAuthor,
              isAdmin: response.data.is_admin
            }
          }
        } catch (error) {
          // If query fails, default to no permissions
          // Silently handle the error to avoid console warnings
        }

        return {
          noteId: note.id,
          hasRead: isAuthor,
          hasWrite: isAuthor,
          isAdmin: false
        }
      })

      const permissionsArray = await Promise.all(permissionsPromises)

      // Convert to Record format
      return permissionsArray.reduce(
        (acc, perm) => {
          acc[perm.noteId] = perm
          return acc
        },
        {} as Record<string, NotePermission>
      )
    },
    enabled: enabled && !!currentUser?.username && notes.length > 0,
    staleTime: 1000 * 60 * 5, // 5 minutes cache
    retry: false
  })
}
