import { DependencyList } from 'react'
// eslint-disable-next-line no-restricted-imports
import { useHotkeys, type HotkeyCallback, type Keys, type Options } from 'react-hotkeys-hook'

import { useIsTopLayer } from '.'

export interface LayeredHotkeysProps {
  keys: Keys
  callback: HotkeyCallback
  options?: Options & { repeat?: boolean; skipEscapeWhenDisabled?: boolean }
  dependencies?: DependencyList
}

/**
 * Wraps useHotkeys and automatically disables the hotkey if the layer is not the top layer.
 * Use this hook for hotkeys that should only work when the view layer is open, e.g. list navigation.
 * Do not use it for global hotkeys that should work regardless of the layer.
 */
export function useLayeredHotkeys({
  keys,
  callback,
  options: { repeat, skipEscapeWhenDisabled, ...options } = {},
  dependencies
}: LayeredHotkeysProps) {
  const isTopLayer = useIsTopLayer()

  useHotkeys(
    keys,
    (keyboardEvent, hotkeysEvent) => {
      /**
       * Ignore repeated keydown events by default. This helps prevent re-submitting forms
       * and aggresively re-running callbacks for users with short key repeat delay settings.
       *
       * @see https://github.com/JohannesKlauss/react-hotkeys-hook/issues/327
       */
      if (!repeat && keyboardEvent.repeat) return

      // some components like Radix popovers and dialogs have custom escape key handling
      // add a custom attribute to prevent global hotkeys from firing alongside
      // https://github.com/radix-ui/primitives/issues/1299
      if (
        skipEscapeWhenDisabled &&
        keyboardEvent.key === 'Escape' &&
        keyboardEvent.target &&
        keyboardEvent.target instanceof HTMLElement &&
        keyboardEvent.target.closest('[disable-escape-layered-hotkeys]')
      ) {
        return
      }

      callback(keyboardEvent, hotkeysEvent)
    },
    {
      ...options,
      // shortcut will always be disabled if the layer is not top layer,
      // regardless of the enabled option passed into this hook
      enabled: isTopLayer ? options.enabled : false
    },
    dependencies
  )
}

/**
 * Sequential (ordered) hotkeys, e.g. press `g` then `i`.
 * Built on react-hotkeys-hook sequences (`g>i`) instead of @shopify/react-shortcuts.
 */
export function useOrderedLayeredHotkeys({
  keys,
  callback,
  options,
  dependencies
}: {
  keys: string[]
  callback: HotkeyCallback
  options?: Options & { repeat?: boolean; skipEscapeWhenDisabled?: boolean }
  dependencies?: DependencyList
}) {
  useLayeredHotkeys({
    keys: keys.join('>'),
    callback,
    options,
    dependencies
  })
}
