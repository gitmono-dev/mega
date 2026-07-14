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
    "image_disk_gb": 30,
    "image_cpus": 8,
    "image_memory_mb": 16000
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

## 4. 构建镜像并上传到 S3

```bash
sudo modprobe nbd max_part=8
sudo bash ~/mega/orion-scheduler/scripts/build-custom-image.sh
# 输出 sha256:<hex>，用作 webhook 的 image_digest
```

```bash
aws s3 cp ~/.local/share/qlean/images/debian-13-buck2/debian-13-buck2.qcow2 \
  s3://gitmega/images/debian-13-buck2.qcow2 --progress
```

`image_digest` 使用构建脚本输出的本地文件 hash；上传前后内容不变则 hash 一致。

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
| Scorpio 挂载问题 | `curl '.../scorpio/status?domain=...'` |
| 重启后状态丢了 | 内存 map；磁盘 qemu 靠启动 reap；重新 POST webhook |
| 进 VM 调试 | [SSH 进入 VM](#ssh-进入-vm) |
