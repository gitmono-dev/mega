# 测试方法

本地调试、API 测试、服务管理和常见问题排查。与当前实现一致：**webhook 必须内联 `server_ws` / scorpio URL**；多 VM 按 domain 唯一。

## 前提条件

```bash
# SSH 密钥（路径写入 target_config.json 的 ssh_public_key_path）
ssh-keygen -t ed25519 -f ~/.ssh/orion_vm_access -N "" -C "orion-scheduler"

# 配置文件
cp orion-scheduler/target_config.json.template orion-scheduler/target_config.json
# 编辑 target_config.json，填入本机路径；可选 max_vms
```

自定义 VM 镜像的构建与上传见 [§4 构建镜像并上传到 S3](#4-构建镜像并上传到-s3)。

---

## 1. 快速开始

```bash
# 构建并启动 scheduler（需 KVM；CONFIG_PATH 指向你的配置）
cargo build -p orion-scheduler
CONFIG_PATH=./orion-scheduler/target_config.json cargo run -p orion-scheduler

# 触发 VM（必填三个 URL；domain = server_ws 的 host）
curl -i -X POST http://localhost:8080/webhook \
  -H "Content-Type: application/json" \
  -d '{
    "server_ws": "wss://orion.gitmega.com/ws",
    "scorpio_base_url": "https://git.gitmega.com",
    "scorpio_lfs_url": "https://git.gitmega.com",
    "image_path": "~/.local/share/qlean/images/debian-13-buck2/debian-13-buck2.qcow2",
    "image_digest": "sha256:753c28888c9d30fe4baef55c1d1dfa9a39431595eca940b7ad85d78d84f3d7a5",
    "image_disk_gb": 50,
    "image_cpus": 8,
    "image_memory_mb": 16000,
    "retain_antares_mounts": true
}'
# 期望：HTTP 202，body 含 vm_id、domain、status=provisioning

# 列表 / 单机 / 日志
curl -s http://localhost:8080/status | jq .
curl -s 'http://localhost:8080/status?domain=orion.gitmega.com' | jq .
curl -N 'http://localhost:8080/logs/orion/stream?domain=orion.gitmega.com'
```

`image_path` 支持 `~/...` 或绝对路径。未传任何 `image_*` 时使用配置里的 `default_image`。

### 单元测试（不启 VM）

```bash
cargo test -p orion-scheduler --bins
```

覆盖 domain 解析、两 domain 共存、`max_vms`/`merge` 等逻辑（见 `state` / `handlers` / `orion_deployer` 测试模块）。

---

## 2. API 参考

### 健康检查

```bash
curl http://localhost:8080/health
# {"status": "healthy", "service": "orion-scheduler"}
```

### Webhook

```bash
# GET — 连通性
curl http://localhost:8080/webhook

# POST — 仅用 default_image（仍须三个 URL）
curl -i -X POST http://localhost:8080/webhook \
  -H "Content-Type: application/json" \
  -d '{
    "server_ws": "wss://orion.gitmega.com/ws",
    "scorpio_base_url": "https://git.gitmega.com",
    "scorpio_lfs_url": "https://git.gitmega.com"
  }'

# POST — 第二个 domain（应与第一台并存）
curl -i -X POST http://localhost:8080/webhook \
  -H "Content-Type: application/json" \
  -d '{
    "server_ws": "wss://orion.xuanwu.openatom.cn/ws",
    "scorpio_base_url": "https://git.xuanwu.openatom.cn",
    "scorpio_lfs_url": "https://git.xuanwu.openatom.cn"
  }'

# POST — 同 domain 再启（Running → 幂等 200；Provisioning → 409）
curl -i -X POST http://localhost:8080/webhook \
  -H "Content-Type: application/json" \
  -d '{
    "server_ws": "wss://orion.gitmega.com/ws",
    "scorpio_base_url": "https://git.gitmega.com",
    "scorpio_lfs_url": "https://git.gitmega.com"
  }'

# POST — 强制重建同 domain
curl -i -X POST http://localhost:8080/webhook \
  -H "Content-Type: application/json" \
  -d '{
    "server_ws": "wss://orion.gitmega.com/ws",
    "scorpio_base_url": "https://git.gitmega.com",
    "scorpio_lfs_url": "https://git.gitmega.com",
    "replace": true
  }'

# POST — 同步阻塞至完成（GHA 可用）
curl -i -X POST http://localhost:8080/webhook \
  -H "Content-Type: application/json" \
  -d '{
    "server_ws": "wss://orion.gitmega.com/ws",
    "scorpio_base_url": "https://git.gitmega.com",
    "scorpio_lfs_url": "https://git.gitmega.com",
    "sync": true
  }'
```

> 旧版仅 `{"target":"..."}` 查 `targets` 表已**不再支持**。

### VM 状态

```bash
# 全部
curl -s http://localhost:8080/status | jq .
# {"status":"ok","count":2,"vms":[{phase,vm_id,domain,vm_ip,...},...]}

# 按 domain / vm_id
curl -s 'http://localhost:8080/status?domain=orion.gitmega.com' | jq .
curl -s http://localhost:8080/vms/<vm_id> | jq .
```

异步部署时轮询直至 `phase` 为 `running` 或 `failed`：

```bash
VM_ID=...   # 从 202 响应取出
while true; do
  curl -s "http://localhost:8080/vms/$VM_ID" | jq -c '{phase,vm_ip,error}'
  sleep 5
done
```

### SSH 进入 VM

```bash
VM_IP=$(curl -s 'http://localhost:8080/status?domain=orion.gitmega.com' | jq -r .vm_ip)
ssh -i ~/.ssh/orion_vm_access root@$VM_IP
```

部署时 scheduler 会在 guest 内创建 **8 GB** swap 文件（`/swapfile`，写入 `/etc/fstab`）。可用 `swapon --show` / `free -h` 确认。

### 日志

| 端点 | 格式 | 说明 |
|------|------|------|
| `GET /logs/orion/stream?domain=` 或 `?vm_id=` | SSE | 多 VM 时**建议**带选择器；`curl -N` |

```bash
curl -N 'http://localhost:8080/logs/orion/stream?domain=orion.gitmega.com'
```

服务端调试：`RUST_LOG=debug cargo run -p orion-scheduler`；systemd：`journalctl -u orion-scheduler -f`。

### Terminal (WebSocket PTY)

`GET /vms/{id}/terminal` — `{id}` 为 **vm_id 或 domain**（与 logs 相同）。协议：

| 方向 | 帧 | 内容 |
|------|-----|------|
| client → server | Binary | stdin |
| client → server | Text JSON | `{"type":"resize","cols":N,"rows":N}` |
| server → client | Binary | PTY stdout |

直连 scheduler smoke（VM 须为 Running）：

```bash
# 交互式：键入会作为 stdin 发送；二进制回显到终端
websocat -b "ws://127.0.0.1:8080/vms/orion.gitmega.com/terminal"

# 或经 mono（需 admin session cookie）：
# ws(s)://{mono}/api/v1/orion/runners/{id}/terminal
```

### Scorpio

```bash
curl -s 'http://localhost:8080/scorpio/status?domain=orion.gitmega.com' | jq .
curl -s 'http://localhost:8080/scorpio/config?domain=orion.gitmega.com' | jq .
```

### 关闭

```bash
# 关一台（必须带参数）
curl -X POST 'http://localhost:8080/shutdown?domain=orion.gitmega.com'
# 或
curl -X POST 'http://localhost:8080/shutdown?vm_id=orion-vm-xxx'

# 关全部跟踪 VM（scheduler 继续跑）
curl -X POST http://localhost:8080/shutdown/all
```

无 `domain`/`vm_id` 的 `POST /shutdown` → **400**。

---

## 3. 服务管理

### 停止与检查

```bash
# 优雅：先关 VM，再停 scheduler
curl -X POST http://localhost:8080/shutdown/all
kill -TERM <orion-scheduler-pid>

# 强制（可能残留 qemu）
pkill -9 -f orion-scheduler
# 勿再全局 pkill 所有 qemu-system-x86（会误伤其它 domain / 其它用户）
# 下次启动会 reap 本用户 XDG_DATA_HOME 下 runs/*/qemu.pid

ps aux | grep -E "orion-scheduler|qemu-system" | grep -v grep
fuser 8080/tcp 2>/dev/null || echo "Port 8080 is free"
```

### 信号与关闭方式

| 操作 | VM | scheduler | 说明 |
|------|-----|-----------|------|
| `Ctrl+C` / SIGTERM / SIGQUIT | **全部**停止 | 停止 | `take_all_machines` + run-dir reap |
| `POST /shutdown?domain=` | **一台**停止 | **继续** | |
| `POST /shutdown/all` | **全部**停止 | **继续** | |
| `pkill -9 -f orion-scheduler` | 可能残留 | 停止 | 不优雅 |

`LISTEN_ADDR` 可改端口；`XDG_DATA_HOME` 可隔离 qlean 数据目录。

---

## 4. 构建镜像并上传到 RustFS

多环境 fan-out（推荐；用 bootstrap secret 自动建 bot + 换票）：

```bash
sudo modprobe nbd max_part=8

export ORION_IMAGE_FANOUT='[
  {
    "name": "mega-dev",
    "register_url": "https://git.example-dev/api/v1/orion/images",
    "bootstrap_secret": "'"$MEGA_INIT_BOOTSTRAP_SECRET"'",
    "rustfs_endpoint": "https://rustfs.example-dev",
    "rustfs_access_key": "...",
    "rustfs_secret_key": "...",
    "rustfs_bucket": "...",
    "rustfs_region": "us-east-1"
  }
]'
# 或: export ORION_IMAGE_FANOUT=./orion-image-fanout.json
# 亦可省略各目标 bootstrap_secret，统一 export MEGA_INIT_BOOTSTRAP_SECRET=...

sudo -E bash ~/mega/orion-scheduler/scripts/build-custom-image.sh
```

脚本对每个目标：`POST …/bots/bootstrap-orion-image`（`X-Mega-Init-Secret`）→ 用返回的 `bot_` token 调 `POST …/orion/images`。也可在目标里设静态 `token` 跳过 bootstrap。

单环境兼容（优先 `scripts/.env`，已 export 的变量优先；`sudo -E` 可覆盖）：

```bash
cp ~/mega/orion-scheduler/scripts/.env.example ~/mega/orion-scheduler/scripts/.env
# 填入 RUSTFS_ACCESS_KEY / RUSTFS_SECRET_KEY，以及 MEGA_INIT_BOOTSTRAP_SECRET
# 或 ORION_IMAGE_REGISTER_TOKEN

sudo bash ~/mega/orion-scheduler/scripts/build-custom-image.sh
# 本地仍发布到 ~/.local/share/qlean/images/
# 若 .env 密钥齐全：上传 orion-images/{sha256}/… 并 POST 注册 catalog
```

也可继续用环境变量（会写回 `scripts/.env`）：

```bash
export RUSTFS_ENDPOINT=https://rustfs.example.com
export RUSTFS_ACCESS_KEY=...
export RUSTFS_SECRET_KEY=...
export RUSTFS_BUCKET=mega
export ORION_IMAGE_REGISTER_URL=https://git.example.com/api/v1/orion/images
export MEGA_INIT_BOOTSTRAP_SECRET=...   # 推荐：自动 bootstrap
# 或: export ORION_IMAGE_REGISTER_TOKEN=bot_...

sudo -E bash ~/mega/orion-scheduler/scripts/build-custom-image.sh
```

构建缓存：Rust tarball 在 `/var/cache/orion-image/rust/`（按版本分文件）；compact 后的 qcow2 在 `/var/cache/orion-image/built/<recipe>/`。切回以前的 `RUST_VERSION` 会复用对应镜像，不必重新 chroot。强制重建：`FORCE_REBUILD=1 sudo bash scripts/build-custom-image.sh`。

对象布局：

```text
orion-images/{sha256_hex}/debian-13-buck2.qcow2
orion-images/{sha256_hex}/image-info.json
```

UI（Campsite POC）通过 `GET /api/v1/orion/images` 列出工具链版本；Start Runner 传 `image_id`，mono 签发预签名 URL 给 scheduler。

若 mono 对内用集群内 RustFS（`*.svc.cluster.local`），而 orion-scheduler 跑在集群外，须在 mono 的 `[object_storage.s3]` 配置公网签发地址，例如：

```toml
endpoint_url = "http://rustfs.mega-dev.svc.cluster.local:9000"
presign_endpoint_url = "https://rustfs.xuanwu.openatom.cn"
```

`presign_endpoint_url` 只影响预签名 URL 的 Host（签名包含 Host，不能事后改写）；对内 PUT/GET 仍走 `endpoint_url`。留空则签发仍用 `endpoint_url`。

未设 `ORION_IMAGE_FANOUT` / RustFS / register env 时脚本只做本地发布（与以前相同）。

### 本地无法构建时的 mock 上传

跳过 qemu/chroot，写 1MiB 假文件后直接走 Stage 8（无需 root）：

```bash
export MOCK_UPLOAD=1
export MOCK_IMAGE_BYTES=1048576   # 可选，默认 1MiB
export OUTPUT_DIR=/tmp/orion-mock-images
export RUSTFS_ENDPOINT=http://127.0.0.1:19000
export RUSTFS_ACCESS_KEY=rustfsadmin
export RUSTFS_SECRET_KEY=rustfsadmin
export RUSTFS_BUCKET=mega
export ORION_IMAGE_REGISTER_URL=http://127.0.0.1:8000/api/v1/orion/images
export MEGA_INIT_BOOTSTRAP_SECRET='...'   # 须与 mono 进程环境变量一致，且 ≥32 字符
# mono 还需要 MEGA_BOT_TOKEN_HMAC_SECRET（≥32）才能签发 bot_ token

bash orion-scheduler/scripts/build-custom-image.sh
```

`SKIP_BUILD=1` 与 `MOCK_UPLOAD=1` 等价。

---

## 5. 常见问题排查

| 问题 | 排查 |
|------|------|
| webhook 400 / missing URL | 必须带 `server_ws`、`scorpio_base_url`、`scorpio_lfs_url` |
| 409 conflict | 同 domain 正在 provisioning；等完成或查 `/status?domain=` |
| 幂等 200 | 同 domain 已 Running；要重建加 `"replace": true` |
| 503 max_vms | 提高配置 `max_vms` 或先 `shutdown` 腾出 slot |
| KVM 权限错误 | `/dev/kvm`；用户是否在 `kvm` 组 |
| QEMU 桥接失败 | `/etc/qemu/bridge.conf` 是否 `allow qlbr0` |
| VM 启动超时 | cloud-init、SSH 是否可达 |
| Orion 启动失败 | `curl -N '.../logs/orion/stream?domain=...'` |
| Scorpio 挂载问题 | `curl '.../scorpio/status?domain=...'`（看 `disk.df_root` / `disk.du`） |
| Guest 磁盘打满 / worker Lost | VM 内 `df -h /`；清 `/data/scorpio/antares/{upper,cl}` 或 `systemctl restart orion-runner`；新盘建议 `image_disk_gb: 50` |
| 重启后状态丢了 | 内存 map；磁盘 qemu 靠启动 reap；重新 POST webhook |
| 镜像 catalog 为空 | 构建时设 `ORION_IMAGE_FANOUT` 或 RustFS + `ORION_IMAGE_REGISTER_URL` + `MEGA_INIT_BOOTSTRAP_SECRET`（或静态 token）；查 mono `GET /api/v1/orion/images`；bootstrap 失败查 secret 是否与 mono 一致 |
| Start Runner 选镜像失败 | mono 对象存储需支持预签名（RustFS/S3）；本地 backend 无 signed URL；集群外 scheduler 若拉不动 `*.svc.cluster.local`，给 mono 配 `presign_endpoint_url` 公网 RustFS |
| 进 VM 调试 | [SSH 进入 VM](#ssh-进入-vm) |
