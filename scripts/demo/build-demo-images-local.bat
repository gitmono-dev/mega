@echo off
setlocal EnableDelayedExpansion

rem ============================================================================
rem Local Image Builder for Demo Environment (Windows Batch Version)
rem ============================================================================
rem This script builds Docker images for the demo environment on your local machine.
rem The script automatically detects the platform (arm64/amd64) based on your machine.
rem By default, images are only built locally. Use --push to push to AWS ECR.
rem
rem Usage:
rem   .\scripts\demo\build-demo-images-local.bat [IMAGE_NAME] [--push]
rem
rem   Examples:
rem     .\scripts\demo\build-demo-images-local.bat              # Build all images locally
rem     .\scripts\demo\build-demo-images-local.bat --push       # Build and push all images
rem     .\scripts\demo\build-demo-images-local.bat mono-engine  # Build mono-engine locally
rem ============================================================================

rem Configuration
set "REGISTRY_ALIAS=m8q5m4u3"
set "REPOSITORY=mega"
set "REGISTRY=public.ecr.aws"
set "SHOULD_PUSH=false"

rem Auto-detect platform if not explicitly set
if "%TARGET_PLATFORMS%"=="" (
    if /i "%PROCESSOR_ARCHITECTURE%"=="AMD64" (
        set "TARGET_PLATFORMS=linux/amd64"
        set "ARCH_SUFFIX=amd64"
    ) else if /i "%PROCESSOR_ARCHITECTURE%"=="ARM64" (
        set "TARGET_PLATFORMS=linux/arm64"
        set "ARCH_SUFFIX=arm64"
    ) else (
        rem Default to arm64 for compatibility
        set "TARGET_PLATFORMS=linux/arm64"
        set "ARCH_SUFFIX=arm64"
        echo [WARN] Unknown machine architecture: %PROCESSOR_ARCHITECTURE%, defaulting to !TARGET_PLATFORMS!
    )
    echo [INFO] Auto-detected platform: !TARGET_PLATFORMS! (machine: %PROCESSOR_ARCHITECTURE%)
) else (
    echo [INFO] Using explicit TARGET_PLATFORMS: %TARGET_PLATFORMS%
)

rem Get script directory and repo root
set "SCRIPT_DIR=%~dp0"
rem Remove trailing backslash
if "%SCRIPT_DIR:~-1%"=="\" set "SCRIPT_DIR=%SCRIPT_DIR:~0,-1%"

if exist "%SCRIPT_DIR%\..\..\Cargo.toml" (
    pushd "%SCRIPT_DIR%\..\.."
    set "REPO_ROOT=!CD!"
    popd
) else if exist "%SCRIPT_DIR%\..\..\moon\package.json" (
    pushd "%SCRIPT_DIR%\..\.."
    set "REPO_ROOT=!CD!"
    popd
) else if exist "Cargo.toml" (
    set "REPO_ROOT=%CD%"
) else (
    set "REPO_ROOT="
)

rem Image Definitions
rem Keep build order consistent with the bash script
set "IMAGE_ORDER=mono-engine orion-server mega-ui mega-init"

set "IMAGES_mono-engine_DOCKERFILE=mono/Dockerfile"
set "IMAGES_mono-engine_CONTEXT=."
set "TAGS_mono-engine=latest"

set "IMAGES_mega-ui_DOCKERFILE=moon/apps/web/Dockerfile"
set "IMAGES_mega-ui_CONTEXT=moon"
set "TAGS_mega-ui=latest"

set "IMAGES_orion-server_DOCKERFILE=orion-server/Dockerfile"
set "IMAGES_orion-server_CONTEXT=."
set "TAGS_orion-server=latest"

set "IMAGES_mega-init_DOCKERFILE=scripts/init_mega/Dockerfile"
set "IMAGES_mega-init_CONTEXT=."
set "TAGS_mega-init=latest"

rem Parse Arguments
set "TARGET_IMAGE="
:parse_args
if "%~1"=="" goto :args_done
if "%~1"=="--push" (
    set "SHOULD_PUSH=true"
) else (
    set "TARGET_IMAGE=%~1"
)
shift
goto :parse_args
:args_done

rem Main Logic
call :check_prerequisites
if errorlevel 1 exit /b 1

call :setup_buildx
if errorlevel 1 exit /b 1

if "%SHOULD_PUSH%"=="true" (
    call :login_ecr
    if errorlevel 1 exit /b 1
)

if not "%TARGET_IMAGE%"=="" (
    call :build_and_push "%TARGET_IMAGE%"
    if errorlevel 1 exit /b 1
) else (
    call :build_all
    if errorlevel 1 exit /b 1
)

echo [INFO] Done!
exit /b 0

rem ============================================================================
rem Functions
rem ============================================================================

:check_prerequisites
    echo [INFO] Checking prerequisites...
    if "%REPO_ROOT%"=="" (
        echo [ERROR] Could not determine repository root.
        echo [ERROR] Please run this script from the mega repository root, or ensure Cargo.toml or moon/package.json exists.
        exit /b 1
    )
    echo [INFO] Repository root: %REPO_ROOT%

    where docker >nul 2>nul
    if errorlevel 1 (
        echo [ERROR] Docker is not installed. Please install Docker Desktop.
        exit /b 1
    )

    docker info >nul 2>nul
    if errorlevel 1 (
        echo [ERROR] Docker daemon is not running. Please start Docker Desktop.
        exit /b 1
    )

    docker buildx version >nul 2>nul
    if errorlevel 1 (
        echo [ERROR] Docker Buildx is not available. Please enable it in Docker Desktop.
        exit /b 1
    )

    if "%SHOULD_PUSH%"=="true" (
        where aws >nul 2>nul
        if errorlevel 1 (
            echo [ERROR] AWS CLI is not installed.
            exit /b 1
        )
        call aws sts get-caller-identity >nul 2>nul
        if errorlevel 1 (
            echo [ERROR] AWS credentials are not configured. Please run: aws configure
            exit /b 1
        )
    )
    echo [INFO] All prerequisites met.
    exit /b 0

:setup_buildx
    echo [INFO] Setting up Docker Buildx...
    set "builder_name=mega-builder"
    
    docker buildx inspect "!builder_name!" >nul 2>nul
    if errorlevel 1 (
        echo [INFO] Creating new buildx builder: !builder_name!
        docker buildx create --name "!builder_name!" --driver docker-container --use >nul
        if errorlevel 1 (
            echo [ERROR] Failed to create buildx builder: !builder_name!
            exit /b 1
        )
    ) else (
        echo [INFO] Using existing buildx builder: !builder_name!
        docker buildx use "!builder_name!" >nul
    )
    
    docker buildx inspect "!builder_name!" --bootstrap >nul 2>nul
    if errorlevel 1 (
        echo [ERROR] Failed to bootstrap buildx builder: !builder_name!
        exit /b 1
    )
    
    rem Check for missing platforms (simplified check for Windows)
    docker buildx inspect "!builder_name!" --bootstrap 2>nul | findstr /i "platforms" >nul
    
    echo [INFO] Buildx setup complete.
    exit /b 0

:login_ecr
    echo [INFO] Logging in to Amazon ECR Public...
    
    rem Capture password into variable
    set "LOGIN_PASSWORD="
    for /f "tokens=*" %%i in ('aws ecr-public get-login-password --region us-east-1 2^>nul') do set "LOGIN_PASSWORD=%%i"
    
    if "!LOGIN_PASSWORD!"=="" (
        echo [ERROR] Failed to get ECR login password. Please check your AWS credentials.
        exit /b 1
    )
    
    echo !LOGIN_PASSWORD! | docker login --username AWS --password-stdin "%REGISTRY%/%REGISTRY_ALIAS%" >nul 2>nul
    if errorlevel 1 (
        echo [ERROR] Failed to login to ECR Public.
        exit /b 1
    )
    echo [INFO] ECR login successful.
    exit /b 0

:build_and_push
    set "img_name=%~1"
    
    if not defined IMAGES_!img_name!_DOCKERFILE (
        echo [ERROR] Unknown image: !img_name!
        exit /b 1
    )

    set "dockerfile=!IMAGES_%img_name%_DOCKERFILE!"
    set "context=!IMAGES_%img_name%_CONTEXT!"
    set "tag=!TAGS_%img_name%!"
    
    rem Check for multiple platforms
    echo "%TARGET_PLATFORMS%" | findstr "," >nul
    if not errorlevel 1 (
        echo [ERROR] Multiple platforms detected in TARGET_PLATFORMS: %TARGET_PLATFORMS%
        echo [ERROR] This local script only supports single platform builds.
        exit /b 1
    )

    rem Ensure ARCH_SUFFIX is set
    if "%ARCH_SUFFIX%"=="" (
        for /f "tokens=2 delims=/" %%a in ("%TARGET_PLATFORMS%") do set "ARCH_SUFFIX=%%a"
    )
    if "%ARCH_SUFFIX%"=="" (
        echo [ERROR] Failed to extract architecture suffix from TARGET_PLATFORMS: %TARGET_PLATFORMS%
        exit /b 1
    )
    
    set "full_tag=!tag!-!ARCH_SUFFIX!"
    rem Per-service repository layout, e.g.:
    rem public.ecr.aws/m8q5m4u3/mega/mono-engine:latest-amd64
    set "image_repo=%REGISTRY%/%REGISTRY_ALIAS%/%REPOSITORY%/!img_name!"
    set "full_image=!image_repo!:!full_tag!"
    
    if "%SHOULD_PUSH%"=="true" (
        echo [INFO] Building and pushing !img_name! ^(!full_tag!^)...
    ) else (
        echo [INFO] Building !img_name! ^(!full_tag!^)...
    )
    echo [INFO]   Dockerfile: !dockerfile!
    echo [INFO]   Context: !context!
    echo [INFO]   Platforms: %TARGET_PLATFORMS%
    
    pushd "%REPO_ROOT%"
    
    set "CACHE_ARGS="
    if "%SHOULD_PUSH%"=="true" (
        rem Simplified cache check: always attempt to use registry cache if pushing
        set "CACHE_ARGS=--cache-from type=registry,ref=!full_image!"
    )
    
    rem Build command
    set "BUILD_CMD=docker buildx build --builder mega-builder --platform %TARGET_PLATFORMS% --file !dockerfile! --tag !full_image! --progress=plain --build-arg BUILDKIT_INLINE_CACHE=1 --load"
    
    if defined CACHE_ARGS (
        set "BUILD_CMD=!BUILD_CMD! !CACHE_ARGS!"
    )
    
    set "BUILD_CMD=!BUILD_CMD! --cache-to type=inline !context!"
    
    rem Execute Build
    call !BUILD_CMD!
    if errorlevel 1 (
        echo [ERROR] Failed to build !img_name!
        popd
        exit /b 1
    )
    
    if "%SHOULD_PUSH%"=="true" (
        if not errorlevel 1 (
            docker push "!full_image!"
            if errorlevel 1 (
                echo [ERROR] Failed to push !img_name!
                popd
                exit /b 1
            )
            echo [INFO] !img_name! built and pushed successfully.
            echo [INFO]   Image: !full_image!
        )
    ) else (
        echo [INFO] !img_name! built successfully ^(local only^).
        echo [INFO]   Image: !full_image!
    )
    
    popd
    exit /b 0

:build_all
    if "%SHOULD_PUSH%"=="true" (
        echo [INFO] Building and pushing all demo images...
    ) else (
        echo [INFO] Building all demo images...
    )
    
    set "FAILED_IMAGES="
    for %%i in (%IMAGE_ORDER%) do (
        echo.
        echo [INFO] ==========================================
        echo [INFO] Building: %%i
        echo [INFO] ==========================================
        call :build_and_push "%%i"
        if errorlevel 1 (
            echo [ERROR] Failed to build %%i
            set "FAILED_IMAGES=!FAILED_IMAGES! %%i"
        )
    )
    
    echo.
    if not "!FAILED_IMAGES!"=="" (
        echo [ERROR] ==========================================
        echo [ERROR] Some images failed to build:
        echo [ERROR] !FAILED_IMAGES!
        echo [ERROR] ==========================================
        exit /b 1
    )
    
    echo [INFO] ==========================================
    if "%SHOULD_PUSH%"=="true" (
        echo [INFO] All images built and pushed successfully!
    ) else (
        echo [INFO] All images built successfully!
    )
    echo [INFO] ==========================================
    exit /b 0
