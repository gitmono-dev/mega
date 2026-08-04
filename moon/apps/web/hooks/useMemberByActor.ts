import { SyncOrganizationMember } from '@gitmono/types'

import { useMemberMap } from '@/components/Issues/utils/sideEffect'

/**
 * Resolve an org member from mono actor identity (campsite_user_id),
 * Campsite username, or github_login via the synced members map.
 */
export function useMemberByActor(actor?: string | null, enabled = true) {
  const memberMap = useMemberMap()
  const data = enabled && actor ? (memberMap.get(actor) as SyncOrganizationMember | undefined) : undefined

  return { data }
}
