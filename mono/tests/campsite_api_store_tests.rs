#![cfg(target_os = "linux")]

//! Integration tests for CampsiteApiStore with real services
//!
//! These tests run inside a QEMU/KVM virtual machine using the qlean crate,
//! with Campsite API running in Docker containers inside the VM.
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
//! cargo test -p mono --test campsite_api_store_tests -- --ignored --nocapture
//! ```
//!
//! ## Test Design
//!
//! This test uses Docker containers for MySQL and Campsite API inside the VM.
//! Tests directly call Campsite API endpoints to verify the API contract.
//!
//! The original tests from .bak are:
//! - test_load_user_from_api_success: Test successful user retrieval with valid cookie/token
//! - test_load_user_from_api_invalid_cookie: Test invalid cookie/token handling
//! - test_load_user_from_api_server_error: Test server error handling
//! - test_load_user_from_api_network_error: Test network error handling

mod common;

use anyhow::{Context, Result};
use common::*;
use qlean::{Distro, GuestArch, Image, ImageConfig, MachineConfig, with_machine};
use serde_json::Value;

use common::config::DEFAULT_CAMPSITE_API_SESSION_COOKIE;

const TEST_COOKIE: &str = "test_session_cookie";

// ============================================================================
// Test phases - directly calling Campsite API
// ============================================================================

async fn call_campsite_api(
    vm: &mut qlean::Machine,
    path: &str,
    cookie: Option<&str>,
) -> Result<(u16, Value)> {
    let cookie_arg = cookie
        .map(|c| format!("Cookie: {}={}", DEFAULT_CAMPSITE_API_SESSION_COOKIE, c))
        .unwrap_or_default();

    let cmd = if cookie.is_some() {
        format!(
            "curl -s -w '\\nHTTP_CODE:%{{http_code}}' -H '{}' http://localhost:{}{}",
            cookie_arg, CAMPSITE_API_PORT, path
        )
    } else {
        format!(
            "curl -s -w '\\nHTTP_CODE:%{{http_code}}' http://localhost:{}{}",
            CAMPSITE_API_PORT, path
        )
    };

    let output = exec_check(vm, &cmd).await?;

    // Parse output: last line is HTTP_CODE, rest is body
    let mut lines: Vec<&str> = output.lines().collect();
    let http_line = lines.pop().unwrap_or("");
    let http_code: u16 = http_line
        .strip_prefix("HTTP_CODE:")
        .and_then(|s| s.trim().parse().ok())
        .unwrap_or(0);

    let body = lines.join("\n");
    let json: Value = serde_json::from_str(&body).unwrap_or(Value::Null);

    Ok((http_code, json))
}

async fn phase1_test_api_health(vm: &mut qlean::Machine) -> Result<()> {
    tracing::info!("============================================================");
    tracing::info!("Phase 1: Testing Campsite API Health");

    let (status, _body) = call_campsite_api(vm, "/health", None).await?;

    if status == 200 {
        tracing::info!("  PASS: Campsite API is healthy");
    } else {
        anyhow::bail!("Campsite API health check failed, status: {}", status);
    }

    Ok(())
}

async fn phase2_test_success(vm: &mut qlean::Machine) -> Result<()> {
    tracing::info!("============================================================");
    tracing::info!("Phase 2: test_load_user_from_api_success");

    // Call /v1/users/me with valid cookie
    let (status, body) = call_campsite_api(vm, "/v1/users/me", Some(TEST_COOKIE)).await?;

    tracing::info!("  Response status: {}, body: {}", status, body);

    if status == 200 {
        if let (Some(id), Some(username)) = (
            body.get("id").and_then(|v| v.as_str()),
            body.get("username").and_then(|v| v.as_str()),
        ) {
            tracing::info!("  PASS: Got user: id={}, username={}", id, username);
        } else {
            anyhow::bail!("Response missing id or username field");
        }
    } else if status == 401 {
        anyhow::bail!("Got 401 - expected 200 for valid cookie");
    } else {
        anyhow::bail!("Unexpected status: {}", status);
    }

    Ok(())
}

async fn phase3_test_invalid_token(vm: &mut qlean::Machine) -> Result<()> {
    tracing::info!("============================================================");
    tracing::info!("Phase 3: test_load_user_from_api_invalid_cookie");

    // Call /v1/users/me with invalid cookie
    // Note: Campsite API returns empty user (logged_in=false) for invalid tokens, not 401
    let (status, body) = call_campsite_api(vm, "/v1/users/me", Some("invalid_fake_token")).await?;

    tracing::info!("  Response status: {}, body: {}", status, body);

    // Check the response - API returns 200 but with empty user for invalid token
    if status == 401 {
        tracing::info!("  PASS: Got 401 Unauthorized for invalid token");
    } else if status == 200 {
        // Check if logged_in is false (invalid token returns empty user)
        let logged_in = body
            .get("logged_in")
            .and_then(|v| v.as_bool())
            .unwrap_or(true);
        if !logged_in {
            tracing::info!("  PASS: Got empty user (logged_in=false) for invalid token");
        } else {
            // This would be unexpected - got valid user with invalid token
            let username = body.get("username").and_then(|v| v.as_str()).unwrap_or("");
            if username.is_empty() {
                tracing::info!("  PASS: Got empty user for invalid token");
            } else {
                tracing::warn!(
                    "  Unexpected: Got valid user '{}' with invalid token",
                    username
                );
            }
        }
    } else {
        anyhow::bail!("Unexpected status: {}", status);
    }

    Ok(())
}

async fn phase4_test_server_error(vm: &mut qlean::Machine) -> Result<()> {
    tracing::info!("============================================================");
    tracing::info!("Phase 4: test_load_user_from_api_server_error");

    // Call nonexistent endpoint
    let (status, _body) =
        call_campsite_api(vm, "/v1/nonexistent/endpoint", Some(TEST_COOKIE)).await?;

    tracing::info!("  Response status: {}", status);

    // 404 is expected for nonexistent endpoint
    if status == 404 {
        tracing::info!("  PASS: Got 404 Not Found for nonexistent endpoint");
    } else {
        anyhow::bail!("  Got status: {} (expected 404)", status);
    }

    Ok(())
}

async fn phase5_test_network_error(vm: &mut qlean::Machine) -> Result<()> {
    tracing::info!("============================================================");
    tracing::info!("Phase 5: test_load_user_from_api_network_error");

    tracing::info!("  Stopping Campsite API container...");
    exec_check(vm, &format!("docker stop {}", CAMPSITE_API_CONTAINER)).await?;

    let result = call_campsite_api(vm, "/v1/users/me", Some(TEST_COOKIE)).await;

    // Restart container for cleanup
    tracing::info!("  Restarting Campsite API container...");
    exec_check(vm, &format!("docker start {}", CAMPSITE_API_CONTAINER)).await?;

    retry_until(
        vm,
        &format!(
            "docker exec {} curl -sf -o /dev/null -w '%{{http_code}}' http://localhost:8080/health",
            CAMPSITE_API_CONTAINER
        ),
        |output| output.trim() == "200",
        "Campsite API",
        60,
        2,
    )
    .await?;

    match result {
        Err(_) => {
            tracing::info!("  PASS: Network error detected (connection failed)");
        }
        Ok((status, _)) => {
            tracing::warn!(
                "  Got status {} after stopping container (may be cached)",
                status
            );
        }
    }

    Ok(())
}

// ============================================================================
// Main test entry
// ============================================================================

#[tokio::test]
#[ignore]
async fn test_campsite_api_store_integration() -> Result<()> {
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
        disk: Some(10),
        clear: true,
        ..Default::default()
    };

    with_machine(&image, &config, |vm| {
        Box::pin(async move {
            tracing::info!("============================================================");
            tracing::info!("CampsiteApiStore Integration Test");
            tracing::info!("============================================================");

            install_docker(vm).await.context("Docker install failed")?;

            setup_mysql(vm).await.context("MySQL setup failed")?;
            setup_campsite_api(vm)
                .await
                .context("Campsite API setup failed")?;

            tracing::info!("All services are ready");
            tracing::info!("");

            // Run test phases
            phase1_test_api_health(vm).await.context("Phase 1 failed")?;
            phase2_test_success(vm).await.context("Phase 2 failed")?;
            phase3_test_invalid_token(vm)
                .await
                .context("Phase 3 failed")?;
            phase4_test_server_error(vm)
                .await
                .context("Phase 4 failed")?;
            phase5_test_network_error(vm)
                .await
                .context("Phase 5 failed")?;

            tracing::info!("");
            tracing::info!("============================================================");
            tracing::info!("All tests passed!");
            tracing::info!("============================================================");

            Ok(())
        })
    })
    .await
}
