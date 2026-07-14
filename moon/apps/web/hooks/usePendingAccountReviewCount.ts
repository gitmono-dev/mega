import { useAdminUserApprovals } from '@/hooks/AccountReview/useAdminUserApprovals'
import { useAdminCheck } from '@/hooks/admin/useAdminCheck'

/** Pending account-review count for system admins; 0 for everyone else. */
export function usePendingAccountReviewCount() {
  const { data: adminCheck, isPending: adminPending, isError: adminError } = useAdminCheck()
  const isAdmin = adminCheck?.data?.is_admin === true
  const { data, isPending: listPending } = useAdminUserApprovals('pending', isAdmin)

  const pendingCount = isAdmin ? (data?.data?.items?.length ?? 0) : 0

  return {
    pendingCount,
    hasPending: pendingCount > 0,
    isAdmin,
    isLoading: (adminPending && !adminError) || (isAdmin && listPending)
  }
}
