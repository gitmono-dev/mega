# Demo Environment Image Builder

A bash script for building Docker images for the demo environment on your local machine. The script automatically detects your machine's platform (arm64/amd64) and supports both local-only builds and pushing to AWS ECR.

## Overview

This script builds Docker images for the following components:
- **mono-engine** - Mono engine service
- **orion-server** - Orion server component
- **mega-ui** - Mega UI web application

Images are built using Docker Buildx and can be tagged with architecture-specific suffixes (e.g., `-arm64`, `-amd64`).

## Prerequisites

### Required
- **Docker Desktop** with Buildx enabled
- **QEMU** (usually auto-installed by Docker Desktop)

### Optional (for pushing to ECR)
- **AWS CLI** configured with credentials
- Access to **Amazon ECR Public**

## Installation

No installation required. Simply ensure you have Docker Desktop installed and running.

### Verify Docker Buildx

Check if Buildx is available:
```bash
docker buildx version
```

If Buildx is not available, enable it in Docker Desktop settings.

## Usage

### Basic Usage

Build all images locally (default behavior):
```bash
./scripts/demo/build-demo-images-local.sh
```

Build a specific image:
```bash
./scripts/demo/build-demo-images-local.sh mono-engine
```

### Pushing to AWS ECR

Build and push all images to AWS ECR:
```bash
./scripts/demo/build-demo-images-local.sh --push
```

Build and push a specific image:
```bash
./scripts/demo/build-demo-images-local.sh mono-engine --push
```

### Platform Detection

The script automatically detects your machine architecture:
- **ARM64/AArch64** machines → builds `linux/arm64` images
- **x86_64/AMD64** machines → builds `linux/amd64` images

To override the auto-detected platform, set the `TARGET_PLATFORMS` environment variable:
```bash
TARGET_PLATFORMS=linux/amd64 ./scripts/demo/build-demo-images-local.sh
```

**Note:** This script only supports single-platform builds. Multiple platforms are not supported.

## Windows Usage

For Windows users, a batch script `build-demo-images-local.bat` is provided with the same functionality as the Bash script.

### Basic Usage

Build all images locally (default behavior):
```cmd
.\scripts\demo\build-demo-images-local.bat
```

Build a specific image:
```cmd
.\scripts\demo\build-demo-images-local.bat mono-engine
```

### Pushing to AWS ECR

Build and push all images to AWS ECR:
```cmd
.\scripts\demo\build-demo-images-local.bat --push
```

Build and push a specific image:
```cmd
.\scripts\demo\build-demo-images-local.bat mono-engine --push
```

### Platform Detection

The script automatically detects your machine architecture (AMD64 or ARM64) based on `%PROCESSOR_ARCHITECTURE%`.
Note that since Windows builds use Docker Desktop to run Linux containers, the default `TARGET_PLATFORMS` is `linux/amd64` (for standard x64 machines) or `linux/arm64`.

To override, set the `TARGET_PLATFORMS` environment variable:

```cmd
set TARGET_PLATFORMS=linux/amd64
.\scripts\demo\build-demo-images-local.bat
```

## Available Images

| Image Name | Dockerfile Path | Build Context | Tag |
|------------|----------------|---------------|-----|
| `mega/mono-engine` | `mono/Dockerfile` | `.` (repo root) | `latest` |
| `mega/orion-server` | `orion-server/Dockerfile` | `.` (repo root) | `latest` |
| `mega/mega-ui` | `moon/apps/web/Dockerfile` | `moon` | `latest` |
| `mega/mega-init` | `scripts/init_mega/Dockerfile` | `.` (repo root) | `latest` |

## Image Tags

Images are tagged with architecture suffixes:
- `{image-tag}-arm64` for ARM64 builds
- `{image-tag}-amd64` for AMD64 builds

Full image names follow this pattern:
```
public.ecr.aws/m8q5m4u3/mega/project/{image-tag}-{arch}
```

Example:
```
ppublic.ecr.aws/m8q5m4u3/mega/mono-engine:latest-arm64
```

## Configuration

### Registry Configuration

The script uses the following default registry settings:
- **Registry**: `public.ecr.aws`
- **Registry Alias**: `m8q5m4u3`
- **Repository**: `mega`

These are hardcoded in the script and can be modified if needed.

### Build Order

Images are built in the following order:
1. mono-engine
2. orion-server
3. mega-ui

This order ensures dependencies are built first.

## Features

- ✅ **Automatic platform detection** - Detects your machine architecture
- ✅ **Single or batch builds** - Build one image or all images
- ✅ **Local or remote builds** - Build locally or push to ECR
- ✅ **Build caching** - Uses registry cache when pushing
- ✅ **Error handling** - Continues building other images if one fails
- ✅ **Progress output** - Plain text build progress for better visibility

## AWS ECR Setup

If you want to push images to AWS ECR:

1. **Install AWS CLI**:
   ```bash
   # macOS
   brew install awscli
   
   # Linux
   sudo apt-get install awscli
   ```

2. **Configure AWS credentials**:
   ```bash
   aws configure
   ```

3. **Verify credentials**:
   ```bash
   aws sts get-caller-identity
   ```

4. **Login to ECR** (handled automatically by the script):
   The script will automatically log in to ECR Public when using `--push`.

## Troubleshooting

### Docker daemon not running
```
Error: Docker daemon is not running. Please start Docker Desktop.
```
**Solution**: Start Docker Desktop and wait for it to fully initialize.

### Buildx not available
```
Error: Docker Buildx is not available. Please enable it in Docker Desktop.
```
**Solution**: Enable Buildx in Docker Desktop settings (usually enabled by default).

### AWS credentials not configured
```
Error: AWS credentials are not configured. Please run: aws configure
```
**Solution**: Configure AWS credentials using `aws configure` or set environment variables.

### Repository root not found
```
Error: Could not determine repository root.
```
**Solution**: Run the script from the repository root, or ensure `Cargo.toml` or `moon/package.json` exists in the repo root.

### Dockerfile not found
```
Error: Dockerfile not found: /path/to/dockerfile
```
**Solution**: Verify that the Dockerfile paths in the script match your repository structure.

### Multiple platforms error
```
Error: Multiple platforms detected in TARGET_PLATFORMS
```
**Solution**: This script only supports single-platform builds. Set `TARGET_PLATFORMS` to a single platform (e.g., `linux/arm64`).

## Build Process

1. **Prerequisites check** - Verifies Docker, Buildx, and optionally AWS CLI
2. **Buildx setup** - Creates or reuses a buildx builder named `mega-builder`
3. **ECR login** - Logs in to AWS ECR (only if `--push` is used)
4. **Image building** - Builds images using Docker Buildx with:
   - Platform-specific tags
   - Build cache (when pushing)
   - Inline cache for faster rebuilds
   - Progress output

## Examples

### Example 1: Build all images locally
```bash
cd /path/to/mega
./scripts/demo/build-demo-images-local.sh
```

### Example 2: Build and push mono-engine
```bash
./scripts/demo/build-demo-images-local.sh mono-engine --push
```

### Example 3: Build for specific platform
```bash
TARGET_PLATFORMS=linux/amd64 ./scripts/demo/build-demo-images-local.sh
```

### Example 4: Build and push all images
```bash
./scripts/demo/build-demo-images-local.sh --push
```

## Output

The script provides color-coded output:
- 🟢 **Green [INFO]** - Informational messages
- 🟡 **Yellow [WARN]** - Warnings
- 🔴 **Red [ERROR]** - Errors

Build progress is shown in plain text format for better visibility in CI/CD environments.

## Notes

- Images built locally (without `--push`) are loaded into your local Docker daemon
- Images pushed to ECR are available at `public.ecr.aws/m8q5m4u3/mega`
- The script uses a buildx builder named `mega-builder` (created automatically if needed)
- Build failures for individual images don't stop the entire process when building all images

## See Also

- [Docker Buildx Documentation](https://docs.docker.com/buildx/)
- [AWS ECR Public Documentation](https://docs.aws.amazon.com/ecr/latest/public/)
- [Docker Desktop Documentation](https://docs.docker.com/desktop/)

