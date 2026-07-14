# Orion Scheduler

Orion Scheduler 是一个常驻运行的服务，负责接收来自 GitHub Actions 的 Webhook 事件，通过 [qlean](https://crates.io/crates/qlean) 拉起 QEMU/KVM 微型虚拟机，将 Orion 二进制文件及 runner 配置通过 SFTP 推送至 VM 内部，启动 `orion-runner` systemd 服务，并保持虚拟机长期运行以支持实时日志拉取。

```mermaid
flowchart LR
    Mono["mono / GHA"] -->|"POST /webhook"| Sched["orion-scheduler"]
    Sched -->|"qlean"| VMa["microVM domain A"]
    Sched -->|"qlean"| VMb["microVM domain B"]
    User["开发者 / 监控"] -->|"GET /status<br/>GET /vms/{id}"| Sched
```

单进程可同时托管**多台 VM**，唯一性由 `server_ws` 的**主机名（domain）**决定：同一 domain 最多一台；不同 domain 互不踢掉。

---

## 快速开始

### 前置要求

| 项目      | 要求                                                                                            |
| ------- | --------------------------------------------------------------------------------------------- |
| CPU 虚拟化 | 需启用 KVM；AWS EC2 环境应选用支持嵌套虚拟化的实例类型（`C8i` / `M8i` / `R8i`），并在实例 CPU 选项中开启 Nested virtualization |
| NBD 模块  | 构建自定义镜像前需加载：`sudo modprobe nbd max_part=8`                                                    |
| Rust    | 1.85 及以上（项目采用 Rust 2024 edition）                                                              |
| 网络      | 主机需存在 `qlbr0` 网桥；`/etc/qemu/bridge.conf` 中应包含 `allow qlbr0`                                   |
| 镜像      | 需构建 `debian-13-buck2.qcow2`（参见[自定义镜像](#自定义镜像)）；基础镜像缺失时构建脚本会自动下载                               |

### 最小配置（`target_config.json`）

从 `target_config.json.template` 复制并修改：

```json
{
  "log_dir": "/var/log/orion-scheduler",
  "orion_source_dir": "/home/user/mega/orion",
  "orion_binary_path": "/home/user/mega/target/debug/orion",
  "ssh_public_key_path": "~/.ssh/orion_vm_access.pub",
  "max_vms": 8,
  "default_image": {
    "image_path": "~/.local/share/qlean/images/debian-13-buck2/debian-13-buck2.qcow2",
    "image_digest": "sha256:753c28888c9d30fe4baef55c1d1dfa9a39431595eca940b7ad85d78d84f3d7a5",
    "image_disk_gb": 30,
    "image_cpus": 8,
    "image_memory_mb": 16000
  }
}
```

可选 `max_vms`：并发 VM 上限（按 domain 计数）；省略则不限制。

配置文件路径可通过 `CONFIG_PATH` 环境变量指定（默认：`target_config.json`）。

---

## 工作流程

`POST /webhook` 将触发一次完整的部署流程：

```mermaid
sequenceDiagram
    participant CI as GitHub Actions
    participant SC as orion-scheduler
    participant Q as qlean
    participant VM as microVM

    CI->>SC: POST /webhook { server_ws, scorpio_* }
    SC->>SC: domain = host(server_ws); 冲突/幂等检查
    SC->>Q: 仅关闭同 domain 旧 VM（若 replace/Failed）
    SC->>Q: KeepAliveMachine::new(custom_image)
    Q->>VM: 启动 QEMU + cloud-init
    SC->>VM: 注入 SSH 公钥 (orion_vm_access.pub)
    SC->>VM: SFTP 上传 orion + runner-config
    SC->>VM: sed 替换 SERVER_WS / scorpio.toml URL
    SC->>VM: systemctl start orion-runner
    VM-->>SC: journalctl 初始日志
    SC->>SC: 写入 log_dir
    SC-->>CI: 202 { vm_id, domain, status: provisioning }
    Note over SC,VM: 该 domain 的 VM 保持运行；其他 domain 不受影响
    CI->>SC: GET /vms/{vm_id} 或 GET /status?domain=
    SC-->>CI: { phase: running, domain, vm_ip, ... }
```

默认 **异步** 模式：`POST /webhook` 立即返回 `202 Accepted` 与 `vm_id` + `domain`，后台执行部署；客户端轮询 `GET /vms/{id}` 或 `GET /status?domain=` 直至 `phase=running`。

**同 domain 再次启动**：

| 当前状态 | 行为 |
|----------|------|
| 无 / Failed | 允许新建（Failed 覆盖） |
| Provisioning | `409 Conflict`，返回现有 `vm_id` |
| Running | 幂等 `200`，返回现有 `vm_id`；`replace: true` 时强制重建 |

GHA 等需同步等待的调用方可传 `"sync": true` 保留旧行为（阻塞至完成，返回 `200`）。

详细实现参见 `[src/orion_deployer.rs](src/orion_deployer.rs)` 中的 `handle_update` 函数。

### Mega UI / mono 代理

Admin 用户通过 Mega UI Orion Client 页调用 mono `POST /api/v1/orion/runners`（空 body）；mono 从 `build.runner_connect_domain` 拼接 `git.` / `orion.` 子域名推导 env URL 后转发至 scheduler。scheduler URL 仅配置在 mono 服务端（`build.orion_scheduler_url`），浏览器不直连 scheduler。

---

## API 端点

| 方法   | 路径                   | 说明                                                | 响应                                              |
| ---- | -------------------- | ------------------------------------------------- | ----------------------------------------------- |
| GET  | `/health`            | 服务健康检查                                            | `{ "status": "healthy", ... }`                  |
| GET  | `/webhook`           | Webhook 端点连通性检查                                   | `{ "status": "ok", "vm_id": null, ... }`        |
| POST | `/webhook`           | 触发部署（按 `server_ws` 域名唯一）                         | 默认 `202 { provisioning, vm_id, domain }`；Running 幂等 `200`；Provisioning `409` |
| GET  | `/status`            | 所有 VM 列表；可选 `?domain=` / `?vm_id=`               | `{ "count", "vms": [...] }` 或单机摘要              |
| GET  | `/vms/{id}`          | 按 vm_id 查询单台                                       | `{ phase, domain, vm_ip, ... }` / 404           |
| GET  | `/logs/orion/stream` | SSE 日志；需 `?domain=` 或 `?vm_id=`（多 VM 时）          | `text/event-stream`                             |
| GET  | `/scorpio/status`    | Scorpio 状态；可选 `?domain=` / `?vm_id=`              | JSON                                            |
| GET  | `/scorpio/config`    | 读取 scorpio.toml；可选选择器                             | `{ "path", "content" }`                         |
| POST | `/shutdown`          | **必须** `?domain=` 或 `?vm_id=`                     | `{ "status": "ok", "domain" }`                  |
| POST | `/shutdown/all`      | 关闭全部跟踪中的 VM                                      | `{ "status": "ok" }`                            |

### POST /webhook 请求体

```bash
curl -X POST http://localhost:8080/webhook \
  -H 'Content-Type: application/json' \
  -d '{
    "server_ws": "wss://orion.gitmega.com/ws",
    "scorpio_base_url": "https://git.gitmega.com",
    "scorpio_lfs_url": "https://git.gitmega.com"
  }'
```

| 字段                | 类型     | 必填   | 说明                                                       |
| ----------------- | ------ | ---- | -------------------------------------------------------- |
| `server_ws`       | string | 是    | Orion WebSocket URL，写入 VM 内 `.env` 的 `SERVER_WS`          |
| `scorpio_base_url`| string | 是    | 写入 `scorpio.toml` 的 `base_url`                           |
| `scorpio_lfs_url` | string | 是    | 写入 `scorpio.toml` 的 `lfs_url`                            |
| `target`          | string | 否    | 仅作日志标签（已废弃查表，GHA 可逐步移除）                              |
| `action`          | string | 否    | GitHub Actions 事件类型，仅作日志记录                               |
| `sync`            | bool   | 否    | 为 `true` 时同步阻塞至部署完成（默认 `false`，立即 202 返回）              |
| `replace`         | bool   | 否    | Running 状态下为 `true` 时强制重建（默认幂等返回已有实例）                 |
| `image_path`      | string | 否    | 本地 qcow2 镜像路径；未指定时使用 `default_image.image_path`           |
| `image_url`       | string | 否    | 远程 HTTPS URL                                             |
| `image_digest`    | string | 条件必填 | 镜像 SHA256/SHA512 校验和，提供 `image_path` 或 `image_url` 时必须指定 |
| `image_disk_gb`   | u32    | 否    | 虚拟机磁盘大小（GB）；未指定时使用 `default_image.image_disk_gb`         |
| `image_cpus`      | u32    | 否    | 虚拟 CPU 数量；未指定时使用 `default_image.image_cpus`               |
| `image_memory_mb` | u32    | 否    | 内存大小（MB）；未指定时使用 `default_image.image_memory_mb`           |

> `image_path` 与 `image_url` 互斥，不可同时指定。提供镜像参数时 `image_digest` 必须提供（格式：`sha256:...` 或 `sha512:...`）。未传任何 `image_*` 字段时，scheduler 使用 `target_config.json` 中的 `default_image` 块。

### GHA / 外部 webhook 迁移

旧版 webhook 通过 `target` 名称在 `target_config.json` 的 `targets` 中查表获取环境 URL。该机制已移除；调用方必须在请求体中内联传入 `server_ws`、`scorpio_base_url`、`scorpio_lfs_url`。`target` 字段仍可选，仅用于日志标签。

示例（GitHub Actions）：

```yaml
- name: Start Orion runner VM
  run: |
    curl -X POST "$SCHEDULER_URL/webhook" \
      -H 'Content-Type: application/json' \
      -d '{
        "server_ws": "wss://orion.example.com/ws",
        "scorpio_base_url": "https://git.example.com",
        "scorpio_lfs_url": "https://git.example.com"
      }'
```

Mega UI 通过 mono 代理启动 runner 时不传 env URL；mono 从基础域名 `runner_connect_domain` 拼接 `git.` / `orion.` 子域名，无需传 `image_*`（由 scheduler `default_image` 填充）。

---

## 自定义镜像

虚拟机启动时间和部署延迟主要取决于镜像是否预装了 Rust / buck2 / apt 包。`scripts/build-custom-image.sh` 通过纯 chroot 方式将工具链直接写入镜像，**无需启动虚拟机**，整个构建过程约需 5 分钟。

### 一键构建

```bash
sudo modprobe nbd max_part=8
sudo bash scripts/build-custom-image.sh
```

> 镜像发布到**调用者**的 qlean 目录（`sudo` 从 `orion` 运行时即 `/home/orion/.local/share/qlean/images/`，不是 `/root/...`）。`orion-scheduler` 与 `cargo run` 也读该路径。可用 `OUTPUT_DIR=...` 覆盖。

> 基础镜像下载源默认指向 USTC 镜像（`https://mirrors.ustc.edu.cn/debian-cdimage/cloud/trixie/latest`）。如需更换为官方源或其他镜像，通过 `BASE_MIRROR_URL` 覆盖（注意 `sudo` 需加 `-E` 传递环境变量）：
>
> ```bash
> BASE_MIRROR_URL=https://cloud.debian.org/images/cloud/trixie/latest \
>   sudo -E bash scripts/build-custom-image.sh
> ```

脚本自动完成以下步骤：

1. 若 `debian-13-generic-amd64.qcow2` 基础镜像缺失，自动下载并校验 SHA512（默认源为 USTC 镜像，可通过 `BASE_MIRROR_URL` 环境变量更换，详见下方说明）
2. 复制基础镜像并扩容至 15GB
3. 通过 `qemu-nbd` 挂载、`growpart` + `resize2fs` 扩展分区；若基础镜像目录缺少 `vmlinuz-*` / `initrd.img-*`，自动从镜像内 `/boot` 提取
4. chroot 进入镜像并安装：
   - Rust 1.95.0 toolchain（在 host 上预下载 tarball，避免 chroot 内 DNS 问题）
   - apt 包：`clang lld pkg-config protobuf-compiler zstd fuse curl git seccomp libseccomp-dev libpython3-dev openssl libssl-dev build-essential`
   - buck2（`2026-04-15` 版本）
   - SSH 公钥写入 `/root/.ssh/authorized_keys`
   - 软链 `rustc` / `cargo` → `/usr/local/bin/`，确保默认 PATH 可找到
5. chroot 末尾执行 `dd` 写满空闲块再删除，利于压缩
6. `qemu-img convert -O qcow2 -c` 压缩（**15GB → ~1.2GB**，节省 90%+）
7. 发布至 qlean 期望的扁平路径：`~/.local/share/qlean/images/debian-13-buck2.qcow2`，并更新 `debian-13-buck2.json` 中的 sha256 digest

### 安全特性

| 特性     | 实现方式                                                     |
| ------ | -------------------------------------------------------- |
| 失败自动清理 | `trap cleanup EXIT INT TERM HUP` 卸载 bind mounts + 断开 NBD |
| 严格错误捕获 | `set -eo pipefail`，chroot 内任何步骤失败立即中止                    |
| 锁检测    | 发布前用 `qemu-img info` 探测目标文件，若被运行中 VM 占用则跳过覆盖             |
| 设备同步   | 使用 `udevadm settle` + 轮询代替固定 `sleep`                     |

### 预装内容速查

| 组件                            | 版本                           | 位置                                        |
| ----------------------------- | ---------------------------- | ----------------------------------------- |
| Rust                          | 1.95.0                       | `/root/.cargo/bin/`，软链至 `/usr/local/bin/` |
| buck2                         | 2026-04-15                   | `/usr/local/bin/buck2`                    |
| clang / lld                   | 19.x                         | apt 系统路径                                  |
| git / protoc / zstd / openssl | apt 当前版本                     | 系统路径                                      |
| SSH key                       | `~/.ssh/orion_vm_access.pub` | `/root/.ssh/authorized_keys`              |

---

## 配置说明

完整配置 schema 参见 `[DESIGN.md](DESIGN.md)` 第 5 节。

### 顶层字段

| 字段                    | 类型     | 默认                         | 说明                                  |
| --------------------- | ------ | -------------------------- | ----------------------------------- |
| `log_dir`             | string | `/var/log/orion-scheduler` | Orion 启动期日志落盘目录                     |
| `orion_source_dir`    | string | 无默认值（必填）                   | Orion 源码目录（含 runner-config、systemd） |
| `orion_binary_path`   | string | 无默认值（必填）                   | Orion 二进制文件路径                       |
| `ssh_public_key_path` | string | 无默认值（必填）                   | SSH 公钥路径                            |
| `max_vms`             | u32    | 无（不限制）                     | 同时跟踪的 domain/VM 上限；超出新 domain 返回 503 |
| `default_image`       | object | 见模板                          | 默认 VM 镜像参数；webhook 未传 `image_*` 时使用 |

### `default_image`

| 字段                 | 类型     | 说明                                               |
| ------------------ | ------ | ------------------------------------------------ |
| `image_path`       | string | 本地 qcow2 镜像路径                                    |
| `image_digest`     | string | 镜像 SHA256 校验和                                    |
| `image_disk_gb`    | u32    | 磁盘大小（GB）                                         |
| `image_cpus`       | u32    | vCPU 数量                                          |
| `image_memory_mb`  | u32    | 内存（MB）                                           |

环境 URL（`server_ws`、`scorpio_base_url`、`scorpio_lfs_url`）由 webhook 请求体传入，不再通过 `targets` 查表。

---

## 目录结构

```text
orion-scheduler/
├── src/
│   ├── main.rs                # axum 入口 + 信号处理
│   ├── handlers.rs            # HTTP 端点 + 日志格式化
│   ├── state.rs               # AppState：按 domain 的 HashMap + KeepAliveMachine
│   ├── config.rs              # 读取/解析 target_config.json（含 max_vms）
│   ├── keep_alive.rs          # qlean::Machine 持久化包装
│   ├── orion_deployer.rs      # handle_update 编排（按 domain 隔离）
│   ├── vm_manager.rs          # SFTP 上传、sed 环境变量替换、systemctl 启停
│   └── vm_cleanup.rs          # 按 runs/qemu.pid 回收（无全局 pkill）
├── scripts/
│   └── build-custom-image.sh  # chroot 离线预装 + qcow2 压缩 + 发布
├── .github/workflows/
│   └── build-custom-image.yml # 镜像 CI（手动触发，上传 S3）
├── target_config.json.template # 配置模板
├── README.md                  # 本文档
├── DESIGN.md                  # 详细设计、生命周期、配置 schema
├── TESTING.md                 # 调试方法、API 测试、常见问题
└── ARTIFACT.md                # 产物分发演进方案（Action + S3 pull）
```

---

## 信号与环境变量

| 信号 / 动作                    | VM  | 服务进程     | 说明                     |
| -------------------------- | --- | -------- | ---------------------- |
| `Ctrl+C` / SIGINT          | **全部**关闭  | 退出       | 关停所有跟踪中的 domain VM，再退出 |
| SIGTERM / SIGQUIT          | **全部**关闭  | 退出       | 同上                     |
| `POST /shutdown?domain=`   | **一台**关闭  | **保持运行** | 必须带 `domain` 或 `vm_id` |
| `POST /shutdown/all`       | **全部**关闭  | **保持运行** | 运维用                      |
| `pkill -9 orion-scheduler` | 可能残留  | 强制终止     | 不优雅关闭；孤儿 qemu 需手动清理或下次启动 `reap_qemu_from_runs` |

| 环境变量          | 默认                   | 说明                      |
| ------------- | -------------------- | ----------------------- |
| `CONFIG_PATH` | 自动探测 / `target_config.json` | 配置文件路径；见 `config::default_config_path` |
| `LISTEN_ADDR` | `0.0.0.0:8080`       | HTTP 监听地址（同机多进程时可覆盖）     |
| `RUST_LOG`    | `info`               | tracing 日志级别，常用 `debug` |
| `XDG_DATA_HOME` | `~/.local/share`   | qlean `runs/` / `images/` 根目录；多进程隔离时可分目录 |

---

## 调试与 SSH 进入 VM

部署完成后从 `GET /status` 或 `GET /vms/{id}` 获取 `vm_ip`，使用构建脚本预装的 SSH 密钥登录：

```bash
VM_IP=$(curl -s 'http://localhost:8080/status?domain=orion.gitmega.com' | jq -r .vm_ip)
# 或：curl -s http://localhost:8080/status | jq -r '.vms[0].vm_ip'
ssh -i ~/.ssh/orion_vm_access root@$VM_IP
```

完整调试流程参见 `[TESTING.md](TESTING.md)`。

---

## Release 与部署

orion-scheduler 是宿主机上的 KVM/QEMU 管理 daemon（依赖 `/dev/kvm`、`qlbr0` 网桥、QEMU 子进程长期存活），不适合做成容器镜像运行。发布方式是「**Linux 原生二进制 + systemd**」，由 GitHub Action 构建，作为 **GitHub Release** 资产发布。

### 触发一次 release

向仓库推一个 `orion-scheduler-vX.Y.Z` 形式的 tag：

```bash
git tag orion-scheduler-v0.1.0
git push origin orion-scheduler-v0.1.0
```

`[.github/workflows/orion-scheduler-release.yml](../.github/workflows/orion-scheduler-release.yml)` 会：

1. 在 `ubuntu-latest` 上 `cargo build --release -p orion-scheduler --target x86_64-unknown-linux-gnu`
2. 打包 `orion-scheduler-vX.Y.Z-linux-amd64.tar.gz`（含二进制、systemd unit、`install.sh`、`target_config.json.template`、`VERSION` 元数据）
3. 生成 `*.tar.gz.sha256`
4. 创建 GitHub Release 并上传 tarball + 校验文件

也可以在 Actions 页面手动 `workflow_dispatch` 触发（需要填 `version`），手动触发的产物只放在 Actions artifacts 里，不会发布为 Release。

Release bundle 内部结构：

```text
orion-scheduler-vX.Y.Z-linux-amd64/
├── bin/orion-scheduler
├── etc/target_config.json.template
├── systemd/orion-scheduler.service
├── install.sh
└── VERSION
```

### Host 端安装

主机预先满足前置要求（KVM 内核模块、`qlbr0` 网桥、QEMU、自定义镜像，见 [前置要求](#前置要求)），然后：

```bash
VERSION=v0.1.0
BUNDLE=orion-scheduler-${VERSION}-linux-amd64
REPO=gitmono-dev/mega

curl -LO https://github.com/${REPO}/releases/download/orion-scheduler-${VERSION}/${BUNDLE}.tar.gz
curl -LO https://github.com/${REPO}/releases/download/orion-scheduler-${VERSION}/${BUNDLE}.tar.gz.sha256
sha256sum -c ${BUNDLE}.tar.gz.sha256
tar -xzf ${BUNDLE}.tar.gz

sudo bash ${BUNDLE}/install.sh
sudo $EDITOR /etc/orion-scheduler/target_config.json   # 首次安装会留下模板
sudo systemctl start orion-scheduler
sudo journalctl -u orion-scheduler -f
```

`[install.sh](install.sh)` 完成的工作：

| 工作            | 路径 / 命令                                                                        |
| ------------- | ------------------------------------------------------------------------------ |
| 创建 service 用户 | `orion` 用户，`-G kvm` 加入 KVM 组                                                   |
| 落二进制          | `/opt/orion-scheduler/bin/orion-scheduler`                                     |
| 落配置           | `/etc/orion-scheduler/target_config.json`（已存在则保留）                              |
| qlean 状态目录    | `/var/lib/orion-scheduler/qlean/{images,runs}`，软链到 `~orion/.local/share/qlean` |
| 日志 & 缓存       | `/var/log/orion-scheduler`、`/var/cache/orion-scheduler`                        |
| systemd       | `/etc/systemd/system/orion-scheduler.service`，`daemon-reload` + `enable`       |

可用环境变量覆盖默认值：`PREFIX` / `ETC_DIR` / `STATE_DIR` / `LOG_DIR` / `CACHE_DIR` / `SERVICE_USER` / `SERVICE_GROUP` / `SKIP_ENABLE=1`。

升级时，下载新 tarball 再跑一次 `install.sh` 即可——配置不会被覆盖，systemd unit 会重启。

### 升级 / 回滚

| 操作       | 命令                                                                                                    |
| -------- | ----------------------------------------------------------------------------------------------------- |
| 升级       | 下载新版本 tarball → `sudo bash <bundle>/install.sh` → `sudo systemctl restart orion-scheduler`            |
| 回滚       | 下载旧版本 tarball → 同上                                                                                    |
| 检查当前版本   | `cat /opt/orion-scheduler/bin/orion-scheduler --version`（若启用了 clap version）或 `cat ~/.../VERSION`（解压后） |
| 停服并保留 VM | `sudo systemctl stop orion-scheduler`；VM 不会被关闭                                                        |
| 完全停服     | 先 `POST /shutdown/all`（或逐台 `?domain=`）关闭 VM，再 `systemctl stop`                                                           |

> **注意**：必须从 Linux x86_64 host 上跑。orion-scheduler 通过 `qlean` 依赖 `kvm-ioctls`，不可在 macOS / ARM64 上运行。

---

## 演进方向

当前 `orion` 二进制路径通过 `target_config.json` 的 `orion_binary_path` 配置，镜像路径通过 `image_path` 或 `image_url`（API 参数）配置。

这要求 orion-scheduler 必须和 mega 源码、镜像构建产物在**同一台机器**上。后续演进方向为：

- mega 仓库 GitHub Action 构建 release 版二进制 → 推送至 S3（带 sha 版本化 + `latest.json` manifest）
- 镜像构建 Action 上传至 S3 + manifest，host 端复用 qlean 内置的 HTTPS 拉取 + sha 校验
- orion-scheduler 每次 webhook 按 manifest 拉取最新版本，本地 cache 命中则跳过下载

详细设计参见 `[ARTIFACT.md](ARTIFACT.md)`。
