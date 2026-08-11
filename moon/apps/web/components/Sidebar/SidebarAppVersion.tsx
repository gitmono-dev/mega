import { Tooltip } from '@gitmono/ui'

function formatVersionLabel(version: string, buildTime: string | undefined): string {
  const short = version.slice(0, 7)

  if (!buildTime) return short

  const date = buildTime.slice(0, 10)

  return date ? `${short} · ${date}` : short
}

function formatVersionTooltip(version: string, buildTime: string | undefined): string {
  if (!buildTime) return version
  return `${version} · ${buildTime}`
}

export function SidebarAppVersion() {
  const version = process.env.NEXT_PUBLIC_APP_VERSION?.trim() || 'dev'
  const buildTime = process.env.NEXT_PUBLIC_APP_BUILD_TIME?.trim() || undefined
  const label = formatVersionLabel(version, buildTime)
  const tooltip = formatVersionTooltip(version, buildTime)

  return (
    <Tooltip label={tooltip} side='top' align='end'>
      <span className='text-tertiary hover:text-secondary px-1 font-mono text-[11px] tabular-nums select-none'>
        {label}
      </span>
    </Tooltip>
  )
}
