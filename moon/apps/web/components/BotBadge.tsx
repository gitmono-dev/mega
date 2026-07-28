import { Badge } from '@gitmono/ui/Badge'
import { cn } from '@gitmono/ui/utils'

export function BotBadge({ size = 'sm', className }: { size?: 'xs' | 'sm'; className?: string }) {
  if (size === 'xs') {
    return (
      <Badge tooltip='Bot' className={cn('h-4.5 w-4.5', className)} color='blue'>
        B
      </Badge>
    )
  }

  return (
    <Badge className={cn(className)} color='blue'>
      Bot
    </Badge>
  )
}
