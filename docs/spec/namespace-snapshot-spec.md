# Mega Namespace Snapshot：服务端实施 Spec

状态：Draft v0.3，2026-09-06。基线 `c4c79bc195541a13ac1505b94728c81a8ff3d603`。本文是目标设计，不是当前服务端能力清单。已实现的基础包括 import 固定 commit 解析、source identity 契约及 source/scope 证明的 additive schema 与存储；未部署或开放 snapshot capability。D1（完整 native + import 原子组合视图）与 D4（安全启用门槛）已获用户确认；D2/D3 尚待确认。

跨仓跟踪：[ScorpioFS #55](https://github.com/gitmono-dev/scorpiofs/issues/55)，关联 [#42 Snapshot](https://github.com/gitmono-dev/scorpiofs/issues/42)。配套客户端规范为 ScorpioFS 仓库的 `docs/spec/monorepo-versioning.md`；本文细化 Mega 的写入、存储、API 与迁移责任。基础实现持续审阅入口为 [Mega Draft PR #2181](https://github.com/gitmono-dev/mega/pull/2181) 与 [ScorpioFS Draft PR #56](https://github.com/gitmono-dev/scorpiofs/pull/56)；基础契约及测试入口见 [source-snapshot-v1.md](source-snapshot-v1.md)。完整 namespace 发布事务、所有写入者接入、GC/lease、HTTP/FUSE 联调及受控更新仍是未完成的交付门槛，不能由基础单测或 Draft PR 创建替代。

## 1. 交付目标与非目标

Mega 提供两个可独立验收的能力：

- `source-snapshot.v1`：一个经过验证的 source/scope/commit，其 tree/blob 读取不再解析实时分支。
- `namespace-snapshot.v1`：服务器发布原生 root 与 import 挂接索引组成的不可变视图，`latest` 只读发布指针。

先修单 source 历史读取，但它不是全库快照完成的证据。整个 view 必须同时固定内容和路由；ScorpioFS 的只读挂载不能补造 Mega 从未记录的挂接历史。

非目标：Git 全局线性历史重写、任意来源的原子跨仓 commit、自动展开 LFS/submodule、运行进程透明换代、迁移旧历史时猜测缺失的依赖版本。Libra 管 VCS，Mega 管发布，ScorpioFS 管投影；不让 Mega 接管工作区 upper/HEAD/index。

## 2. 源码核对结果

下列链接相对本仓库，行号以基线为准。实现时重新核对，不以注释代替执行路径。

| 证据 | 当前行为 | 改动结论 |
| --- | --- | --- |
| [ImportApiService](../../ceres/src/application/api_service/import_api_service.rs)，`get_root_tree`，约 117 行 | refs 参数被忽略，读取当前默认分支 | 新 resolver 必须显式解析 commit/ref；保留 legacy latest 行为于旧 API |
| [tree_ops](../../ceres/src/application/api_service/tree_ops.rs)，`get_binary_tree_by_path` | 先查当前 path，再校验可选 oid | 新接口从固定 root/OID 读；不能把旧接口的 oid 校验包装成历史读取 |
| [api_handler](../../mono/src/api/mod.rs)，约 82 行 | 按当前 import_dir 与 `git_repo` 切换 handler | snapshot router 不调用这个实时路由函数决定历史路径归属 |
| [mega_commit](../../jupiter/callisto/src/mega_commit.rs) 与 [mega_refs](../../jupiter/callisto/src/mega_refs.rs) | commit 没有 scope 字段，ref 有 path；scope clone 可生成新 commit | 增加持久化 scope 证明，不能靠最新 ref 反推所有历史 commit 的 scope |
| [共享事务](../../jupiter/src/storage/mod.rs)，`begin_db_transaction`，约 327 行 | monorepo/import 元数据使用同一应用连接开事务 | 新 namespace 元数据加入此事务；当前不必引入跨数据库提交协议 |
| [import post-receive](../../ceres/src/application/code_edit/post_receive/import.rs) | 分支 ref 和原生占位路径 attach 有联合事务、root CAS | 保留现有保护，并增加 binding/published pointer/逐 ref expected-old 校验 |
| [git_db_storage](../../jupiter/src/storage/git_db_storage.rs)，`update_ref_in_txn` | 读行后写新值，接口没有 expected-old 参数 | root CAS 不能替代每个 import ref 的并发租约校验 |
| [网页编辑](../../ceres/src/application/api_service/import_api_service.rs)，`save_file_edit`，约 374 行 | 先生成 tree，再读默认 ref，最后单独 update_ref | 一次固定 base 构建修改，使用共同 publisher；并发变化返回冲突，不拼接不同 base |
| [transport 建仓](../../ceres/src/transport/protocol/mod.rs)，约 168 行 | receive-pack 准备阶段可能先保存 git_repo，再接收对象；当前已拒绝删除默认分支 | 登记与已发布 binding 分开；保留默认分支删除保护并在事务内重验 |
| [smart receive-pack](../../ceres/src/transport/protocol/smart.rs)，约 334–403 行 | finalize 成功之后才构造各 command 的 report-status；tag 提前写入 | 不声称现有分支在 finalize 前回报成功；新 publisher 必须仍在成功边界之内 |
| [Mono merge](../../ceres/src/application/api_service/mono/cl/merge.rs)，`apply_update_result` | 事务写 tree/commit/ref，候选 ref 更新在事务前计算 | 接入 publication CAS 和读集校验，避免陈旧候选覆盖并发发布 |

搜索命中的 `build_trigger/service.rs::create_repo_and_save_ref` 位于 `#[cfg(test)]`，不是新生产建仓入口。当前发现的 artifact GC 管理另一类对象；本次没有验证到覆盖 Git snapshot 闭包的保留协议，不能沿用 artifact GC 的完成状态宣称 Git 对象已被保护。

## 3. 身份与 scope 验证

### 3.1 不可变身份

`SourceSnapshot = {source_id, scope_path, commit_oid, root_tree_oid, object_format}`。

单 source 身份的已实现编码、校验器与两仓共享向量见 [source-snapshot-v1.md](source-snapshot-v1.md)。它不替代后续 namespace view/index 编码、发布或保留闸门。

- `source_id` 是实例内永久身份，映射 backend kind 与现有 repo_id；删除/改路径不能重用该身份。建议持久 UUID，现有整数 repo_id 只作内部关联。
- 原生主仓只有一个 source，但可有多个 scope；import 各自独立 source。路径既不是 source ID，也不能供客户端指定任意后端 URL。
- 直接 root/scope commit 的 `root_tree_oid` 等于验证后的 commit.tree；从一个已证明的 native source 派生子目录时，保留 base commit provenance，root_tree_oid 则是沿固定 base tree 到目标 scope 验证得到的 subtree。这两种证明必须区分，不能无证明替换根树。scope `/project/a` 的 root 已在 a 内，读取 `src/lib.rs` 不能再拼接 `project/a`。
- 同一 commit 字节可出现在多个有效 scope；证明表是多对多关系，不设 `commit_oid → 唯一 scope` 假设。
- M1 对象格式只宣布已实现的 SHA-1；类型预留其他算法，未知算法显式拒绝。

`NamespaceView = {schema_version, instance_id, native, bindings_root, overrides_root?, materialization_policy}`；`view_id` 是规范化 descriptor 的 SHA-256 digest。binding 内容固定 `{mount_path, source_snapshot, source_subpath, policy}`，不得保存一个待读时解析的 branch 作为内容身份。

`publication_seq` 是实例发布序号；`view_id` 是内容/来源描述身份；客户端 `generation/delta_seq` 不由 Mega 分配。发布时间、租约、actor、浮动 selector 放在发布记录，不进入 view hash。未发布的候选 view 也可以有 view_id，但不能冒充 publication_seq。

`projection_key` 是客户端的有效投影身份；服务端不以它替代 provenance。客户端还需考虑访问域、合成 stat 策略与 inode 规则。

### 3.2 Scope 证明

拟新增 `source_commit_scope`，唯一键 `(source_id, scope_path, algorithm, commit_oid)`，保存 root_tree_oid、证明类型和可审计来源（产生该对象的 ref mutation/父 scope 映射/已发布 root）。

证明在原生 root/子 scope commit 创建、scope clone 派生、CL 接收、merge 生成路径 commits 时一起记录。已知 root commit 可沿已验证 root 历史建立 root-scope 关系；不能把任意存在于 `mega_commit` 的对象默认视为 `/`。

存量历史子 scope 没有可靠证明时返回 `SOURCE_SCOPE_UNVERIFIED`。允许显式管理回填经过验证的映射；不从如今同名目录或已被清理的 child ref 猜测。已固定 descriptor 不因 child ref 被 `remove_none_cl_refs` 清理而失效。

### 3.3 Selector

请求为带类型联合：`published_view(view_id|latest)`、`source_commit(source_id, scope_path, commit_oid)`、`source_ref(source_id, scope_path, full_ref_name)`。ref 必须完整限定 `refs/heads/...` 或 `refs/tags/...`；branch/tag 同名不自动猜测。

tag 解析返回 ref OID、必要的 annotated-tag peeling 链与最终 commit。树/blob tag 不是合法 commit selector；循环、超深链或 target 不可用明确失败。原有 tag 表与 ref 表存在两种表示，resolver 需要回归两类创建入口，不能假设所有 ref OID 都直接是 commit。

裸 tree 读取是对象操作，不形成可宣称 commit provenance 的 SourceSnapshot。source_ref 只解析一次，返回固定身份后所有后续读取不再跟随 ref。

## 4. 挂接索引与目录语义

索引输入是已发布 binding，不是每次读 `git_repo + import_refs`。原生树在 import 边界的占位内容由 binding 替换；聚合目录合并原生子项与固定 binding 子项，同名非声明替换冲突拒绝。跨 scope override 不允许穿过 import source 边界。

路径以组件匹配；`/rust` 不匹配 `/rust_v1`。v1 采用有效 UTF-8 路径组件，不做大小写折叠/Unicode 归一化；拒绝 NUL、`.`、`..`、重复分隔等非规范输入，非 UTF-8 名称明确返回 unsupported，不静默改名。Git 路径语义不套用 Windows 路径规则。路径编码规则进入 schema 版本。

建议实现持久化压缩 byte-radix/Merkle trie：组件间使用禁止出现在名称中的 NUL 作为内部边界，内部节点最多 256 个分支，value 保存独立 binding digest。节点大小、最长路径、递归深度有硬上限；压缩长 label 必须仍受节点上限约束。

仅说“重写祖先节点”还不够：根节点若内嵌百万 children，单次更新仍是 O(R)。首个索引 PR 必须证明节点 fanout/大小受限，更新 b 个 binding 的成本受变动 key 长度和受限节点数控制；持久化旧节点继续被旧 view 引用。

按 prefix seek 和分页，不在每次 mount 或每页 readdir 扫描全 registry。cursor 绑定 view_id、prefix、最后排序 key、schema 与查询参数并防篡改；续页重新鉴权。ScorpioFS directory handle 绑定该 view，不能跨 view 使用 cookie。

百万 binding 测试记录 node reads/writes、bytes、峰值内存和分页工作量，不只报告平均耗时。初始全量建索引允许 O(R)，在线单点发布和小工作集 mount 不允许。

## 5. 元数据模型（拟新增，不是现有数据库表）

| 表/存储 | 关键字段与约束 | 用途 |
| --- | --- | --- |
| `snapshot_source` | source_id PK，instance_id/kind/repo_id，状态；repo 身份不可重用 | 与可移动路径解耦；删除后保留 tombstone |
| `source_commit_scope` | §3.2 复合唯一键、root tree、proof | 验证 native scope 和历史读取 |
| `namespace_binding_head` | source_id、当前 mount_path、selected_ref、policy、revision、active/staged；活动路径唯一 | 当前发布政策/配置；不是历史读源 |
| `namespace_node` | digest PK、schema、canonical bytes；不可原地覆盖 | binding trie 与不可变 binding values |
| `namespace_view` | view_id PK、canonical descriptor、native root、bindings_root | 可重放 view；记录可与多次发布关联 |
| `namespace_head` | instance_id PK、publication_seq、view_id、writer_epoch | 唯一默认发布指针和 CAS/fencing 条件 |
| `namespace_publication` | `(instance_id,seq)` 唯一、view_id、parent_seq/view、reason、operation_id | 发布历史，不把 parent 写入 view hash |
| `snapshot_operation` | `(actor_domain,operation_id)` 唯一、request_digest、receipt | 响应丢失时可查询；同 key 异 payload 拒绝 |
| `snapshot_pin` | pin_id、target kind/id、owner、expires_at、state | view/source/prepare 的持久保留根 |
| `namespace_outbox` | event_id 唯一、seq、view_id、delivery_state | 与 publication 同事务，提交后幂等通知 |

`selected_ref` 是发布政策，绑定自身始终是 commit/tree。一个 source 的默认 binding 在 v1 只选一个 ref；拒绝多个默认标志的存量异常，不依赖 `.one()` 随机选中结果。以后显式多个挂接位置需要独立政策与冲突规则，不通过现有 repo_path 一对一映射悄悄扩展。

数据库结构迁移放 `jupiter-migrate`，通过 SeaORM 工具生成 Callisto entities；按仓库约束，helper 放 `entity_ext`，不手改生成 Model。大规模回填作为可恢复的 application backfill，不塞进自动启动的 schema migration 长事务。

字段编码、digest 域分离、nil/空索引、排序及未知字段处理必须在 G01 给出共享 golden vectors 后冻结。当前 JSON/Rust 草图不是任意序列化即可互通的实现标准；canonical bytes 未冻结前不发布 v1 capability。

## 6. 发布事务与并发约束

统一应用服务接收 `PublishPlan {operation_id, expected_head, ref_read_set, binding_read_set, prepared_objects, native_change?, binding_changes}`，返回 `PublicationReceipt {seq, view_id, outcome}`。HTTP endpoint 名称不决定领域模型；Git 与网页编辑调用同一服务。

```text
读取固定 base/head → 构造并验证 candidate objects/index
  → 建立 prepare pin，确认对象持久可读
  → 同一个 DB transaction：校验 expected/ref/binding 读集
       + 条件更新 refs/登记政策 + 持久化 scope proof
       + 写 view/publication/operation/pin/outbox
       + CAS namespace_head
  → COMMIT → 回报内容发布成功 → outbox 异步投递
```

这是一个数据库事务，不是两个顺序提交的 ref txn 和 namespace txn。事务内新 tree 元数据可同事务写入，外部 payload 必须在此前持久化；事务失败的 prepared 对象不是已发布数据，prepare pin 到期后才可回收。

要求：

1. `UPDATE ref ... WHERE ref_id = expected_old`；新建用唯一约束防并发重复，删除也验证 expected-old。任何受该发布事务管理的 ref 校验失败，整个事务回滚。
2. `namespace_head` CAS 同时比较 expected seq/view/epoch，影响行数必须恰为 1。Redis lock 只优化竞争，不是正确性证明；租约锁过期不能让 stale writer 发布。
3. native candidate 基于哪个 root 构建，必须校验同一 root；网页编辑的 tree 与 commit parent 来自同一固定 base。不能失败后只把 parent 换成 latest 继续写旧 tree。
4. 失败后重新读取 base、重建计划并重新校验；自动重试只适用于仍满足调用者 expected-old 条件的无冲突操作。用户提交的陈旧修改返回 409，不自动覆盖并发修改。
5. 同一 push 的多个 branch command 应先计算事务成功后的 ref 集，再按 selected_ref 选择默认 binding；不能把第一条 push command 当全库要发布的版本。
6. 无可见内容/路由变化时可以只提交 ref/operation；不强制增加 publication_seq。若现有流程确实创建了新的 native root commit，即使 tree 相同，provenance 变化仍按新 view 发布；去除这类额外 root commit 是另一个兼容性优化。
7. `latest` 从已提交 namespace_head 读取，不拼读几个当前 HEAD。固定 view 读取无需长事务；resolve latest 与创建 pin 需要防止 head 前进后旧 view 被 GC 的竞态。
8. 响应丢失不代表事务失败；operation receipt 是结果查询依据。outbox 重投不重做 ref 更新。内容已提交而邮件、CL 展示状态或通知失败时，明确 committed 状态，不声称 rollback。

PostgreSQL 普通 begin 不自动提供跨多次查询的一致旧快照；在线构造基于不可变旧 view，附加可变读全部进入条件校验。SQLite 使用支持的事务模式/条件写重试，不照搬行级锁 SQL；两个后端都测试。若部署另有对象存储一致性或多个元数据 DB，需重新审查此假设，不能直接开启 capability。

## 7. 写入入口覆盖矩阵

| 入口 | 需要实施的行为 | 默认 namespace 是否推进 |
| --- | --- | --- |
| 原生 CL merge / 网页编辑最终 merge：`mono/cl/merge.rs::apply_update_result` | 同一 publisher 原子保存 root/path commits、proof、相关 refs；保留 admin-file 检查 | main 可见 root 变化时推进 |
| 原生 scope clone：`transport/pack/monorepo.rs` 与 `code_edit/utils.rs` | 生成派生 commit/ref 时写 scope proof | 单纯 clone 缓存/证明生成不推进 |
| 原生 CL push：`persist_mono_refs`、post-receive | 固定候选 base/scope/head，原有 CL 状态流保持 | 未合并 CL 不推进；生成独立 candidate view |
| 原生路径 attach：`mono/sync.rs` | root 真变化与 proof 纳入 publisher；派生 path refs 同步不能篡改已发布 root | 有可见 root 变化才推进；与后续 merge 是一个还是两个业务发布需明确 |
| import 首次 receive-pack：`transport/protocol/mod.rs` + `post_receive/import.rs` | 提前登记仅 staged；首个有效 selected commit、占位路径与 binding 一起发布；事务内再次检查父子路径冲突 | 内容成功后才新增名字；失败 push 不产生空 binding |
| import 默认/selected branch push | expected-old 校验 + binding 指向新 commit + 原生 attach（若实际发生）同事务 | 推进；旧 view 仍固定旧 commit |
| import 非 selected branch push/删除 | 只更新相应 ref；保留当前默认分支删除禁令并在事务内重验 | 不因该 branch 本身推进；现有额外 root 变更按 §6.6 处理 |
| import 网页 `save_file_edit` | 单 base 构造；objects prepare + selected ref CAS + binding publish | 推进；不允许绕过 D2 不可变策略 |
| import tag REST / Git tag 写入 | ref 与 tag metadata 一致；selector 正确 peel；禁止 snapshot 读回查标签 | v1 固定 commit/selected branch 的 binding 不随 tag 移动；新 resolve 看到新 tag |
| 登记/取消挂接/改路径/换 selected ref、import_dir 变更 | 这是需补齐的管理操作，不声称当前已有完整 API；走 publisher，历史 binding 不变 | 在一个新 view 中原子改变路由 |

默认分支删除已在 transport 预检查中拒绝；本计划不是补一个“从未存在”的检查，而是把它变成所有相关写入口的事务内约束。移除仓库挂接与删除分支是两种操作，不能自动选一个剩余分支冒充用户意图。

Git report-status 维持现有支持的原子/非原子语义，不额外宣称 tags 和 branches 已全批原子化。现有 tag 提前写入需单独回归失败行为；v1 不支持自动跟随 tag 的发布政策，避免把这一差异藏进 namespace 承诺。

未来脚本/导入器不可绕开 publisher 直接改活动 refs/registry。上线前用写入口审计与 CI 检查约束低层调用；禁止/隔离未接入的管理写入，再宣告完整 namespace capability。

## 8. API 与代码归属

遵循 [架构约束](../architecture.md) 和 [Ceres 边界](../../ceres/README.md)：mono 是薄 HTTP router；Ceres 应用服务不依赖 axum/transport 实现；Jupiter 负责存储。新增 REST DTO 在 `ceres/src/model/snapshot.rs`，不是直接塞入 Orion 的 `api-model`。

| 拟改动位置 | 责任 |
| --- | --- |
| `ceres/src/application/snapshot/{resolver,publisher,bindings,lease,mod}.rs` | 独立 SnapshotApplicationService；通过对象/存储 port 复用现有能力 |
| `ceres/src/application/api_service/mono/app_services.rs` | 注入并提供 snapshot 服务 accessor，不继续扩张 MonoApiService 为全部实现容器 |
| `jupiter/src/storage/snapshot_storage.rs` 与 storage mod | 事务内条件写、view/index/operation/pin/outbox；无需 application 反向依赖 |
| `mono/src/api/snapshot.rs`、router、`api_doc.rs` | 鉴权 context、DTO→领域调用、统一错误、utoipa 注册 |
| `common/src/errors` 现有错误定义/转换 | typed snapshot errors 与 HTTP 映射，不新增字符串 `[code:...]` 协议 |
| `jupiter-migrate/src/migration` 与 Callisto | additive schema + generated entities +回填状态 |

API 路径是候选设计，实际 OpenAPI 由 Rust/utoipa 生成；不维护一份与代码竞争的手写 OpenAPI 文件。

| 拟议 API | 必须保证 |
| --- | --- |
| `GET /api/v1/snapshots/capabilities` | instance、schema、算法、路径编码、source/namespace readiness 与 retention 限制 |
| `POST /api/v1/snapshots/resolve` | typed selector；原子获得固定 descriptor + pin/lease；明确 consistency |
| `GET /api/v1/snapshots/{id}` | 相同 ID 的规范 descriptor 不变；可用性/租约另作 envelope |
| `GET /api/v1/snapshots/{id}/bindings` | 固定 prefix/cursor，惰性读取受限索引 |
| `GET /api/v1/snapshots/{id}/tree` | 固定路由；entry 有 type/mode/OID/source、准确 size 或后续同对象 stat token |
| `GET /api/v1/sources/{id}/trees/{oid}` 与 `/blobs/{oid}` | 明确 source、类型、算法、snapshot 授权上下文；tree 原始字节可校验 Git hash |
| `POST /api/v1/snapshots/{id}/leases`、`DELETE /api/v1/snapshot-leases/{id}` | 幂等创建/续期/释放；期限使用服务器时间 |
| `GET /api/v1/snapshot-operations/{operation_id}` | actor-domain 限定结果查询，恢复丢响应；不可跨用户枚举 |

固定树 entry 若返回未知 size，ScorpioFS 必须在向 FUSE stat 报告前取得该 OID 的精确长度；不能用 0 占位。二进制 blob 返回文件原字节，Git 哈希按类型+长度头校验，不能错误剥掉文件本身的相似前缀。

## 9. 授权、保留与故障语义

**D4 已于 2026-09-06 获用户确认**：snapshot 读 API 默认关闭，只有显式配置 source/scope 读授权和对象保留策略后才允许启用。当前基线通用 Cedar guard 主要覆盖 CL，并有开发期 permit-all 策略；不能把接入该 guard 当成满足本门槛。配置缺失/无效、授权或保留实现未就绪时拒绝开启 capability；当前基础实现尚未暴露 snapshot HTTP 路由。具体 ACL/lease 配置与实现仍需在后续 API PR 中验证。

全局 object store 命中不是访问授权。snapshot lease 只保留数据，不赋予永久读权；鉴权仍用当前访问政策，撤权可返回 403，但不得换成 latest 的内容。descriptor/binding 分页也可能泄露私有路径，不能只保护 blob。

读请求携带由固定 tree walk 产生的 object ticket 或等价可验证上下文，限定 source、scope/root、类型与 OID；初始 root ticket 由 resolver 生成。客户端随意填一个存在的 OID 不证明其可达性。优先采用可惰性展开的路径/父 tree 证明，不为每个 mount 扫描整个可达闭包；ticket 防伪、过期与续期机制必须与 lease 配套，不能当作绕过当前 ACL 的能力令牌。

GC roots 至少包含有效 publication 保留窗口、source/view leases、prepare pin、候选 CL pin。以树/对象图进行标记，lease 创建/续期与回收共享 GC epoch 或等效协调；不得在检查过期后与续期竞争删除有效对象。repo tombstone 不能级联删掉旧 view 仍引用的对象。

若按 pack 删除，包内任一保留对象要求保留整个 pack 或先安全 repack；若有 deltified 对象，还必须保留解码所需的 base 链。只标记直接 tree/blob OID 不足以证明底层 pack 可删。未完成 Git 对象/存储层保留审计前，可采用明确配置的“不回收这些对象”试点，但不能宣称已实现有界 GC。

错误 envelope 为 `{code,message,retryable,details}`；公开响应避免泄露跨权限域对象是否存在。

| HTTP / code | 语义 |
| --- | --- |
| 400 `INVALID_SELECTOR/PATH` | 输入类型或规范路径不合法 |
| 403 `FORBIDDEN` | 当前授权不允许；如采用隐藏存在性策略可统一为 404 |
| 404 `SOURCE/OBJECT/PATH_NOT_FOUND` | 授权域内不存在；空目录是成功的空 entries |
| 409 `EXPECTED_VIEW_MISMATCH/REF_MOVED/BINDING_CONFLICT` | 条件不满足，不静默重基 |
| 409 `SOURCE_SCOPE_MISMATCH/SOURCE_SCOPE_UNVERIFIED` | 错误或无法证明的 commit scope |
| 409 `IMMUTABLE_BINDING/DEFAULT_REF_REQUIRED` | 违反已选择的发布政策 |
| 410 `SNAPSHOT_EXPIRED` | 对象可用性不再承诺；仅 manifest 尚存不是有效租约 |
| 422 `HISTORICAL_BINDINGS_UNAVAILABLE` | 旧主仓 commit 没有对应历史 catalog |
| 501 `CAPABILITY_UNSUPPORTED` | 如不支持的算法、非 UTF-8、LFS hydrate |
| 503 `OBJECT_UNAVAILABLE/PUBLICATION_NOT_READY` | 暂时不可读或未就绪；没有 latest fallback |

服务端对象错误映射成客户端 I/O 错误与独立诊断，不得伪造空文件/空目录。租约时长与历史保留期是部署参数，在实施前确认；这份 spec 不擅自承诺永久保留。

## 10. 迁移、开启与回退

1. **Additive schema**：只建新表/索引，旧读取继续。迁移前备份与校验，按 [migration workflow](../../jupiter-migrate/README.md) 测 PostgreSQL/SQLite 升级；生产回退不 drop 新历史表。
2. **Source capability**：按源历史读与 scope proof 到位后可独立开启；未具备完整路由历史就不返回 namespace capability。
3. **Writer fencing**：全部服务实例升级到能遵守 publisher 的版本；短期阻断旧 writer/管理脚本。writer_epoch 只有所有入口执行检查才有效，不能把新列本身当作旧二进制已被隔离。
4. **初始 catalog**：推荐第一版维护窗口内暂停有关元数据写入，获取一致 native root/registry/default refs，验证对象和 scope、构建索引并保存可恢复回填进度；遇无默认 ref、重复默认、嵌套冲突或对象缺失，列入异常清单，不发布缺项“完整”视图。
5. **Cutover**：在仍受写屏障保护时发布 seq 初始值与完整 view，切换写入口后解除屏障；旧 API 可继续提供 latest，但新 ScorpioFS 只使用 snapshot API。大规模回填过长时另设计在线 changelog/catch-up，不能只用普通事务分页扫描冒充一致快照。
6. **Shadow/read compare**：固定时间点对比独立物化 oracle，不将活跃 latest 的漂移误判为快照错误。后台可以审计 head/ref/binding 一致性，但后台补偿不是原子发布的替代品。
7. **Rollback**：先停新发布，保留既有固定 view 读取和 leases；回退的服务若会绕过 publisher 写入，必须停相关写流并撤下 namespace latest capability。禁止静默用旧实时路由服务已分配的 view_id。

历史起点必须公开：初始 seq 之前缺 catalog 的主仓 commit 仍只能读 native scope，或由用户提供显式 bindings 构造 `consistency=explicit_composition`；后者可重放，但不是补回了当年的全库原子快照。

## 11. 第一轮 PR 切片与退出条件

下列是实施工作包，不是已创建的 Mega issue/PR；不提供未经排期的完成日期。

| 包 | 交付范围 | 退出门槛 |
| --- | --- | --- |
| G01 契约/fixture | typed selectors、路径规范、canonical bytes/golden vectors、固定 native/import A/B 场景；两仓共同消费 fixture | digest 跨语言一致；歧义 selector/错误 scope 拒绝；D1 影响功能分级 |
| G02 单 source 历史读 | Import refs 修复的新 resolver、scope-aware tree/blob、权限与错误；scope proof 的最小存储 | MG01–MG04/MG13；旧 API 回归；不宣称 namespace 已完成 |
| G03 发布存储核心 | additive schema、bounded binding trie、publish CAS/read-set/receipt/outbox、pins | MG05/MG06/MG09/MG12/MG15；两个数据库后端 |
| G04 接入全部 writer | §7 矩阵：native merge、import push/web、建仓 staging、政策变更与 proof | MG07–MG11；每个生产入口均有覆盖，无后台补偿窗口 |
| G05 迁移/租约/能力上线 | 回填、writer fencing、保留根与对象存储审计、capabilities、故障演练 | MG14–MG17；保留参数明确，完整性审计通过 |
| G06 ScorpioFS 联调 | 真 Mega 双版本挂载、分页、惰性旧 import、并发发布、租约错误传递 | ScorpioFS V01–V09/V14/V15/V17 与独立 oracle；无 FUSE/有 FUSE 分层 |

G02 可以与 ScorpioFS fake backend/CAS 类型工作并行，G03/G04 不等“共享 lower 架构选型”。先完成一个 native + 一个 import 的真实端到端切片，再扩规模，避免只做大量表和 endpoint 而未证明历史读取。

## 12. 验收与故障注入

所有 MG 测试是拟新增，尚未运行。fixture 的期望内容由独立 Git object 物化与显式 binding composition 得到，不用被测 resolver 生成期望值。

| ID | 场景及断言 |
| --- | --- |
| MG01 | import branch 从 A→B，固定 A 的 tree/blob/size 永远是 A；同路径 A/B 同时可读 |
| MG02 | native root/scope 同路径寻址、同 commit 多 scope、缺证明、清理 child ref；证明不丢且不重复拼前缀 |
| MG03 | annotated/lightweight tag、同名 branch/tag、tree/blob target；解析只做一次且错误明确 |
| MG04 | 空目录、缺目录、旧 oid、损坏 Git bytes、symlink/mode；不能以空成功代替错误 |
| MG05 | native merge 与 import update 并发：线性化结果是两个可串行化的新 view，不丢任一成功更新 |
| MG06 | same ref 两个 expected-old writer：最多一个成功；陈旧网页编辑不把旧 tree 接到新 parent |
| MG07 | 多 branch push 的 selected ref、非默认 push、默认删除、失败 tag/branch 混合；按承诺返回，不任取第一条 command |
| MG08 | 新 repo 登记后 unpack 失败、并发父子仓登记、取消挂接/改路径；失败新 repo 不出现在发布目录，旧 view 路由不漂 |
| MG09 | objects prepare、ref write、view write、head CAS、commit、response、outbox 各处 crash；已提交结果可查、未提交不出现 |
| MG10 | 网页编辑/native merge/管理入口的绕过测试；全部可见 mutation 都能查到对应 publication 或明确 no-op |
| MG11 | 版本目录 D2 两种政策分别测：不可变拒绝所有写入口的第二次内容发布；可变策略保留旧绑定/对象 |
| MG12 | 百万 binding 单点更新、小前缀 mount、跨多页目录：节点与内存有界，不扫描整个 registry，cursor 不串 view |
| MG13 | 猜测另一个 repo 的 OID、已撤权 lease、私有 binding、跨域 CAS 命中；均不能绕过授权 |
| MG14 | pin 创建/续期与 GC 竞争、pack/base 保留、repo tombstone、lease 过期；不删有效保留对象 |
| MG15 | PostgreSQL/SQLite 同样的事务失败、竞争、幂等 key 异 payload；无仅在某后端成立的保证 |
| MG16 | 回填中断续跑、缺默认 ref/对象、旧 writer、首次发布窗口和回退；不误宣布 capability |
| MG17 | 初始 seq 前的历史 root 缺 binding catalog：422；显式组合标注非原子历史；后续首次惰性读取仍固定旧 import |

发布指标最少包含 publish_conflict/retry、prepared bytes、txn duration、outbox lag、active pins、retained bytes/pack amplification、index nodes read/written；标签不加入无界 path/OID。

## 13. 待确认与实施前闸门

产品决策仍沿用客户端 spec 编号，避免两仓各自解释：

- **D1 已确认（2026-09-06）**：Mega 原子发布 native root + 固定 import bindings 的完整 namespace view，ScorpioFS 固定该 view 读取。单 source 是中间工作包，不将全库原子一致性从本次目标延期。
- **D2 推荐**：明确标记为发布版本的目录首次发布后不可改，通用 import 默认 branch 仍可演进；不能仅从数字路径名自动判断政策。另一方案允许原地更新，但每次新绑定且保留历史。
- **D3 推荐**：运行 build 固定旧 view，新 build 用新 view；现有工作区受控切换。透明 live-refresh 不属于本服务端 PR 的承诺。

实施闸门：G01 冻结 canonical 编码/字段；G04 审计全部写入口；G05 明确实际 PostgreSQL/SQLite、对象后端、导入拓扑、维护窗口及保留期。基础设施验证可继续，不把这些未确认事项写成“用户已同意”。
