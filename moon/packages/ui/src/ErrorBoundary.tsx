// eslint-disable-next-line no-restricted-imports
import { ErrorBoundaryProps, ErrorBoundary as ReactErrorBoundary } from 'react-error-boundary'

const logError = (error: unknown) => {
  console.error(error)
}

export const ErrorBoundary = (props: ErrorBoundaryProps) => <ReactErrorBoundary onError={logError} {...props} />
