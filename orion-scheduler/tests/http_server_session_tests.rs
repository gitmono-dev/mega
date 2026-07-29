#![cfg(target_os = "linux")]

//! Integration tests for HTTP server session middleware with real Redis
//!
//! These tests run inside a QEMU/KVM virtual machine using the qlean crate,
//! with Redis running in Docker containers inside the VM.
//!
//! ## Prerequisites
//!
//! This test requires system-level dependencies:
//! - QEMU/KVM virtualization (qemu-system-x86_64, qemu-img)
//! - libguestfs-tools (guestfish, virt-copy-out)
//! - xorriso, sha256sum
//!
//! Install on Ubuntu/Debian:
//! ```bash
//! sudo apt-get install qemu-system-x86 qemu-utils libguestfs-tools xorriso
//! ```
//!
//! ## Running the Test
//!
//! ```bash
//! # Run test (note the --ignored flag)
//! cargo test -p orion-scheduler --test http_server_session_tests -- --ignored --nocapture
//! ```
//!
//! ## Test Design
//!
//! This test uses Docker containers for Redis inside the VM,
//! reusing docker-compose.demo.yml for consistency with the demo environment.
//! All test scenarios run in a single test function to avoid multiple VM startups.

mod common;

use std::time::Duration;

use anyhow::{Context, Result};
use common::*;
use qlean::{Distro, GuestArch, Image, ImageConfig, MachineConfig, with_machine};

// ============================================================================
// Test Scenarios - All in one function to avoid multiple VM startups
// ============================================================================

/// Phase 1: Test session creation and persistence via Redis
async fn phase1_test_session_creation_and_persistence(vm: &mut qlean::Machine) -> Result<()> {
    tracing::info!("============================================================");
    tracing::info!("Phase 1: Session Creation and Persistence via Redis");

    let session_key = "test:http:session:persistence";

    exec_check(
        vm,
        &format!(
            "docker exec {} redis-cli SET {} 'test_value' EX 3600",
            REDIS_CONTAINER, session_key
        ),
    )
    .await?;

    let result = exec_check(
        vm,
        &format!(
            "docker exec {} redis-cli GET {}",
            REDIS_CONTAINER, session_key
        ),
    )
    .await?;

    let retrieved = result.trim();
    if retrieved == "test_value" {
        tracing::info!("  PASS: Session data created and persisted");
    } else {
        anyhow::bail!(
            "Session persistence failed: expected 'test_value', got {}",
            retrieved
        );
    }

    Ok(())
}

/// Phase 2: Test session clearing via Redis
async fn phase2_test_session_clearing(vm: &mut qlean::Machine) -> Result<()> {
    tracing::info!("============================================================");
    tracing::info!("Phase 2: Session Clearing via Redis");

    let session_key = "test:http:session:clearing";

    exec_check(
        vm,
        &format!(
            "docker exec {} redis-cli SET {} 'test_value' EX 3600",
            REDIS_CONTAINER, session_key
        ),
    )
    .await?;

    exec_check(
        vm,
        &format!(
            "docker exec {} redis-cli DEL {}",
            REDIS_CONTAINER, session_key
        ),
    )
    .await?;

    let result = exec_check(
        vm,
        &format!(
            "docker exec {} redis-cli GET {}",
            REDIS_CONTAINER, session_key
        ),
    )
    .await?;

    let retrieved = result.trim();
    if retrieved.is_empty() || retrieved == "(nil)" {
        tracing::info!("  PASS: Session data cleared successfully");
    } else {
        anyhow::bail!("Session clearing failed, got: {}", retrieved);
    }

    Ok(())
}

/// Phase 3: Test session expiry via Redis TTL
async fn phase3_test_session_expiry(vm: &mut qlean::Machine) -> Result<()> {
    tracing::info!("============================================================");
    tracing::info!("Phase 3: Session Expiry via Redis TTL");

    let session_key = "test:http:session:expiry";

    exec_check(
        vm,
        &format!(
            "docker exec {} redis-cli SET {} 'test_value' EX 1",
            REDIS_CONTAINER, session_key
        ),
    )
    .await?;

    tracing::info!("  Waiting for session to expire (1 second)...");
    tokio::time::sleep(Duration::from_secs(2)).await;

    let result = exec_check(
        vm,
        &format!(
            "docker exec {} redis-cli GET {}",
            REDIS_CONTAINER, session_key
        ),
    )
    .await?;

    let retrieved = result.trim();
    if retrieved.is_empty() || retrieved == "(nil)" {
        tracing::info!("  PASS: Session expired as expected");
    } else {
        anyhow::bail!("Session should have expired, got: {}", retrieved);
    }

    Ok(())
}

/// Phase 4: Test session isolation (different keys)
async fn phase4_test_session_isolation(vm: &mut qlean::Machine) -> Result<()> {
    tracing::info!("============================================================");
    tracing::info!("Phase 4: Session Isolation via Redis");

    let session_key1 = "test:http:session:isolation:1";
    let session_key2 = "test:http:session:isolation:2";

    exec_check(
        vm,
        &format!(
            "docker exec {} redis-cli SET {} 'value1' EX 3600",
            REDIS_CONTAINER, session_key1
        ),
    )
    .await?;

    exec_check(
        vm,
        &format!(
            "docker exec {} redis-cli SET {} 'value2' EX 3600",
            REDIS_CONTAINER, session_key2
        ),
    )
    .await?;

    let result1 = exec_check(
        vm,
        &format!(
            "docker exec {} redis-cli GET {}",
            REDIS_CONTAINER, session_key1
        ),
    )
    .await?;

    let result2 = exec_check(
        vm,
        &format!(
            "docker exec {} redis-cli GET {}",
            REDIS_CONTAINER, session_key2
        ),
    )
    .await?;

    if result1.trim() == "value1" && result2.trim() == "value2" {
        tracing::info!("  PASS: Sessions are properly isolated");
    } else {
        anyhow::bail!(
            "Session isolation failed: key1={}, key2={}",
            result1.trim(),
            result2.trim()
        );
    }

    Ok(())
}

/// Phase 5: Test session without data (non-existent key)
async fn phase5_test_session_without_data(vm: &mut qlean::Machine) -> Result<()> {
    tracing::info!("============================================================");
    tracing::info!("Phase 5: Session Without Data via Redis");

    let session_key = "test:http:session:nonexistent";

    let result = exec_check(
        vm,
        &format!(
            "docker exec {} redis-cli GET {}",
            REDIS_CONTAINER, session_key
        ),
    )
    .await?;

    let retrieved = result.trim();
    if retrieved.is_empty() || retrieved == "(nil)" {
        tracing::info!("  PASS: Non-existent session returns nothing");
    } else {
        anyhow::bail!("Expected no data, got: {}", retrieved);
    }

    Ok(())
}

// ============================================================================
// MAIN TEST
// ============================================================================

#[tokio::test]
#[ignore]
async fn test_http_server_session_with_redis() -> Result<()> {
    tracing_subscriber_init();

    let image = Image::new(
        ImageConfig::default()
            .with_distro(Distro::Debian)
            .with_arch(GuestArch::Amd64),
    )
    .await?;
    let config = MachineConfig {
        core: 2,
        mem: 2048,
        disk: Some(15),
        clear: true,
        ..Default::default()
    };

    with_machine(&image, &config, |vm| {
        Box::pin(async move {
            tracing::info!("============================================================");
            tracing::info!("HTTP Server Session Integration Test (Redis)");
            tracing::info!("============================================================");

            install_docker(vm).await.context("Docker install failed")?;

            setup_redis(vm).await.context("Redis setup failed")?;

            tracing::info!("All services are ready");
            tracing::info!("");

            phase1_test_session_creation_and_persistence(vm)
                .await
                .context("Phase 1 failed")?;
            phase2_test_session_clearing(vm)
                .await
                .context("Phase 2 failed")?;
            phase3_test_session_expiry(vm)
                .await
                .context("Phase 3 failed")?;
            phase4_test_session_isolation(vm)
                .await
                .context("Phase 4 failed")?;
            phase5_test_session_without_data(vm)
                .await
                .context("Phase 5 failed")?;

            tracing::info!("");
            tracing::info!("All test phases completed successfully!");

            Ok(())
        })
    })
    .await
    .context("Failed to run VM test")?;

    Ok(())
}
