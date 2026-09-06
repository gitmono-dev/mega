# Mega 的 monorepo 版本发布设计

这份文档是设计入口。目标只有一句话：

> Mega 每次发布一份完整的 monorepo 版本清单，同时固定主仓内容和所有挂载仓库的内容。

实现字段、数据库表和编码见文末链接；理解本设计不需要先掌握这些内部细节。

## 为什么由 Mega 负责

Mega 的目录不是一棵普通 Git tree，它同时包含主仓原生目录、挂载在路径下的独立 Git 仓库，以及两者共同组成的聚合目录。

如果客户端只固定主仓 commit，却在读取时查询 import 的当前默认分支，就可能得到：

| 路径 | build 开始时 | import 更新后 |
| --- | --- | --- |
| /project/app | 主仓 M1 | 仍是 M1 |
| /third-party/lib | import I1 | 变成 I2 |

这不是可重现的 monorepo 版本。ScorpioFS 无法补回 Mega 从未记录的历史挂载关系，所以完整版本必须由 Mega 发布。

## 对外只有一份版本清单

~~~text
版本 ID
├── 主仓：固定 commit 和 root tree
└── 挂载表
    ├── /third-party/lib → lib 的固定 commit/tree
    └── /toolchains/rust → rust 的固定 commit/tree
~~~

版本 ID 由清单内容计算。主仓 commit、任一 import commit、挂载路径或政策改变，都会产生新 ID。

清单只保存固定 commit/tree，不保存“读取时再解析”的 branch。branch 用于决定下一次发布什么，不属于已发布内容。

Mega 另有 latest 指针指向最近成功发布的清单。latest 可以移动；具体版本 ID 永远不改变。

## 如何发布

所有改变用户可见目录的入口最终调用同一个发布服务：主仓 merge、import selected branch 的 push/网页编辑、挂载点增删移动、selected branch 或目录政策变化。

~~~text
固定当前版本作为 base
  → 准备并验证 Git 对象
  → 计算新的完整清单
  → 同一数据库事务：
      检查所有旧 refs 和旧 latest
      更新 refs
      保存清单、发布记录和操作回执
      条件更新 latest
      保存通知事件
  → 提交成功后才报告成功
~~~

refs 和版本指针不能分两次提交。每个 ref 更新都检查调用者看到的旧 OID；两个并发 writer 从同一旧版本出发时最多一个成功。

请求带幂等操作 ID。若提交后网络断开，重试查询回执，不再次修改 ref 或重复发布。

## release 目录

目录政策由元数据明确标记，不从名字推断：

- mutable：普通开发目录，可由后续发布更新；
- immutable_release：release 目录，首次发布后内容不可改变。

已经确认采用 release 不可变政策。Git push、网页编辑和管理接口都必须拒绝第二次内容变更，也不能先降级为 mutable 来绕过。新内容使用新的 release 路径。名字像 1.2.3 但没有显式标记的目录不会自动成为 release。

mutable 目录更新只产生新清单，不改写历史清单。

## 如何读取

读取具体版本时，Mega 从固定清单判断路径属于主仓还是 import，从该来源的固定 root tree 遍历，然后返回经过 Git 哈希验证的 tree/blob。

读取中不得查询当前默认分支，不得用今天的挂载表解释过去版本，也不得在历史对象缺失时回退到 latest。旧主仓 commit 若没有当时的挂载记录，应返回“历史绑定不可用”。

## 安全与保留

新读取能力默认关闭。启用前必须配置 source/path 授权、历史对象保留期、workspace 租约，以及 Git pack/delta base 的保留方式。

知道 OID 或命中全局 CAS 不构成授权。每次读取仍需验证当前权限和对象在指定版本中的可达性。租约防止对象被回收，但不绕过权限撤销。

## 内部名词只是实现手段

- stable source ID：仓库移动后仍能定位历史内容；
- scope proof：证明 commit/tree 属于指定目录或 import；
- immutable radix index：百万级挂载表可局部更新和按前缀读取；
- receipt：恢复“已提交但响应丢失”的请求；
- outbox：内容提交后可靠通知。

数据库中的 view、head、publication、operation 等表只是分别保存清单、latest、历史和回执。对外仍是一件事：发布或读取一份完整版本清单。

## 当前状态

已实现并测试：固定 source 读取、身份和目录证明、两仓共享清单编码、有界不可变挂载索引、SQLite/PostgreSQL 持久化，以及 ref 条件更新、latest CAS、回执和 outbox 的事务核心。

仍未完成，因此 PR 保持 Draft：真实主仓与 imports 的组合发布服务、所有生产写入口接入、release 政策执行、对象租约/GC/授权、正式 HTTP API，以及真实 ScorpioFS/FUSE 双版本验证。

## 详细资料

- [服务端实施细节](spec/namespace-snapshot-spec.md)
- [版本清单编码](spec/namespace-manifest-v1.md)
- [挂载索引](spec/namespace-index-v1.md)
- [发布事务核心](spec/namespace-publication-core.md)
