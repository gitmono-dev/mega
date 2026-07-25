import { useAtomValue } from 'jotai'

import { layersAtom } from '.'

/**
 * Drop this in the app to show the current DismissibleLayer stack in the UI.
 */
export function DismissibleLayerDevtools() {
  const layers = useAtomValue(layersAtom)

  return (
    <div className='bg-secondary fixed right-4 bottom-4 z-[9999] p-3 font-mono shadow-xl'>
      <ul>
        {Array.from(layers.values()).map((layer) => (
          <li key={layer}>{layer}</li>
        ))}
      </ul>
    </div>
  )
}
