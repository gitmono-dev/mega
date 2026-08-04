import { useMemberByActor } from '@/hooks/useMemberByActor'
import { megaUserHandle } from '@/utils/megaUser'

/** Resolves and displays Mega user handle (github_login preferred). */
export function MegaUserLabel({ username, className }: { username?: string | null; className?: string }) {
  const { data: member } = useMemberByActor(username)
  const label = megaUserHandle(member?.user, username || '') || 'username not found'

  return <span className={className}>{label}</span>
}
