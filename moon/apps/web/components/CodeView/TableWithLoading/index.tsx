import { LoadingSpinner } from '@gitmono/ui'

import CodeTable from '../CodeTable'

const SpinnerTable = ({ isLoading, datasource, content }: any) => {
  return (
    <div className='bg-primary relative h-screen p-3.5'>
      {isLoading ? (
        <div className='align-center absolute top-1/2 left-1/2 container flex -translate-x-1/2 -translate-y-1/2 justify-center'>
          <LoadingSpinner />
        </div>
      ) : (
        <CodeTable directory={datasource} loading={isLoading} readmeContent={content} />
      )}
    </div>
  )
}

export default SpinnerTable
