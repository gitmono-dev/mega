# Mega → ScorpioFS 文件传输协议 v1

状态：Draft，2026-09-06。本次仅设计，未增加 HTTP endpoint 或已部署 capability。
先读 [带实例的传输设计](../scorpiofs-transfer-design.md)。

已确认：以大量源码小文件为主，支持局部打包；大文件由 Mega 校验完整 Git blob 后生成分块表，客户端信任经认证的 Mega 所提供的对应关系。保留既有默认关闭、显式授权和对象保留门槛。workspace 如何切换版本仍由客户端 spec 定义，本协议不依赖透明切换。

## 1. 与已有实现的关系

当前 ScorpioFS [SourceReader](https://github.com/gitmono-dev/scorpiofs/blob/codex/system-paper-spec/src/snapshot/backend.rs) 逐层获取 tree，并通过 Bytes 返回整个对象，默认 tree/blob 上限分别为 16/64 MiB；它尚无批量对象协议和随机分块读取。旧 Dicfuse 的 inode 内容缓存也不能直接充当跨版本的对象缓存。

Mega 已有按父目录合并 blob 查询的 [blob_ops](../../ceres/src/application/api_service/blob_ops.rs) 和按仓库批量查询对象元数据的 [GitDbStorage](../../jupiter/src/storage/git_db_storage.rs)，可以复用查询思路。当前 helper 的实时路由、出错跳过和 Vec 全量读取行为不能直接成为这里的契约。

本协议新增传输 DTO 和可重建缓存，不向严格编码的 SourceSnapshot、NamespaceBinding 或 NamespaceView 中添加字段。包 ID、压缩方式、分块表不参与 view_id。相同版本使用不同压缩器仍表示相同文件。

## 2. 协议选择与边界

- 传输：HTTPS，优先 HTTP/2；允许 HTTP/1.1 连接池保持相同语义，未协商 h2 时不能声称获得 h2 性能。
- 元数据：UTF-8 JSON；字节大小和 offset 使用无符号整数，v1 最大对象 8 TiB，以 capabilities 声明的更小限制为准。
- 小文件批量：标准 ustar + 单个 zstd frame；不使用跨包字典、Git delta、Base64 或自定义二进制帧。
- 单个文件与分块：二进制 HTTP body。内容字节与压缩字节分别计数。
- source、path、OID、模式和错误规则继承既有 snapshot spec。v1 不展开 gitlink 或 LFS 指针，不自动跟随远端 symlink。
- JSON 示例中的 V100、A、P 等是标注过的教学简写，不是可通过严格 ID 校验的 fixtures。

Git pack 支持 delta/base 链，适合 Git 对象传输；本设计选择独立小包以限制解码依赖和重试单位。以后若实测证明 pack 更优，可以协商另一种编码，不把它作为首版依赖。[Git pack 格式](https://git-scm.com/docs/pack-format)

## 3. 能力协商及公共请求上下文

扩展现有 GET /api/v1/snapshots/capabilities 的传输 envelope：

~~~json
{
  "transfer": {
    "protocol_version": 1,
    "metadata": true,
    "small_packs": true,
    "cached_packs": false,
    "chunk_reads": false,
    "pack_encodings": ["tar", "tar.zstd"],
    "limits": {
      "directory_entries": 256,
      "metadata_bytes": 1048576,
      "lookup_paths": 128,
      "small_blob_bytes": 262144,
      "pack_objects": 128,
      "pack_raw_bytes": 4194304,
      "pack_tar_bytes": 5242880,
      "pack_wire_bytes": 6291456,
      "chunk_bytes": 1048576,
      "chunk_page_entries": 256
    }
  }
}
~~~

这是启用后的格式示例；当前不得返回 true。能力取客户端支持与服务端声明的交集，客户端还可施加更低资源限制。metadata、small_packs、cached_packs、chunk_reads 分别经过验收再开启，不宣称一个开关就完成全部功能。

挂载首先通过既有 resolve 原子取得具体 view_id 和租约。之后所有新路由均以 /api/v1/snapshots/{view_id} 为前缀，拒绝以 latest 作为文件读取 ID。

公共 header：

~~~http
Authorization: Bearer <current-access-token>
X-Mega-Snapshot-Lease: <lease-id>
~~~

租约负责保留，token 负责当前授权。请求在同一服务 origin 内完成，不重定向携带凭据的请求。路径出现在请求体或 query 时进行规范校验、正确百分号编码，禁止将用户路径用于拼接后端主机地址。敏感 header 与路径不进入默认访问日志。

路径解析由固定 view 的 native/bindings 完成。缓存命中、304、包缓存、分块缓存都在当前授权检查之后返回。只知道 OID、pack ID 或 chunk digest 不能获取内容。

## 4. 目录和 lookup：正文之前先获取元数据

沿用 GET /{view_id}/tree?path=...&limit=256&cursor=...。这里及后文的 /{view_id} 均省略公共 /api/v1/snapshots 前缀。

目录页示例：

~~~json
{
  "view_id": "V100",
  "path": "/project/app/src",
  "entries": [
    {
      "name": "main.rs",
      "kind": "file",
      "mode": "100644",
      "blob_oid": "A",
      "raw_size": 8192,
      "source_id": "MAIN"
    },
    {
      "name": "parser.rs",
      "kind": "file",
      "mode": "100644",
      "blob_oid": "B",
      "raw_size": 8192,
      "source_id": "MAIN"
    }
  ],
  "next_cursor": null,
  "pack_hints": []
}
~~~

- file/executable/symlink 必须有准确 raw_size；symlink 是目标文本长度。Git tree 不包含文件长度，Mega 从已验证的对象长度元数据批量读取，不能对每个文件发起远端 HEAD 或下载正文。缺元数据返回可重试 METADATA_NOT_READY，不能填 0。
- directory 使用单独的结构：name/kind/mode；真实 source tree 可带 tree_oid，聚合目录不伪造 Git tree OID。目录 FUSE 属性采用固定客户端规则，不把子项数量当文件字节数。
- gitlink 明确返回 kind=gitlink 和 commit_oid；不伪装为可自动遍历的子目录。
- cursor 绑定 view、规范目录路径、排序规则、最后扫描 key、查询参数及授权政策代次并防篡改。按 UTF-8 名称字节排序；服务端以索引 seek 合并固定 native/binding 子项，不能每页扫描整个 registry 或巨型 Git tree。
- 每页同时限制返回数、响应字节数和扫描工作量。ACL 过滤后可以出现空 entries + 非空 cursor；只有 next_cursor=null 才表示枚举完成。政策变化使旧 cursor 返回 CURSOR_STALE，从同一 view 重新枚举。
- lookup 的缺失只能由固定路径解析确认；网络错误或未加载完的一页不能产生 ENOENT。directory handle 与 cookie 固定 view 和本次枚举状态。
- 大目录的 source tree 可在接收或后台验证后建立可 seek 的派生条目索引，版本映射指向同一 tree；索引未就绪明确报错，不阻塞在线请求做无限全扫描。

新增 POST /{view_id}/transfer/lookup，body 为 {"paths":[...]}，最多 128 个绝对路径。单次请求在服务端沿固定树完成深路径解析，返回逐路径元数据或明确错误；共享父路径批量读取。这将深路径的网络往返收敛为一次，不宣称服务端只做一次树访问。

目录列表是受认证 Mega 给出的投影与长度信息，不是“一个目录页可独立通过 Git tree 哈希证明”。原始 tree API 保留完整 Git 哈希校验；如果将来要求对聚合分页进行独立密码学证明，需要另行提供完整证明链。

## 5. 按缺失列表打包

POST /{view_id}/transfer/pack 是只读、可安全重试的批量下载操作：

~~~json
{
  "encoding": "tar.zstd",
  "items": [
    { "path": "/project/app/src/main.rs", "expected_blob_oid": "A" },
    { "path": "/project/app/src/parser.rs", "expected_blob_oid": "B" }
  ]
}
~~~

规则：

1. 以 items 而非“客户端没有哪些全库对象”作为输入；最多 128 项、128 KiB 请求体。不发送全局 have 列表或 Bloom filter。
2. 服务端在固定 view 中解析每一项，验证当前权限、对象类型和 expected_blob_oid。输入路径必须在同一固定 SourceSnapshot/scope 内；跨 import 边界拆包，错误码 MIXED_SOURCE_BATCH。
3. 输入允许多个合法路径引用同一 blob，但授权逐路径检查，运输按 (kind, object_format, OID) 去重；总大小按唯一对象计算。
4. 任一项权限、版本、大小或对象状态不符合要求，整个请求在发送 200 前失败。返回非 200 JSON 错误；不把失败对象从成功清单中静默删掉。需要缩小请求时客户端拆分。
5. 每对象不超过 256 KiB，总 blob payload 不超过 4 MiB。使用已验证长度预检，并在读取中重新限制；未知大小不能通过无限读取“探测”。
6. 服务端先在受限内存或临时文件中构造并验证完整小包，得到 wire 长度与 pack ID，再开始响应。这有有限的打包等待，必须记录 time-to-first-byte；不能伪称零准备延迟。相同对象集合和编码请求可以合并构建，但每个调用方分别鉴权。
7. 每对象服务器读取时验证实际 Git 哈希；已通过受信任不可变存储验证的缓存可复用。客户端仍验证全部收到的对象。
8. POST 表示有请求体的读取，不创建发布操作，不推进 latest；代理不自动缓存 POST。复用由服务端包缓存管理。

响应：

~~~http
HTTP/2 200
Content-Type: application/zstd
X-Mega-Archive-Format: tar-v1
X-Mega-Pack-Id: sha256:<digest-of-wire-bytes>
Content-Length: <compressed-byte-count>
Cache-Control: private, no-store, no-transform
~~~

tar.zstd 是实际表示，HTTP Content-Encoding 不再设置 zstd，避免客户端或代理双重解压。客户端通过响应 type 和 archive-format 选择解码器。协商 encoding=tar 时返回 application/x-tar，同样限制 tar/wire 字节数。

小包为完成度边界；HTTP 200 仅表示响应开始，不等于包已验证完成。中途超时、长度不符、缺成员、压缩结尾异常都使本次包失败。

## 6. 小包格式和校验

解压后的 ustar 内容依次为 manifest.json、按 OID 字节序排列的唯一 blob 成员、两个 512-byte 零结束块。拒绝结束块之后的非零内容；可接受的零填充仍计入 tar 上限。

~~~json
{
  "schema_version": 1,
  "object_format": "sha1",
  "objects": [
    { "oid": "A", "kind": "blob", "raw_size": 8192 },
    { "oid": "B", "kind": "blob", "raw_size": 8192 }
  ]
}
~~~

manifest 最多 64 KiB，只包含上述字段；blob 文件名严格为 objects/blob/<40位小写十六进制OID>。manifest 与对象成员均为普通 tar 文件，uid/gid/mtime=0、mode=0644、空 uname/gname，使用标准 ustar header 和八进制 size；不允许 PAX 扩展、链接、设备、目录、稀疏文件或重复成员。

tar 模式与 Git 可执行位没有对应关系。Git symlink blob 作为普通对象字节运输；从包中不会创建符号链接。

客户端：

1. 限制 wire、zstd window、解压 tar、单成员和总 payload 字节数；限额在分配/写入前检查。一个 zstd frame，禁止跨包 dictionary、skippable frame 和拼接 frame；window 最大 8 MiB。
2. 先验证 manifest 中的对象集合与请求预期完全一致，再读取各成员。tar header 长度必须等于 manifest.raw_size。
3. 将成员流入受控临时缓存，同时计算 SHA1("blob " + decimal_length + NUL + payload)，与预期 OID 比较。任何类似 Git 头的文件前缀都属于文件内容。
4. 不调用“按 tar 路径解包到 workspace”的通用操作。只用已校验 OID 生成缓存位置；写入完成后原子替换到本地对象缓存。
5. wire bytes 计算 SHA-256 并对比 pack ID；检查 HTTP 完整结束、压缩结束、tar 结束与成员覆盖。包 ID 是压缩表示 ID，不是新的文件或 monorepo 版本。
6. 断流前已完整通过 Git 哈希验证的对象可以保留；未完成成员删除。重试重新列出缺失对象，形成更小的包。不能使用压缩字节 offset 恢复半个解压流。

采用标准 tar 的结构，但客户端采用比通用解包工具更严格的结束及类型检查。[tar 格式](https://www.gnu.org/software/tar/manual/html_section/Standard.html)；
zstd frame 和 window 规则参考 [RFC 8878](https://www.rfc-editor.org/rfc/rfc8878.html) 与 [RFC 9659](https://www.rfc-editor.org/rfc/rfc9659.html)。

## 7. 热点目录包：复用、选择与失效

目录页可以返回有界 pack_hints。示例中成员路径相对当前目录：

~~~json
{
  "pack_id": "P",
  "encoding": "tar.zstd",
  "wire_bytes": 614400,
  "raw_bytes": 1048576,
  "members": [
    { "name": "main.rs", "blob_oid": "A", "raw_size": 8192 },
    { "name": "parser.rs", "blob_oid": "B", "raw_size": 8192 }
  ]
}
~~~

这是节选示意；真实 members 必须列全并与 raw_bytes 相符。hint 可省略，最多覆盖当前返回页的 128 个直接子文件，和 entries 一起受 1 MiB 响应限制。页布局改变允许不给 hint，不影响目录正确性。

GET /{view_id}/transfer/packages/{pack_id}?path=<directory> 读取已存在的小包。Mega 必须验证该包的全部成员在请求目录和固定 source 下对应相同 OID，并检查当前每个成员权限、租约。内部包缓存命中不能跳过这个步骤。

热点包按直接父目录、source/scope、稳定名称排序分组，目标 1 MiB、上限同按需包。不跨 source，不无界递归收集子目录；超大文件排除。只有受请求触发或有预算的热点预热才构建。不同 view 中相同对象集合和编码可复用包字节，目录到包的关联另存并按版本验证。

插入文件可能改变相邻分组，不承诺包边界稳定。客户端缓存以 blob 为单位，因此分组变化不会使已有 blob 失效。

ScorpioFS 的初始选择规则：

- 当前明确需要/已有访问记录预计将用到的文件中，包内至少 75% 原始字节尚未缓存，且浪费预算允许时，才选现成包。
- 已缓存大多数文件时，POST 精确缺失列表；单次 GET 某个文件没有被强制转换为整包下载。
- 不将“readdir 看到了一个文件”当成“马上会读取文件”。无历史线索时由并发需求聚合；目录预取可关闭。
- 单文件阻塞读取立即发出，聚合窗口最多 1 ms，不等待包凑满；已经有并发请求的缺失优先组成包。
- 预取最多 2 个并发包、8 MiB 原始内容；高优先级读取、内存/磁盘压力或低命中率出现时暂停。普通读取不等待预取队列排空。

GET 包被缓存淘汰后返回 404 PACK_EVICTED，客户端用同一版本的精确对象列表重建；这不代表文件不存在。包 ID 丢失不能自动改读最新版本。

## 8. 单文件与大文件

新增 GET /{view_id}/transfer/blob?path=...&expected_blob_oid=... 提供不超过 1 MiB 的完整对象，返回原始 application/octet-stream，HTTP 内容编码保持 identity。其 Git 哈希验证同小包；空 blob 合法。256 KiB～1 MiB 文件走此接口，超过 1 MiB 默认走分块。

原先 source raw-object API 仍按其现有上限和完整哈希规则工作，用于兼容与独立校验；新客户端不得因 chunk endpoint 失败就无限制退回整文件下载。

大文件先请求 GET /{view_id}/transfer/chunk-map?path=...&expected_blob_oid=...&cursor=...：

~~~json
{
  "view_id": "V100",
  "blob_oid": "COMPILER",
  "map_id": "MAP",
  "raw_size": 3221225472,
  "chunk_size": 1048576,
  "chunk_count": 3072,
  "chunks": [
    {
      "index": 100,
      "offset": 104857600,
      "raw_size": 1048576,
      "sha256": "C100"
    }
  ],
  "next_cursor": "NEXT"
}
~~~

分页示意：首请求可带 start_chunk=100；后续只能用 cursor，cursor 绑定 view、blob、map、分页范围和授权上下文。每页最多 256 项、128 KiB，不能为了读取一块而下载整个 TB 文件的清单。

块信息结构：

~~~text
BlobTransferInfo {
    blob_oid, object_format, raw_size,
    chunk_size, chunk_count, map_id,
    state: preparing | ready | corrupt
}

ChunkEntry {
    index, raw_size, sha256
}
~~~

offset = index * chunk_size，不另存一个可矛盾的偏移。除最后一块之外长度固定；chunk_count = ceil(raw_size/chunk_size)。分块只面向大于 1 MiB 的 blob，没有“空块”的例外。

map_id 定义为 SHA-256：
ASCII "mega.blob-chunks.v1" + NUL，
再依次拼接 object_format 的单字节值 1（sha1）、20-byte blob OID、
u64-BE raw_size、u32-BE chunk_size、u64-BE chunk_count，
最后按 index 顺序拼接每项的 u32-BE raw_size 和 32-byte SHA-256。
ID 显示为 sha256: + 小写十六进制。

这项编码只用于派生分块表身份。客户端读取一页不能独立计算完整 map_id，也不能从 Git OID 推导分块哈希；它信任经认证 Mega 对分页内容、map_id 和 blob 的绑定。已获用户确认。全表被获取时可额外重算 ID，但这不是每次随机读取的前提。

读取块：

~~~http
GET /api/v1/snapshots/V100/transfer/chunks/MAP/100?path=/toolchains/compiler.bin&expected_blob_oid=COMPILER
Authorization: Bearer <token>
X-Mega-Snapshot-Lease: <lease>
Accept-Encoding: identity
~~~

200 返回完整块的原字节，Content-Length 必须精确匹配已取得条目。客户端计算 SHA-256，对照分块表验证后才把所需范围交给 FUSE。

一块是一个 HTTP resource；这里不使用文件 Range，也不把整文件 zstd 流切片。HTTP Range 对内容编码后的表示计偏移，和原文件 offset 容易混淆。[RFC 9110 §14.1.2](https://www.rfc-editor.org/rfc/rfc9110.html#section-14.1.2)

块完整性只在完整块验证后成立；断线重取这一块。若下载齐全部块，客户端可流式重算 Git blob 哈希后把它提升为完整 blob 缓存；只有部分块时保持“按可信分块表验证”状态。

## 9. 服务端存储与任务调度

需要的逻辑记录，不代表本次已新增数据库表：

~~~text
ObjectLength(domain, kind, algorithm, oid) -> verified_raw_size
DirectoryEntries(source_tree_oid, name) -> kind, oid
PackCache(domain, member_set, encoding_revision) -> pack_id, bytes_location, sizes
DirectoryPack(view/source-tree, directory, group) -> pack_id, member_paths
BlobTransferInfo(domain, blob_oid, chunk_size) -> ready map_id
ChunkTable(map_id, index) -> raw_size, sha256, storage_location
~~~

domain 是服务端授权隔离域，不能由客户端自己指定。文件身份与权限关联分别保存；同一物理对象去重不等于允许跨域读取。

- 新 Git 对象接收/合并路径应在已验证字节流上记录准确长度。存量对象受限回填；GET tree 不做同步无限回填。
- source/binding 路径解析合并公共前缀，按 source 和唯一 OID 批量查 size。禁止 N 次单对象查询替代一个真正的批量查询。
- 包构建按对象集合合并重复任务；服务实例维护 CPU、数据库、内存和临时磁盘信号量，多副本共享缓存但各请求仍鉴权。初始建议每实例 4 个 pack builder，最终依据 CPU 和存储能力配置。
- zstd 默认 level 1；预热可评估 level 3，已有压缩对象采用 tar 或原始单文件；不在请求线程进行无界高等级压缩。
- 大文件的分块任务必须读取重建后的 blob、验证完整 Git OID，再将块和分块表原子标为 ready。验证失败标为 corrupt，临时块不能对外可见。
- 大文件支持范围读取的原始存储可配合分块索引；Git pack/delta 先受限还原到磁盘或块对象存储。不能让每块读取重新产生 O(file_size) 解码工作。
- 对存量大 blob 建表会有一次 O(file_size) 成本。preparing 返回 503 CHUNK_MAP_NOT_READY + Retry-After，排队去重；新导入可在 ingest 时准备。没有完整图时不承诺“首次冷读取只需一块的服务端 IO”。
- 分块页和块存储一旦对客户端发布，其可用性要跟随 view/source 租约和请求 in-flight pin。回收与续租协调，不得把有效读依赖作为普通 LRU 删除。小包是可重建性能缓存，可提前淘汰。
- 原始 Git pack/base 保留仍由既有 GC 契约负责。分块缓存不是完成 Git 对象保留审计的替代品。

首次试点显式配置总缓存字节限额和临时磁盘限额，磁盘不足停止预热/构建并返回容量错误。实现必须有流式对象读取接口；不得用 Vec 容纳 GB 级文件后声称支持分块。

## 10. ScorpioFS 缓存与 FUSE 接入

~~~text
PathCache(domain, view_id, path) -> source, kind, mode, blob_oid, size
BlobCache(domain, algorithm, kind, oid) -> verified whole payload
ChunkCache(domain, sha256, raw_size) -> verified chunk payload
ChunkMapCache(domain, blob_oid, map_id, page) -> authenticated entries
~~~

路径缓存绑定版本；对象内容不强制绑定 view，以复用相同文件。内容命中前仍需有效的路径/版本和授权上下文。chunk 命中必须先取得对应 blob 的受信任 map entry，不能由裸 SHA-256 值直接读缓存。

具体接入：

1. 增加目录元数据客户端和请求调度层，让 lookup/getattr/readdir 使用准确大小；新的 lower 不采用旧的“未知大小暂填 0”行为。
2. 同一 (domain, kind, OID) 的并发缺失合并，取消一个 waiter 不取消其他仍需要的 waiter。
3. 请求分为 demand 和 prefetch，保留至少一个并发槽处理元数据/阻塞读取；不能仅依赖 HTTP/2 priority header。
4. 初始上限：8 个数据流、2 个元数据流、64 MiB 用户态传输缓冲，单包以流式临时文件解包。这些不包含内核 page cache 与持久化缓存，监控中必须分别报告。
5. 大文件新增 range-reader 接口供 FUSE read(offset,length) 使用，不让现有 read_file -> Bytes API 返回“其实只有一块”的伪完整文件。
6. 块覆盖公式为 floor(offset/chunk_size) 到 floor((min(offset+length,file_size)-1)/chunk_size)。checked arithmetic；length=0 或 offset>=EOF 返回空，越 EOF 裁剪。跨块依次拼接已验证部分，不用零填充尚未下载的洞。
7. 顺序读取可预取后续 2 块；随机访问降低预取，负载压力下停用。所有预取都固定相同 view。
8. 本地内容对象原子写入，进程重启清理不完整临时文件。FUSE 句柄携带既有版本与代次；旧响应不能填入新版本的路径/属性缓存。

缓存和 kernel page cache 无法收回已经交给应用的数据。服务端对新请求逐次检查当前授权；在线客户端定期确认授权，在拒绝后停止新读取并使可失效元数据失效。任何短期授权缓存有效期都需由服务端明确给出，缺省不授予离线权限；不宣称能瞬间撤销已读字节或 mmap 页面。

## 11. 重试、失败与降级

| 情况 | 服务端/客户端行为 |
| --- | --- |
| 401/403 | 当前请求失败，停止有关后台预取；不重试到其他 origin |
| 410 SNAPSHOT_EXPIRED | 停止相关版本读取，要求显式恢复租约或重新建立 workspace |
| 404 PATH_NOT_FOUND | 仅固定目录解析确认时产生 ENOENT |
| 404 PACK_EVICTED | 同一版本重新按缺失列表请求包 |
| 409 EXPECTED_OBJECT_MISMATCH | 目录与请求不一致，失败并检查固定版本；不自动接受新 OID |
| 409 CURSOR_STALE | 同一版本重开枚举，不能沿用旧 cookie |
| 413 BATCH_LIMIT_EXCEEDED | 按服务器上限拆分，不把一个大文件无限重试为小包 |
| 429 / 503 | 遵循 Retry-After 与有抖动退避，先暂停预取 |
| 503 METADATA_NOT_READY / CHUNK_MAP_NOT_READY | 有界等待后台准备，不报空文件，不悄悄整库预取 |
| 503 OBJECT_UNAVAILABLE / CORRUPT | 暴露 I/O 错误，不报告“路径不存在” |
| 压缩/哈希/长度错误 | 丢弃未验证数据；有限重试后 I/O 错误并诊断，禁止写入内容缓存 |

初始网络重试预算：单次逻辑读最多 3 次、总计 30 秒；后端准备任务可由调用方显式等待更久，不由内核读无限挂起。客户端取消请求时服务端停止无其他消费者的下载或任务；完整缓存包可供其他请求继续使用。

small_packs 不支持时，可以退回固定 source 的完整对象接口，但保持字节上限、授权与租约。cached_packs 不支持时使用精确列表包。chunk_reads 不支持且对象超限时明确失败，不能切换到 legacy latest。

## 12. 交付顺序及验收

| 切片 | Mega | ScorpioFS | 验收 |
| --- | --- | --- | --- |
| T1 | 目录准确长度、分页、固定路径 lookup | 元数据缓存、stat、逐对象基线 | stat 不下载正文，深路径一次 lookup RPC |
| T2 | 精确小包接口、标准编码、资源限制 | 合并缺失、校验解包、内容缓存 | 冷构建小文件包传输与逐对象基线对比 |
| T3 | 热点包缓存及 hints | 按需选择、限额预取、跨版本复用 | 更新 10/1000 文件只传缺失内容 |
| T4 | 大文件 map、随机读存储、保留 | range reader、块缓存与重试 | 3 GiB 文件局部读、故障与双版本 FUSE |

实际路由进入 mono，调度/路径/包服务进入 Ceres，持久化和流式对象读取进入 Jupiter/对象存储。ScorpioFS 在 snapshot 下增加协议客户端、对象缓存与调度器，由 Dicfuse lower 调用；Libra 不接管这些传输。

必须覆盖：

- Native + import A/B 两版本同时读，旧版本首次惰性请求不查询新 refs。
- 独立 Git oracle 验证解包对象，binary/空文件/可执行文件/symlink blob，重复 OID、Unicode 路径和恶意 tar header。
- tar 每个阶段断流、manifest 多/缺成员、超限、zstd window/字典/多帧异常、压缩及 Git 哈希不符。
- 同一缓存包在另一个 source、路径复用、权限撤销后的访问；包包含一个无权限文件时不能发送任何正文。
- 超大目录的分页字节/扫描上限、ACL 空页、跨版本 cursor、无效页不能产生假 ENOENT。
- 多个请求同时读同一 OID、部分取消、磁盘满、包构建失败、冷并发请求不会重复压缩 N 次。
- 分块 offset/EOF/跨块/超大整数、map 与 blob 不匹配、错误块摘要、部分下载不能提升完整 Git blob 状态。
- ingest 未完成、map 未就绪、租约与 GC 竞争、pack delta 还原；分别测试新导入与存量冷数据。

性能实验需使用同一固定版本、文件集合、服务端 CPU/IO 预算和客户端并发：

| 工作负载 | 核心指标 |
| --- | --- |
| 1,000 × 8 KiB 同目录文件，冷缓存 | 最多 8 次按需包请求，目录分页另计；对比并发单对象 GET |
| 上述文件只改 10 个，热缓存 | 理想受控场景新增正文为 80 KiB，允许 tar/压缩开销；不能下载其余 990 个正文 |
| 深 10 层单路径读取 | 元数据一次 lookup RPC，记录服务端树读取而非隐藏 IO |
| 百万目录项、少量子目录工作集 | 页工作量、数据库查询、RSS 不随全库大小线性增长 |
| 3 GiB 文件的 100 MiB 偏移读取 64 KiB | map ready 且关预取时一块 1 MiB 正文；map/HTTP overhead 另计 |
| RTT 1/20/80 ms，带宽 100 Mbit/s / 1 Gbit/s | 冷/热构建时长、p50/p95 首文件时间、请求数、wire/raw 字节、CPU |
| 并发构建与随机读取 | 队列等待、pack 命中、预取命中与浪费、峰值内存/磁盘、解码工作量 |

预取关闭/开启、tar/ tar.zstd、逐对象 HTTP/2 均作为基线。Git partial clone 也记录了逐对象获取的代价及批量预取的做法，但这不是本系统的加速数据。[Git partial clone](https://git-scm.com/docs/partial-clone)

本次完成的是协议设计。上述基准尚未运行；小包大小、并发和预取阈值必须通过 T2/T3 实测后调整，不能把算出来的请求数量当成已实现的构建性能。
