import { Badge } from '@gitmono/ui/Badge'
import { cn } from '@gitmono/ui/utils'

export function GuestBadge({ size = 'sm', className }: { size?: 'xs' | 'sm'; className?: string }) {
  if (size === 'xs') {
    return (
      <Badge tooltip='Guest' className={cn('h-4.5 w-4.5', className)} color='amber'>
        G
      </Badge>
    )
  }
  if (size === 'sm') {
    return (
      <Badge className={cn(className)} color='amber'>
        Guest
      </Badge>
    )
  }
}
