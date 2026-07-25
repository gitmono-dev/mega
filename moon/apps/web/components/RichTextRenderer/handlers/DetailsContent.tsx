import { NodeHandler } from '.'

export const DetailsContent: NodeHandler = ({ children }) => {
  return <div className='mt-1 ml-1'>{children}</div>
}
