#![cfg(target_os = "linux")]

//! Integration tests for session management with real Redis
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
//! cargo test -p orion-scheduler --test session_management_tests -- --ignored --nocapture
//! ```
//!
//! ## Test Design
//!
//! This test uses Docker containers for Redis inside the VM,
//! reusing docker-compose.demo.yml for consistency with the demo environment.
//! All test scenarios run in a single test function to avoid multiple VM startups.

mod common;

use anyhow::{Context, Result};
use common::*;
use qlean::{Distro, GuestArch, Image, ImageConfig, MachineConfig, with_machine};

// ============================================================================
// Test Scenarios - All in one function to avoid multiple VM startups
// ============================================================================

/// Phase 1: Test session creation and retrieval via Redis
async fn phase1_test_session_creation_and_retrieval(vm: &mut qlean::Machine) -> Result<()> {
    tracing::info!("============================================================");
    tracing::info!("Phase 1: Session Creation and Retrieval via Redis");

    let session_key = "test:session:creation";
    let test_data =
        r#"{"id":1,"username":"testuser","email":"test@example.com","name":"Test User"}"#;

    exec_check(
        vm,
        &format!(
            "docker exec {} redis-cli SET {} '{}' EX 3600",
            REDIS_CONTAINER, session_key, test_data
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
    if retrieved == test_data {
        tracing::info!("  PASS: Session data successfully created and retrieved");
    } else {
        anyhow::bail!(
            "Session data mismatch: expected {}, got {}",
            test_data,
            retrieved
        );
    }

    Ok(())
}

/// Phase 2: Test session clearing via Redis
async fn phase2_test_session_clearing(vm: &mut qlean::Machine) -> Result<()> {
    tracing::info!("============================================================");
    tracing::info!("Phase 2: Session Clearing via Redis");

    let session_key = "test:session:clearing";
    let test_data = r#"{"id":1,"username":"testuser","email":"test@example.com"}"#;

    exec_check(
        vm,
        &format!(
            "docker exec {} redis-cli SET {} '{}' EX 3600",
            REDIS_CONTAINER, session_key, test_data
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
        tracing::info!("  PASS: Session data successfully cleared");
    } else {
        anyhow::bail!("Session should be cleared but got: {}", retrieved);
    }

    Ok(())
}

/// Phase 3: Test session persistence via Redis
async fn phase3_test_session_persistence(vm: &mut qlean::Machine) -> Result<()> {
    tracing::info!("============================================================");
    tracing::info!("Phase 3: Session Persistence via Redis");

    let session_key = "test:session:persistence";
    let test_data = r#"{"id":1,"username":"testuser","email":"test@example.com"}"#;

    exec_check(
        vm,
        &format!(
            "docker exec {} redis-cli SET {} '{}' EX 3600",
            REDIS_CONTAINER, session_key, test_data
        ),
    )
    .await?;

    for i in 0..3 {
        let result = exec_check(
            vm,
            &format!(
                "docker exec {} redis-cli GET {}",
                REDIS_CONTAINER, session_key
            ),
        )
        .await?;

        let retrieved = result.trim();
        if retrieved == test_data {
            tracing::info!("  Iteration {}: Session data persisted correctly", i + 1);
        } else {
            anyhow::bail!(
                "Session data mismatch at iteration {}: expected {}, got {}",
                i + 1,
                test_data,
                retrieved
            );
        }
    }

    tracing::info!("  PASS: Session data persisted across multiple retrievals");
    Ok(())
}

/// Phase 4: Test session ID generation (using Redis INCR)
async fn phase4_test_session_id_generation(vm: &mut qlean::Machine) -> Result<()> {
    tracing::info!("============================================================");
    tracing::info!("Phase 4: Session ID Generation via Redis");

    let counter_key = "test:session:counter";

    let result1 = exec_check(
        vm,
        &format!(
            "docker exec {} redis-cli INCR {}",
            REDIS_CONTAINER, counter_key
        ),
    )
    .await?;
    let id1 = result1.trim().parse::<i64>()?;

    let result2 = exec_check(
        vm,
        &format!(
            "docker exec {} redis-cli INCR {}",
            REDIS_CONTAINER, counter_key
        ),
    )
    .await?;
    let id2 = result2.trim().parse::<i64>()?;

    if id2 > id1 {
        tracing::info!("  PASS: Generated unique session IDs: {}, {}", id1, id2);
    } else {
        anyhow::bail!("Session IDs should be unique and incrementing");
    }

    Ok(())
}

// ============================================================================
// MAIN TEST
// ============================================================================

#[tokio::test]
#[ignore]
async fn test_session_management_with_redis() -> Result<()> {
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
            tracing::info!("Session Management Integration Test (Redis)");
            tracing::info!("============================================================");

            install_docker(vm).await.context("Docker install failed")?;

            setup_redis(vm).await.context("Redis setup failed")?;

            tracing::info!("All services are ready");
            tracing::info!("");

            phase1_test_session_creation_and_retrieval(vm)
                .await
                .context("Phase 1 failed")?;
            phase2_test_session_clearing(vm)
                .await
                .context("Phase 2 failed")?;
            phase3_test_session_persistence(vm)
                .await
                .context("Phase 3 failed")?;
            phase4_test_session_id_generation(vm)
                .await
                .context("Phase 4 failed")?;

            tracing::info!("");
            tracing::info!("All test phases completed successfully!");

            Ok(())
        })
    })
    .await
    .context("Failed to run VM test")?;

    Ok(())
}
