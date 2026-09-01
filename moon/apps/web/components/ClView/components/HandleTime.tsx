import { format, formatDistance, fromUnixTime } from 'date-fns'

import { Tooltip } from '@gitmono/ui'

interface HandleTimeProps {
  created_at: number
}

const HandleTime = ({ created_at }: HandleTimeProps) => {
  const time = formatDistance(fromUnixTime(created_at), new Date(), { addSuffix: true })
  const formatTimestamp = (timestamp: number) => {
    const date = timestamp > 1e12 ? fromUnixTime(Math.floor(timestamp / 1000)) : fromUnixTime(timestamp)

    return format(date, 'yyyy-MM-dd HH:mm:ss')
  }

  return (
    <Tooltip label={formatTimestamp(created_at)}>
      <span className='underline'>{time}</span>
    </Tooltip>
  )
}

export default HandleTime
