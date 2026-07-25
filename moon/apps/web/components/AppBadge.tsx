import { Badge } from '@gitmono/ui/Badge'
import { cn } from '@gitmono/ui/utils'

export function AppBadge({ size = 'sm', className }: { size?: 'xs' | 'sm'; className?: string }) {
  if (size === 'xs') {
    return (
      <Badge tooltip='App' className={cn('h-4.5 w-4.5', className)} color='blue'>
        A
      </Badge>
    )
  }
  if (size === 'sm') {
    return (
      <Badge className={cn(className)} color='blue'>
        App
      </Badge>
    )
  }
}
