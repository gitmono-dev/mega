import * as RadixRadioGroup from '@radix-ui/react-radio-group'

export type RadioGroupProps = RadixRadioGroup.RadioGroupProps & {
  label?: string
}

export function RadioGroup({ label, children, ...props }: React.PropsWithChildren<RadioGroupProps>) {
  return (
    <RadixRadioGroup.Root {...props} className={props.className}>
      {label && (
        <span className='block cursor-default text-sm leading-none font-medium tracking-tight select-none'>
          {label}
        </span>
      )}

      {children}
    </RadixRadioGroup.Root>
  )
}
