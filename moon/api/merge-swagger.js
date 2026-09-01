#!/usr/bin/env node
const fs = require('fs')
const path = require('path')

const fileBase = path.join(process.cwd(), 'api/gen')
const outputFile = path.join(fileBase, 'merged_swagger.json')

// 要排除的文件
const excludeFiles = new Set(['merged_swagger.json', 'openapi_schema.json'])

// 找到所有 JSON 文件，排除指定的文件
const files = fs
  .readdirSync(fileBase)
  .filter((f) => f.endsWith('.json') && !excludeFiles.has(f))
  .map((f) => path.join(fileBase, f))

if (files.length === 0) {
  console.error('没有找到 JSON 文件可供合并')
  process.exit(1)
}

// 深度合并函数
function deepMerge(target, source) {
  for (const key of Object.keys(source)) {
    if (
      typeof target[key] === 'object' &&
      target[key] !== null &&
      !Array.isArray(target[key]) &&
      typeof source[key] === 'object' &&
      source[key] !== null &&
      !Array.isArray(source[key])
    ) {
      deepMerge(target[key], source[key])
    } else {
      target[key] = source[key]
    }
  }
  return target
}

// 依次读取并 merge
const merged = files
  .map((file) => JSON.parse(fs.readFileSync(file, 'utf-8')))
  .reduce(
    (acc, swagger) => {
      acc.info = acc.info && Object.keys(acc.info).length > 0 ? acc.info : swagger.info || {}
      acc.paths = { ...acc.paths, ...(swagger.paths || {}) }
      acc.components = deepMerge(acc.components, swagger.components || {})
      return acc
    },
    { openapi: '3.0.0', info: {}, paths: {}, components: {} }
  )

/**
 * Mega snowflake ids exceed JS Number.MAX_SAFE_INTEGER. Response schemas already
 * use string (serde + utoipa value_type), but path params often emit int64 or a
 * `$ref: SnowflakeId` (sometimes without the schema component). Rewrite those
 * path params to string so swagger-typescript-api generates `groupId: string`.
 */
const SNOWFLAKE_PATH_PARAM_NAMES = new Set([
  'group_id',
  'thread_id',
  'comment_id',
  'key_id',
  'bot_id',
  'installation_id',
  'token_id',
  'cl_id',
  'item_id'
])

function isMegaSnowflakeIdPath(apiPath, paramName) {
  if (SNOWFLAKE_PATH_PARAM_NAMES.has(paramName)) return true
  if (paramName !== 'id') return false
  return (
    apiPath.includes('/label/') ||
    apiPath.includes('/bots/') ||
    apiPath.includes('/webhooks/') ||
    apiPath.includes('/triggers/') ||
    apiPath.includes('/ssh/') ||
    apiPath.includes('/token/') ||
    apiPath.includes('/user/token') ||
    apiPath.includes('/user/ssh')
  )
}

function isSnowflakeIdRef(schema) {
  return typeof schema?.$ref === 'string' && schema.$ref.includes('SnowflakeId')
}

/**
 * Path params may be int64 (axum_extras) or `$ref: SnowflakeId` (utoipa Path type).
 * The SnowflakeId component is often missing from the emitted OpenAPI, so rewrite
 * both forms to inline `string` for STA.
 */
function rewriteSnowflakePathParams(doc) {
  let rewritten = 0
  for (const [apiPath, pathItem] of Object.entries(doc.paths || {})) {
    if (!pathItem || typeof pathItem !== 'object') continue
    for (const op of Object.values(pathItem)) {
      if (!op || typeof op !== 'object' || !Array.isArray(op.parameters)) continue
      for (const param of op.parameters) {
        if (param.in !== 'path') continue
        const schema = param.schema
        if (!schema) continue
        if (schema.type === 'string') continue

        const refSnowflake = isSnowflakeIdRef(schema)
        if (!refSnowflake && !isMegaSnowflakeIdPath(apiPath, param.name)) continue

        if (refSnowflake || schema.type === 'integer' || schema.format === 'int64' || schema.format === 'int32') {
          param.schema = {
            type: 'string',
            description: schema.description || param.description
          }
          rewritten += 1
        }
      }
    }
  }

  // Keep a string SnowflakeId component if anything still $refs it.
  if (!doc.components) doc.components = {}
  if (!doc.components.schemas) doc.components.schemas = {}
  const existing = doc.components.schemas.SnowflakeId
  if (!existing || existing.type !== 'string') {
    doc.components.schemas.SnowflakeId = {
      type: 'string',
      description: 'Snowflake id; JSON string so JS keeps full precision.'
    }
  }

  return rewritten
}

/** Also rewrite request/response body properties that are snowflake ids (e.g. Orion cl_id). */
function rewriteInt64PropToString(propSchema) {
  if (!propSchema || propSchema.type === 'string') return false
  if (propSchema.type === 'integer' || propSchema.format === 'int64') {
    const description = propSchema.description
    Object.keys(propSchema).forEach((k) => delete propSchema[k])
    propSchema.type = 'string'
    if (description) propSchema.description = description
    return true
  }
  return false
}

function rewriteSnowflakeSchemaProps(doc) {
  let rewritten = 0
  const schemas = doc.components?.schemas || {}
  for (const schema of Object.values(schemas)) {
    const props = schema?.properties
    if (!props) continue
    for (const [name, prop] of Object.entries(props)) {
      if (
        name === 'cl_id' ||
        name === 'group_id' ||
        name === 'item_id' ||
        name === 'bot_id' ||
        name === 'thread_id' ||
        name === 'comment_id' ||
        name === 'token_id' ||
        name === 'installation_id' ||
        name === 'key_id' ||
        name === 'conversation_id' ||
        name === 'parent_comment_id' ||
        name === 'anchor_id' ||
        name === 'position_id'
      ) {
        if (rewriteInt64PropToString(prop)) rewritten += 1
      }
    }
  }
  return rewritten
}

const rewrittenPathCount = rewriteSnowflakePathParams(merged)
const rewrittenSchemaCount = rewriteSnowflakeSchemaProps(merged)

// 输出文件
fs.writeFileSync(outputFile, JSON.stringify(merged, null, 2))
console.log(`Swagger JSON 文件合并完成，已生成 ${outputFile} 🎉`)
if (rewrittenPathCount > 0 || rewrittenSchemaCount > 0) {
  console.log(
    `Rewrote snowflake ids to string (path params: ${rewrittenPathCount}, schema props: ${rewrittenSchemaCount})`
  )
}
