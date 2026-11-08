# solana-portfolio-api

> High-reliability, fail-closed Solana DeFi portfolio valuation microservice built on Axum and Tokio.

[![Rust](https://img.shields.io/badge/rust-stable-brightgreen.svg)](https://www.rust-lang.org/)
[![License: MIT](https://img.shields.io/badge/License-MIT-blue.svg)](LICENSE)
[![Framework: Axum](https://img.shields.io/badge/Framework-Axum%200.7-purple.svg)](https://github.com/tokio-rs/axum)
[![Runtime: Tokio](https://img.shields.io/badge/Runtime-Tokio%201.x-black.svg)](https://tokio.rs/)
[![Dependencies: Zero Extra](https://img.shields.io/badge/Dependencies-Zero%20Extra-blueviolet.svg)]()
[![Tests: 13/13 Passing](https://img.shields.io/badge/Tests-13%2F13%20Passing-success.svg)]()

---

## Executive Summary

`solana-portfolio-service` is an asynchronous microservice engineered to retrieve, validate, and compute aggregate USD valuations for Solana wallet token holdings. 

In decentralized finance (DeFi), naive implementations frequently fall prey to subtle arithmetic corruption (e.g. `NaN` poisoning, infinite price loops, silent feed drops) or synchronization crashes (unhandled mutex/read-write lock poisoning). This service demonstrates how to construct a resilient, production-grade financial component in idiomatic Rust that guarantees predictable failure modes, strict input normalization, and bounded latency.

---

## Architecture & Data Flow

```
                      +------------------------------------------+
                      |         HTTP Client Request              |
                      |    GET /portfolio?wallet=<address>       |
                      +--------------------+---------------------+
                                           |
                                           v
                      +--------------------+---------------------+
                      |   Axum Rejection-Aware Query Extractor   |
                      |   - Strips whitespace & control chars    |
                      |   - Enforces length bounds (1..=128)     |
                      |   - Rejects blank/missing with HTTP 400  |
                      +--------------------+---------------------+
                                           |
                                           v
                      +--------------------+---------------------+
                      |            PortfolioService              |
                      +----------+--------------------+----------+
                                 |                    |
                   (Cache Hit)   |                    | (Cache Miss)
                                 v                    v
                   +-------------+------+     +-------+------------------+
                   |  MemoryRepository  |     |   SolanaProvider Trait   |
                   |  (Read Lock Safe)  |     |   Bounded RPC Timeout    |
                   +-------------+------+     +-------+------------------+
                                 |                    |
                                 |                    v
                                 |            +-------+------------------+
                                 |            | Fail-Closed Float Audit  |
                                 |            | - Rejects NaN / +/-Inf   |
                                 |            | - Rejects Negatives      |
                                 |            | - Traps Prod/Sum Overflow|
                                 |            +-------+------------------+
                                 |                    |
                                 |                    v
                                 |            +-------+------------------+
                                 |            |  Atomic Cache Insertion  |
                                 |            |  MemoryRepository::save  |
                                 |            +-------+------------------+
                                 |                    |
                                 +----------+---------+
                                            |
                                            v
                      +---------------------+--------------------+
                      |    Standardized JSON Response Body       |
                      |    HTTP 200 OK / Structured Error       |
                      +------------------------------------------+
```

---

## Key Architectural Principles

### 1. Fail-Closed Floating-Point Arithmetic
Binary floating-point (`f64`) operations under IEEE 754 admit invalid states (`NaN`, `+Infinity`, `-Infinity`, negative balances). 
- In financial applications, silently discarding an invalid price feed or clamping it to `0.0` produces incorrect portfolio balances and conceals upstream oracle failure.
- `calculate_total_usd` validates every token position across four invariant checks: non-empty symbols, finite non-negative values, multiplication overflow, and summation overflow.
- If any check fails, execution aborts with `AppError::InvalidPositionData` (HTTP 422). The data is never committed to storage, protecting the cache from corrupted states.

### 2. Poison-Resistant Concurrent Storage
Standard Rust `std::sync::RwLock` transitions to a poisoned state if a thread panics while holding a read or write guard.
- Naive code often calls `.ok()?` or `if let Ok(...)`, silently dropping persistence failures and serving stale or missing data.
- The `Repository` trait explicitly returns `Result<Option<Portfolio>, AppError>` and `Result<(), AppError>`, safely capturing poisoned locks and transforming them into `AppError::Storage` (HTTP 500) with diagnostic logging.

### 3. Rejection-Aware Query Extraction
Default Axum `Query` extractors return `422 Unprocessable Entity` or raw `text/plain` deserialization strings when parameters are missing.
- By extracting `Result<Query<PortfolioQuery>, QueryRejection>` alongside `wallet: Option<String>`, the service intercepts missing (`GET /portfolio`), malformed, and whitespace-only (`GET /portfolio?wallet=   `) requests.
- All invalid inputs deterministically return `HTTP 400 Bad Request` with a uniform JSON body: `{"error": "..."}`.

### 4. Decoupled Provider Trait & Bounded Latency
- Upstream Solana network calls are encapsulated in the `SolanaProvider: Send + Sync` trait using pinned boxed futures (`BoxFuture`), enabling deterministic unit and integration testing without network calls or external mocking frameworks.
- Requests are guarded with `tokio::time::timeout` via `REQUEST_TIMEOUT_MS`. If an RPC endpoint stalls, the call is canceled and translated into `AppError::UpstreamTimeout` (HTTP 504 Gateway Timeout).

### 5. Zero Extra Dependencies
This service is intentionally designed with zero auxiliary third-party crates beyond the core async web stack:
- Uses native `std::fmt::Display` and `std::error::Error` (no `anyhow` or `thiserror`).
- Uses native boxed futures for trait dispatch (no `async-trait`).
- Executes end-to-end HTTP integration tests via raw `tokio::net::TcpStream` (no `reqwest` or `tower-test`).

---

## API Specification

### Get Portfolio Valuation

```http
GET /portfolio?wallet=<WALLET_ADDRESS>
```

#### Query Parameters

| Parameter | Type | Required | Description |
| :--- | :--- | :--- | :--- |
| `wallet` | String | Yes | Solana wallet public key (Base58) or identifier (1..=128 characters). Must not contain interior whitespace or control characters. |

#### Success Response (HTTP 200 OK)

```json
{
  "wallet": "9WzDXwBbmkg8ZTbNMqUxvQRAyrZzDsGYdLVL9zYtAWWM",
  "positions": [
    {
      "token": "SOL",
      "amount": 2.5,
      "usd_price": 150.0
    },
    {
      "token": "USDC",
      "amount": 100.0,
      "usd_price": 1.0
    }
  ],
  "total_usd": 475.0,
  "updated_at": "2026-11-08T09:00:00.000000+00:00"
}
```

#### Error Contract

All error paths return a predictable JSON payload:

```json
{
  "error": "<diagnostic_message>"
}
```

| HTTP Status | Condition | Root Cause / Handling |
| :--- | :--- | :--- |
| `400 Bad Request` | Missing, blank, or malformed `wallet` query parameter. | Client validation error. |
| `422 Unprocessable Entity` | Upstream data violates invariants (`NaN`, `Inf`, `< 0.0`, overflow). | Refuses corrupted valuation feeds. |
| `500 Internal Server Error`| Internal storage or lock poisoning failure. | Controlled containment of concurrent synchronization faults. |
| `502 Bad Gateway` | Upstream Solana RPC returns unparseable or rejected response. | External dependency failure. |
| `504 Gateway Timeout` | Upstream Solana RPC exceeds configured request timeout. | Enforces bounded response latencies. |

---

## Configuration

The service is configured via environment variables:

| Variable | Type | Default | Description |
| :--- | :--- | :--- | :--- |
| `PORT` | `u16` | `8080` | TCP port for the Axum HTTP listener. |
| `REQUEST_TIMEOUT_MS` | `u64` | `5000` | Maximum latency threshold before an upstream call returns HTTP 504. |

---

## Getting Started

### Prerequisites
* Rust 1.75+ (`cargo`, `rustc`)

### 1. Build
```bash
cargo build --release
```

### 2. Run Test Suite
Execute the 13 automated unit and integration tests:
```bash
cargo test
```

### 3. Start Service
```bash
cargo run --release
```
The server will start listening on `0.0.0.0:8080`.

### 4. Verify Endpoints
```bash
# Valid portfolio request (HTTP 200)
curl -i "http://127.0.0.1:8080/portfolio?wallet=test_wallet"

# Missing wallet parameter (HTTP 400)
curl -i "http://127.0.0.1:8080/portfolio"

# Whitespace-only wallet parameter (HTTP 400)
curl -i "http://127.0.0.1:8080/portfolio?wallet=%20%20%20"
```

---

## Test Suite Architecture

The repository maintains 100% pass rates across unit, concurrency, and wire-level integration tests:

| Test Name | Layer | Coverage Focus |
| :--- | :--- | :--- |
| `test_calculate_total_usd_valid` | Unit | Standard multi-token accumulation and empty portfolio boundary (0.0). |
| `test_calculate_total_usd_negative_amount_and_price` | Unit | Rejection of negative holdings and negative price feeds. |
| `test_calculate_total_usd_nan_and_infinity` | Unit | Trap validation against `f64::NAN`, `f64::INFINITY`, and `f64::NEG_INFINITY`. |
| `test_calculate_total_usd_overflow_and_empty_token` | Unit | Arithmetic overflow prevention (`1e300 * 1e100`) and empty token strings. |
| `test_normalize_wallet` | Unit | Whitespace trimming, blank inputs, interior whitespace, and length checks. |
| `test_service_caching_and_miss` | Concurrency | Cache miss provider invocation followed by atomic cache hit bypass. |
| `test_service_invalid_positions_not_cached` | Concurrency | Verifies repository is not poisoned when an upstream provider returns corrupted data. |
| `test_service_upstream_timeout` | Concurrency | Mock provider latency injection triggering `AppError::UpstreamTimeout`. |
| `test_handler_successful_portfolio_request` | Wire (TCP) | End-to-end HTTP/1.1 exchange over ephemeral socket verifying JSON 200 OK. |
| `test_handler_missing_wallet_returns_400_json` | Wire (TCP) | Verifies `GET /portfolio` returns HTTP 400 with structured JSON error. |
| `test_handler_blank_wallet_returns_400_json` | Wire (TCP) | Verifies `GET /portfolio?wallet=%20%20` returns HTTP 400 with structured JSON. |
| `test_handler_upstream_timeout_returns_504_json`| Wire (TCP) | Injects mock provider latency to verify wire-level HTTP 504 Gateway Timeout. |
| `test_handler_upstream_error_returns_502_json` | Wire (TCP) | Injects upstream failure to verify wire-level HTTP 502 Bad Gateway. |

---

## Production Considerations

In production blockchain and accounting applications, binary floating-point (`f64`) values are subject to decimal rounding discrepancies inherent to binary representation. 

While this service demonstrates robust `f64` boundary validation (preventing panics and undefined state serialization), a mission-critical ledger should transition to:
1. **Fixed-point decimal types** (such as `rust_decimal` or `decnumber`) to guarantee exact fractional math.
2. **Native integer units** (e.g. integer `u64` / `u128` representing base lamports or SPL token fractional units) combined with explicit token decimal metadata.

---

## License

This project is licensed under the [MIT License](LICENSE).
