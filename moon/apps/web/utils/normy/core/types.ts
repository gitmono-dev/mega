export type DataPrimitive = string | number | boolean | null | undefined | Date

export type DataPrimitiveArray = string[] | number[] | boolean[] | null[] | undefined[] | Date[]

export interface DataObject {
  [index: string]: Data
}

export type Data = DataPrimitive | DataObject | DataPrimitiveArray | DataObject[]

export interface NormalizerConfig {
  getNormalizationObjectKey?: (obj: DataObject) => string | undefined
  devLogging?: boolean
  structuralSharing?: boolean
}

export interface UsedKeys {
  [path: string]: ReadonlyArray<string>
}

export interface NormalizedData {
  queries: {
    [queryKey: string]: {
      data: Data
      dependencies: ReadonlyArray<string>
      usedKeys: UsedKeys
    }
  }
  objects: { [objectId: string]: DataObject }
  dependentQueries: { [objectId: string]: ReadonlyArray<string> }
}
