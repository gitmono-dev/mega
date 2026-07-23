#!/usr/bin/env bash
set -euo pipefail

# ============================================================================
# Local Image Builder for Demo Environment
# ============================================================================
# This script builds Docker images for the demo environment on your local machine.
# The script automatically detects the platform (arm64/amd64) based on your machine.
# By default, images are only built locally. Use --push to push to AWS ECR and Harbor.
#
# Prerequisites:
#   - Docker Desktop with Buildx enabled
#   - (Optional) AWS CLI configured with credentials (only needed for --push)
#   - (Optional) Access to Amazon ECR Public (only needed for --push)
#   - (Optional) docker login registry.xuanwu.openatom.cn (only needed for --push)
#   - QEMU (usually auto-installed by Docker Desktop)
#
# Usage:
#   ./scripts/demo/build-demo-images-local.sh [IMAGE_NAME] [--push] [--prune-buildkit]
#
#   Examples:
#     ./scripts/demo/build-demo-images-local.sh              # Build all images locally
#     ./scripts/demo/build-demo-images-local.sh --push       # Build and push all images
#     ./scripts/demo/build-demo-images-local.sh mono-engine  # Build mono-engine locally
#     ./scripts/demo/build-demo-images-local.sh mono-engine --push  # Build and push mono-engine
#     ./scripts/demo/build-demo-images-local.sh --prune-buildkit     # Build and prune buildkit cache after build
#
#   If IMAGE_NAME is provided, only that image will be built.
#   Otherwise, all images will be built.
#   Use --push to push to ECR Public and Harbor (registry.xuanwu.openatom.cn).
# ============================================================================

# Colors for output
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
NC='\033[0m' # No Color

# Configuration
REGISTRY_ALIAS="m8q5m4u3"
REPOSITORY="mega"
REGISTRY="public.ecr.aws"
HARBOR_REGISTRY="${HARBOR_REGISTRY:-registry.xuanwu.openatom.cn}"
SHOULD_PUSH=false  # Default: only build, don't push
SHOULD_PRUNE_BUILDKIT=false  # Optional: prune buildkit cache after build
BUILDX_BUILDER_ARGS=()
BUILDX_PRUNE_ARGS=()

# Auto-detect platform if not explicitly set
# The script automatically detects your machine architecture and sets the appropriate platform
if [ -z "${TARGET_PLATFORMS:-}" ]; then
    MACHINE_ARCH=$(uname -m)
    case "${MACHINE_ARCH}" in
        arm64|aarch64)
            TARGET_PLATFORMS="linux/arm64"
            ;;
        x86_64|amd64)
            TARGET_PLATFORMS="linux/amd64"
            ;;
        *)
            # Default to arm64 for compatibility (e.g., macOS Apple Silicon)
            TARGET_PLATFORMS="linux/arm64"
            printf "\033[1;33m[WARN]\033[0m Unknown machine architecture: %s, defaulting to %s\n" "${MACHINE_ARCH}" "${TARGET_PLATFORMS}"
            ;;
    esac
    printf "\033[0;32m[INFO]\033[0m Auto-detected platform: %s (machine: %s)\n" "${TARGET_PLATFORMS}" "${MACHINE_ARCH}"
else
    printf "\033[0;32m[INFO]\033[0m Using explicit TARGET_PLATFORMS: %s\n" "${TARGET_PLATFORMS}"
fi

# Get script directory and repo root
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
# Try to find repo root by looking for known files
if [ -f "${SCRIPT_DIR}/../../Cargo.toml" ]; then
    REPO_ROOT="$(cd "${SCRIPT_DIR}/../.." && pwd)"
elif [ -f "${SCRIPT_DIR}/../../moon/package.json" ]; then
    REPO_ROOT="$(cd "${SCRIPT_DIR}/../.." && pwd)"
elif [ -f "${PWD}/Cargo.toml" ] || [ -f "${PWD}/moon/package.json" ]; then
    REPO_ROOT="${PWD}"
else
    REPO_ROOT=""
fi

# Image configurations (ordered for consistent build order)
declare -a IMAGE_ORDER=("mono-engine" "orion-server" "mega-ui")
get_image_config() {
    case "$1" in
        "mono-engine") echo "mono/Dockerfile:." ;;
        "mega-ui") echo "moon/apps/web/Dockerfile:moon" ;;
        "orion-server") echo "orion-server/Dockerfile:." ;;
    esac
}

get_image_tag() {
    case "$1" in
        "mono-engine") echo "latest" ;;
        "mega-ui") echo "latest" ;;
        "orion-server") echo "latest" ;;
    esac
}

is_valid_image() {
    case "$1" in
        "mono-engine"|"mega-ui"|"orion-server") return 0 ;;
        *) return 1 ;;
    esac
}

# Functions
log_info() {
    printf "${GREEN}[INFO]${NC} %s\n" "$1"
}

log_warn() {
    printf "${YELLOW}[WARN]${NC} %s\n" "$1"
}

log_error() {
    printf "${RED}[ERROR]${NC} %s\n" "$1"
}

check_prerequisites() {
    log_info "Checking prerequisites..."
    
    # Check if we found repo root
    if [ -z "${REPO_ROOT}" ] || [ ! -d "${REPO_ROOT}" ]; then
        log_error "Could not determine repository root."
        log_error "Please run this script from the mega repository root, or ensure Cargo.toml or moon/package.json exists."
        exit 1
    fi
    
    # Verify repo root
    if [ ! -f "${REPO_ROOT}/Cargo.toml" ] && [ ! -f "${REPO_ROOT}/moon/package.json" ]; then
        log_error "Repository root validation failed: ${REPO_ROOT}"
        log_error "Expected to find Cargo.toml or moon/package.json in repository root."
        exit 1
    fi
    
    log_info "Repository root: ${REPO_ROOT}"
    
    # Check Docker
    if ! command -v docker &> /dev/null; then
        log_error "Docker is not installed. Please install Docker Desktop."
        exit 1
    fi
    
    # Check if Docker is running
    if ! docker info &> /dev/null; then
        log_error "Docker daemon is not running. Please start Docker Desktop."
        exit 1
    fi
    
    # Check Docker Buildx
    if ! docker buildx version &> /dev/null; then
        log_error "Docker Buildx is not available. Please enable it in Docker Desktop."
        exit 1
    fi
    
    # Only check AWS prerequisites if we need to push
    if [ "$SHOULD_PUSH" = "true" ]; then
        # Check AWS CLI
        if ! command -v aws &> /dev/null; then
            log_error "AWS CLI is not installed. Please install it: brew install awscli"
            exit 1
        fi
        
        # Check AWS credentials
        if ! aws sts get-caller-identity &> /dev/null; then
            log_error "AWS credentials are not configured. Please run: aws configure"
            exit 1
        fi
    fi
    
    log_info "All prerequisites met ✓"
}

setup_buildx() {
    log_info "Setting up Docker Buildx..."
    
    local builder_name="mega-builder"

    if [ "$SHOULD_PUSH" != "true" ]; then
        # Local-only builds prefer a docker-driver builder so the image is loaded
        # directly into the local Docker engine (no extra --load tar export).
        # Try common docker-driver builder names; fall back to the currently
        # active builder if none of them are present.
        local local_builder=""
        local candidate
        for candidate in default desktop-linux orbstack colima; do
            if docker buildx inspect "${candidate}" >/dev/null 2>&1; then
                # Only pick docker-driver builders here; container builders
                # would re-introduce the --load round-trip we are trying to avoid.
                if docker buildx inspect "${candidate}" 2>/dev/null \
                        | grep -E '^Driver:[[:space:]]+docker$' >/dev/null; then
                    local_builder="${candidate}"
                    break
                fi
            fi
        done

        if [ -n "${local_builder}" ]; then
            log_info "Using local docker-driver buildx builder: ${local_builder}"
            BUILDX_BUILDER_ARGS=(--builder "${local_builder}")
            BUILDX_PRUNE_ARGS=(--builder "${local_builder}")
        else
            log_warn "No docker-driver buildx builder found; using currently active builder."
            log_warn "Tip: enable Docker Desktop's default builder, or run 'docker buildx use default'."
            BUILDX_BUILDER_ARGS=()
            BUILDX_PRUNE_ARGS=()
        fi

        log_info "Buildx setup complete ✓"
        return 0
    fi

    # Reuse existing builder if present; otherwise create it.
    # NOTE: `docker buildx ls | grep` is not reliable enough (can false-match), so prefer `inspect`.
    if docker buildx inspect "${builder_name}" >/dev/null 2>&1; then
        log_info "Using existing buildx builder: ${builder_name}"
        docker buildx use "${builder_name}" >/dev/null
    else
        log_info "Creating new buildx builder: ${builder_name}"
        if ! docker buildx create --name "${builder_name}" --driver docker-container --use >/dev/null; then
            log_error "Failed to create buildx builder: ${builder_name}"
            exit 1
        fi
    fi

    BUILDX_BUILDER_ARGS=(--builder "${builder_name}")
    BUILDX_PRUNE_ARGS=(--builder "${builder_name}")

    # Ensure the builder is bootstrapped (idempotent)
    if ! docker buildx inspect "${builder_name}" --bootstrap >/dev/null 2>&1; then
        log_error "Failed to bootstrap buildx builder: ${builder_name}"
        exit 1
    fi
    
    # Verify platform support
    local platforms_output
    platforms_output=$(docker buildx inspect "${builder_name}" --bootstrap 2>/dev/null | grep -i "platforms" || echo "")
    
    # Parse TARGET_PLATFORMS (comma-separated) and check each platform
    IFS=',' read -ra REQUESTED_PLATFORMS <<< "${TARGET_PLATFORMS}"
    local missing_platforms=()
    
    for platform in "${REQUESTED_PLATFORMS[@]}"; do
        # Trim whitespace
        platform=$(echo "$platform" | xargs)
        if [[ ! "$platforms_output" =~ "$platform" ]]; then
            missing_platforms+=("$platform")
        fi
    done
    
    if [ ${#missing_platforms[@]} -gt 0 ]; then
        log_warn "Some requested platforms may not be fully enabled:"
        for missing in "${missing_platforms[@]}"; do
            log_warn "  - ${missing}"
        done
        log_warn "Platforms available: $platforms_output"
        log_warn "Continuing anyway..."
    fi
    
    log_info "Buildx setup complete ✓"
}

prune_buildkit_cache() {
    if [ "${SHOULD_PRUNE_BUILDKIT}" != "true" ]; then
        return 0
    fi

    log_info "Pruning BuildKit cache (filter: unused for 24h)..."
    if ! docker buildx prune "${BUILDX_PRUNE_ARGS[@]}" --force --filter "until=24h" >/dev/null 2>&1; then
        log_warn "BuildKit cache prune failed. You can retry manually: docker buildx prune --force"
        return 0
    fi

    log_info "BuildKit cache prune complete ✓"
}

login_ecr() {
    log_info "Logging in to Amazon ECR Public..."
    
    if ! aws ecr-public get-login-password --region us-east-1 2>/dev/null | \
        docker login --username AWS --password-stdin "${REGISTRY}/${REGISTRY_ALIAS}" 2>/dev/null; then
        log_error "Failed to login to ECR Public. Please check your AWS credentials."
        exit 1
    fi
    
    log_info "ECR login successful ✓"
}

build_and_push() {
    local image_name=$1
    local config=$(get_image_config "$image_name")
    local dockerfile_path=$(echo "$config" | cut -d':' -f1)
    local build_context=$(echo "$config" | cut -d':' -f2)
    local image_tag=$(get_image_tag "$image_name")
    
    # Validate single platform build (local script only supports single platform)
    # Check if TARGET_PLATFORMS contains comma (multiple platforms)
    if [[ "${TARGET_PLATFORMS}" == *","* ]]; then
        log_error "Multiple platforms detected in TARGET_PLATFORMS: ${TARGET_PLATFORMS}"
        log_error "This local script only supports single platform builds."
        log_error "Please set TARGET_PLATFORMS to a single platform (e.g., linux/arm64 or linux/amd64)."
        exit 1
    fi
    
    # Extract architecture suffix from single platform (e.g., linux/arm64 -> arm64)
    # This assumes TARGET_PLATFORMS is a single platform in format "os/arch"
    local arch_suffix=$(echo "${TARGET_PLATFORMS}" | awk -F'/' '{print $NF}')
    
    # Validate that we successfully extracted an architecture suffix
    if [ -z "${arch_suffix}" ]; then
        log_error "Failed to extract architecture suffix from TARGET_PLATFORMS: ${TARGET_PLATFORMS}"
        log_error "Expected format: os/arch (e.g., linux/arm64 or linux/amd64)"
        exit 1
    fi
    local image_tag_with_arch="${image_tag}-${arch_suffix}"
    local image_repo="${REGISTRY}/${REGISTRY_ALIAS}/${REPOSITORY}/${image_name}"
    local harbor_repo="${HARBOR_REGISTRY}/${REPOSITORY}/${image_name}"
    
    # Verify paths exist (use absolute paths)
    local full_dockerfile="${REPO_ROOT}/${dockerfile_path}"
    local full_context="${REPO_ROOT}/${build_context}"
    
    if [ ! -f "${full_dockerfile}" ]; then
        log_error "Dockerfile not found: ${full_dockerfile}"
        return 1
    fi
    
    if [ ! -d "${full_context}" ]; then
        log_error "Build context not found: ${full_context}"
        return 1
    fi
    
    # Additional validation: check if Dockerfile is within or accessible from build context
    # For most cases, Dockerfile should be accessible from the build context
    local dockerfile_in_context="${full_context}/${dockerfile_path}"
    local dockerfile_relative_to_context=""
    
    # Try to find Dockerfile relative to context
    if [ -f "${dockerfile_in_context}" ]; then
        dockerfile_relative_to_context="${dockerfile_path}"
    elif [ -f "${full_dockerfile}" ]; then
        # Dockerfile is outside context, which is fine for buildx
        dockerfile_relative_to_context="${dockerfile_path}"
    else
        log_error "Cannot resolve Dockerfile path relative to build context"
        return 1
    fi
    
    if [ "$SHOULD_PUSH" = "true" ]; then
        log_info "Building and pushing ${image_name} (${image_tag_with_arch})..."
    else
        log_info "Building ${image_name} (${image_tag_with_arch})..."
    fi
    log_info "  Dockerfile: ${dockerfile_path}"
    log_info "  Context: ${build_context}"
    log_info "  Platforms: ${TARGET_PLATFORMS}"
    
    # Change to repo root for build
    cd "${REPO_ROOT}"
    
    # Build cache args - try to use registry cache, but don't fail if it doesn't exist
    # Only check registry cache if we're pushing (requires AWS access)
    local cache_from_args=()
    if [ "$SHOULD_PUSH" = "true" ]; then
        if docker manifest inspect "${image_repo}:${image_tag_with_arch}" &> /dev/null 2>&1; then
            # Verify that existing image is a single-architecture image, not a manifest list
            local manifest_output
            manifest_output=$(docker manifest inspect "${image_repo}:${image_tag_with_arch}" 2>/dev/null || echo "")
            
            if echo "$manifest_output" | grep -q '"manifests"'; then
                log_error "Error: ${image_repo}:${image_tag_with_arch} is already a manifest list!"
                log_error "This script only builds single-architecture images."
                log_error "Please delete the incorrect manifest list from ECR first:"
                log_error "  aws ecr-public batch-delete-image \\"
                log_error "    --repository-name ${REPOSITORY} \\"
                log_error "    --registry-id ${REGISTRY_ALIAS} \\"
                log_error "    --image-ids imageTag=${image_tag_with_arch} \\"
                log_error "    --region us-east-1"
                log_error ""
                log_error "Or continue anyway - the build will overwrite it with a single-architecture image."
                log_warn "Continuing with build (will overwrite manifest list)..."
                # Continue anyway - buildx build will overwrite it
            else
                log_info "  Using registry cache from existing image (single-architecture)"
            fi
            cache_from_args=("--cache-from" "type=registry,ref=${image_repo}:${image_tag_with_arch}")
        else
            log_info "  No existing image found, building from scratch"
        fi
    fi
    
    # Build command arguments
    local build_args=(
        "${BUILDX_BUILDER_ARGS[@]}"
        --platform "${TARGET_PLATFORMS}"
        --file "${dockerfile_path}"
        --tag "${image_repo}:${image_tag_with_arch}"
        --tag "${harbor_repo}:${image_tag_with_arch}"
        --progress=auto
        --build-arg BUILDKIT_INLINE_CACHE=1
    )
    
    if [ "$SHOULD_PUSH" = "true" ]; then
        # Load before docker push so the pushed artifact is a single-architecture image manifest.
        build_args+=(--load)
    fi

    # Add cache arguments
    if [ ${#cache_from_args[@]} -gt 0 ]; then
        build_args+=("${cache_from_args[@]}")
    fi

    if [ "$SHOULD_PUSH" = "true" ]; then
        # Inline cache helps subsequent remote builds without writing a local layer tar cache.
        build_args+=(--cache-to type=inline)
    fi
    
    # Add build context
    build_args+=("${build_context}")
    
    log_info "output build args: ${build_args[*]}"
    
    # Build the image.
    if ! docker buildx build "${build_args[@]}"; then
        log_error "Failed to build ${image_name}"
        return 1
    fi

    # Push if requested (ensures single-arch manifest is uploaded)
    if [ "$SHOULD_PUSH" = "true" ]; then
        if ! docker push "${image_repo}:${image_tag_with_arch}"; then
            log_error "Failed to push ${image_name} to ECR"
            return 1
        fi
        if ! docker push "${harbor_repo}:${image_tag_with_arch}"; then
            log_error "Failed to push ${image_name} to Harbor (${harbor_repo})"
            log_error "Login first: docker login ${HARBOR_REGISTRY}"
            return 1
        fi
        log_info "${image_name} built and pushed successfully ✓"
        log_info "  ECR:    ${image_repo}:${image_tag_with_arch}"
        log_info "  Harbor: ${harbor_repo}:${image_tag_with_arch}"
    else
        log_info "${image_name} built successfully ✓"
        log_info "  Image: ${image_repo}:${image_tag_with_arch} (local only)"
        log_info "  Harbor tag also applied: ${harbor_repo}:${image_tag_with_arch}"
    fi
    return 0
}

build_all() {
    if [ "$SHOULD_PUSH" = "true" ]; then
        log_info "Building and pushing all demo images..."
    else
        log_info "Building all demo images..."
    fi
    
    local failed_images=()
    for image_name in "${IMAGE_ORDER[@]}"; do
        echo ""
        log_info "=========================================="
        log_info "Building: ${image_name}"
        log_info "=========================================="
        if ! build_and_push "${image_name}"; then
            failed_images+=("${image_name}")
            log_error "Failed to build ${image_name}, continuing with next image..."
        fi
    done
    
    echo ""
    if [ ${#failed_images[@]} -eq 0 ]; then
        log_info "=========================================="
        if [ "$SHOULD_PUSH" = "true" ]; then
            log_info "All images built and pushed successfully!"
        else
            log_info "All images built successfully!"
        fi
        log_info "=========================================="
        return 0
    else
        log_error "=========================================="
        log_error "Some images failed to build:"
        for img in "${failed_images[@]}"; do
            log_error "  - ${img}"
        done
        log_error "=========================================="
        return 1
    fi
}

# Main execution
main() {
    # Parse command line arguments
    # Extract --push flag and collect other arguments
    local args=()
    for arg in "$@"; do
        if [ "$arg" = "--push" ]; then
            SHOULD_PUSH=true
        elif [ "$arg" = "--prune-buildkit" ]; then
            SHOULD_PRUNE_BUILDKIT=true
        else
            args+=("$arg")
        fi
    done
    
    # Determine operation mode
    local mode_info=""
    if [ "$SHOULD_PUSH" = "true" ]; then
        mode_info="build and push"
    else
        mode_info="build (local only)"
    fi
    
    log_info "Starting local image ${mode_info}..."
    log_info "Registry: ${REGISTRY}/${REGISTRY_ALIAS}/${REPOSITORY}"
    if [ "$SHOULD_PUSH" = "true" ]; then
        log_info "Mode: Build and push to ECR"
    else
        log_info "Mode: Build only (local)"
    fi
    if [ "$SHOULD_PRUNE_BUILDKIT" = "true" ]; then
        log_info "Extra: BuildKit cache prune enabled"
    fi
    echo ""
    
    check_prerequisites
    setup_buildx
    
    # Only login to ECR if we need to push
    if [ "$SHOULD_PUSH" = "true" ]; then
        login_ecr
    fi
    
    # Build specific image or all images
    if [ ${#args[@]} -eq 1 ]; then
        local image_name="${args[0]}"
        if is_valid_image "$image_name"; then
            if build_and_push "$image_name"; then
                prune_buildkit_cache
                log_info "Done!"
                exit 0
            else
                log_error "Build failed"
                exit 1
            fi
        else
            log_error "Unknown image: $image_name"
            log_info "Available images: ${IMAGE_ORDER[*]}"
            exit 1
        fi
    elif [ ${#args[@]} -eq 0 ]; then
        if build_all; then
            prune_buildkit_cache
            log_info "Done!"
            exit 0
        else
            log_error "Some builds failed"
            exit 1
        fi
    else
        log_error "Too many arguments. Usage: $0 [IMAGE_NAME] [--push]"
        log_info "Available images: ${IMAGE_ORDER[*]}"
        exit 1
    fi
}

# Run main function
main "$@"
