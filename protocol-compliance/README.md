# x402 Protocol Compliance Test Harness

This directory contains a comprehensive protocol compliance test harness for the x402-rs project. It tests various combinations of client, server, and facilitator implementations across multiple chains and protocol versions.

## Overview

The test harness validates that different implementations (Rust and TypeScript) can interoperate correctly when using the x402 payment protocol. It spins up real services (facilitator, server) and makes actual payment-enabled HTTP requests to verify end-to-end functionality.

### What is x402?

x402 is a protocol for HTTP payment requirements. Servers can mark endpoints as requiring payment, clients include payment headers, and facilitators verify and settle payments on-chain.

## Quick Start

### Prerequisites

1. **Rust toolchain** - For building the Rust binaries
2. **Node.js & pnpm** - For running the test harness
3. **Environment variables** - For blockchain access

### Setup

All commands below should be run from the **repository root** (where the main [`justfile`](justfile) is located).

1. **Install Node.js dependencies:**
   ```bash
   just compliance-install
   ```
   This runs `pnpm install` in the `protocol-compliance/` directory.

2. **Build Rust binaries:**
   ```bash
   just build-all
   ```
   This builds all crates including:
   - `x402-facilitator` (facilitator binary)
   - `x402-axum-example` (Rust server example)
   - `x402-reqwest-example` (Rust client example)

3. **Configure environment:**
   ```bash
   cp protocol-compliance/.env.example protocol-compliance/.env
   # Edit protocol-compliance/.env with your RPC URLs and private keys
   ```

   Required environment variables (in `protocol-compliance/.env`):
   - `BASE_SEPOLIA_RPC_URL` - Base Sepolia RPC endpoint
   - `BASE_SEPOLIA_BUYER_PRIVATE_KEY` - Private key for test buyer (with `0x` prefix)
   - `BASE_SEPOLIA_FACILITATOR_PRIVATE_KEY` - Private key for facilitator
   - `SOLANA_DEVNET_RPC_URL` - Solana Devnet RPC endpoint
   - `SOLANA_DEVNET_BUYER_PRIVATE_KEY` - Base58-encoded private key for Solana buyer
   - `SOLANA_DEVNET_FACILITATOR_PRIVATE_KEY` - Base58-encoded private key for Solana facilitator

### Running Tests

All commands are run from the **repository root** using the main [`justfile`](justfile):

```bash
# Run all protocol compliance tests (builds + runs tests)
just compliance-test-all

# Run all tests without rebuilding
just compliance-test

# Type check the TypeScript code
just compliance-typecheck

# Install dependencies
just compliance-install
```

For more granular control, you can run commands directly from the `protocol-compliance/` directory:

```bash
cd protocol-compliance

# Run all tests
pnpm test

# Run specific test file
pnpm vitest run v2-eip155-exact-rs-rs-rs

# Run tests matching a pattern
pnpm vitest run -t eip155

# Watch mode for development
pnpm test:watch

# Type check
pnpm typecheck
```

## Test Matrix

The test harness supports these axes of configuration (not all combinations are valid or interesting):

| x402 Version | Client    | Server    | Facilitator | Namespace | Scheme | Extension                                   |
|--------------|-----------|-----------|-------------|-----------|--------|---------------------------------------------|
| v1           | Rust (rs) | Rust (rs) | Local (rs)  | eip155    | exact  | (none)                                      |
| v2           | TS (ts)   | TS (ts)   | TS (@x402)  | solana    |        | eip2612GasSponsoring (eip155 + exact)       |
|              |           |           | Remote      | aptos     |        | erc20ApprovalGasSponsoring (eip155 + exact) |
|              |           |           |             |           |        | sign-in-with-x                              |
|              |           |           |             |           |        | bazaar (v2 only)                            |

### Current Test Coverage

| # | Combination              | Client     | Server     | Facilitator | Chain  | Status |
|---|--------------------------|------------|------------|-------------|--------|--------|
| 1 | v2-eip155-exact-rs-rs-rs | Rust       | Rust       | Rust        | eip155 | ✅      |
| 2 | v2-eip155-exact-ts-rs-rs | TypeScript | Rust       | Rust        | eip155 | ✅      |
| 3 | v2-eip155-exact-ts-ts-rs | TypeScript | TypeScript | Rust        | eip155 | ✅      |
| 4 | v2-eip155-exact-rs-ts-rs | Rust       | TypeScript | Rust        | eip155 | ✅      |
| 5 | v2-solana-exact-rs-rs-rs | Rust       | Rust       | Rust        | Solana | ✅      |
| 6 | v2-solana-exact-ts-rs-rs | TypeScript | Rust       | Rust        | Solana | ✅      |
| 7 | v2-solana-exact-rs-ts-rs | Rust       | TypeScript | Rust        | Solana | ✅      |
| 8 | v2-solana-exact-ts-ts-rs | TypeScript | TypeScript | Rust        | Solana | ✅      |

> **Note:** Combination 3 (TS Client + TS Server + Rust Facilitator) is critical for testing the Rust facilitator's compatibility with the canonical TypeScript implementation, isolating any quirks in the Rust facilitator.

### Naming Convention

Test files follow the pattern:
```
v{version}-{namespace}-{scheme}-{client}-{server}-{facilitator}.{modifier}.test.ts
```

The `.{modifier}` part is optional and used for extension tests.

**Base pattern:** `v{version}-{namespace}-{scheme}-{client}-{server}-{facilitator}.test.ts`

Example: `v2-eip155-exact-rs-rs-rs.test.ts` means:
- x402 **v2**
- **EIP155** chain namespace
- **exact** payment scheme
- **Rust** client
- **Rust** server
- **Rust** facilitator

**With modifier:** `v{version}-{namespace}-{scheme}-{client}-{server}-{facilitator}.{modifier}.test.ts`

Example: `v2-eip155-exact-rs-rs-rs.siwx.test.ts` means the same as above, but with the **sign-in-with-x** extension/modifier applied.

**Available modifiers (for future extensions):**
- `siwx` - Sign-in with X authentication
- `bazaar` - Bazaar marketplace extension (v2 only)
- `eip2612` - EIP2612 Gas Sponsoring
- `erc20` - ERC20 Approval Gas Sponsoring

## Architecture

### Test Lifecycle

Each test follows this pattern:

```typescript
describe('test-name', () => {
  let facilitator: RSFacilitatorHandle;
  let server: RSServerHandle | TSServerHandle;

  beforeAll(async () => {
    // 1. Start facilitator
    facilitator = await RSFacilitatorHandle.spawn();
    
    // 2. Start server (connected to facilitator)
    server = await RSServerHandle.spawn(facilitator.url);
  }, 120000);

  afterAll(async () => {
    // 3. Clean up in reverse order
    await server.stop();
    await facilitator.stop();
  });

  it('should verify payment flow', async () => {
    // 4. Invoke client and verify results
    const stdout = await invokeRustClient(endpoint, { eip155: privateKey });
    expect(stdout).toContain('VIP content');
  });
});
```

### Key Components

#### 1. Facilitator Handle ([`src/utils/facilitator.ts`](src/utils/facilitator.ts))

Manages the Rust facilitator binary lifecycle:

```typescript
export class RSFacilitatorHandle {
  readonly url: URL;
  readonly process: ProcessHandle;

  static async spawn(port?: number): Promise<RSFacilitatorHandle>;
  async stop(): Promise<void>;
}
```

- Spawns `target/debug/x402-facilitator` binary
- Automatically allocates an available port
- Waits for health check before returning
- Generates temporary config file with chain settings

#### 2. Server Handles ([`src/utils/server.ts`](src/utils/server.ts))

**Rust Server (`RSServerHandle`):**
- Spawns `target/debug/x402-axum-example` binary
- Configured via `FACILITATOR_URL` and `PORT` environment variables

**TypeScript Server (`TSServerHandle`):**
- Creates Hono server with `@x402/hono` middleware
- Registers payment schemes for EIP155 and Solana
- Supports both chains in a single server instance

#### 3. Client Utilities ([`src/utils/client.ts`](src/utils/client.ts))

**Rust Client (`invokeRustClient`):**
- Spawns `target/debug/x402-reqwest-example` as one-shot process
- Returns stdout for verification
- Supports EIP155 and Solana private keys

**TypeScript Client (`makeFetch`):**
- Returns an x402-enabled fetch function
- Uses `@x402/fetch` with `@x402/evm` or `@x402/svm` schemes
- Async initialization for Solana keypair

### Process Management

All Rust binaries are managed through `ProcessHandle` ([`src/utils/process-handle.ts`](src/utils/process-handle.ts)), which provides:
- Consistent logging with role-based prefixes (e.g., `[rs-facilitator]`)
- Graceful shutdown on test completion
- Exit detection for early failure detection

## Project Structure

```
protocol-compliance/
├── src/
│   ├── tests/                    # Test files
│   │   ├── v2-eip155-exact-*.ts  # EIP155 tests
│   │   └── v2-solana-exact-*.ts  # Solana tests
│   ├── utils/
│   │   ├── facilitator.ts        # Facilitator lifecycle management
│   │   ├── server.ts             # Server handle implementations
│   │   ├── client.ts             # Client invocation utilities
│   │   ├── config.ts             # Environment & facilitator config
│   │   ├── process-handle.ts     # Process lifecycle management
│   │   ├── waitFor.ts            # Polling utilities
│   │   └── workspace-root.ts     # Repository root reference
│   ├── types/
│   │   └── index.ts              # TypeScript type definitions
│   └── index.ts                  # Main entry point
├── package.json                  # Node.js dependencies
├── tsconfig.json                 # TypeScript configuration
├── vitest.config.ts              # Test runner configuration
├── .env.example                  # Environment template
└── README.md                     # This file
```

**Note:** The main [`justfile`](justfile) is in the repository root, not in this directory.

## Test Scenarios

### Basic Payment Flow

```
┌─────────┐      GET /resource       ┌─────────┐
│ Client  │ ───────────────────────> │ Server  │
└─────────┘                          └─────────┘
     │                                    │
     │<──────────────────────── 402 Payment Required
     │                                    │
     │ GET /resource + X-Payment header   │
     │ ─────────────────────────────────> │
     │                                    │
     │                              Verify Payment
     │                              ┌─────────────┐
     │                              │ Facilitator │
     │                              └─────────────┘
     │                                    │
     │<──────────────────────── 200 OK + Content
```

### What Each Test Verifies

1. **Facilitator health** - Facilitator responds to health checks
2. **402 without payment** - Protected endpoints return 402 when no payment header
3. **200 with payment** - Valid payment headers result in successful response with VIP content

## Future Work

### Planned Additions

| Category           | Item                          | Status             |
|--------------------|-------------------------------|--------------------|
| **Chains**         | Aptos support                 | ⏳ Pending          |
| **Schemes**        | upto                          | ⏳ Pending          |
| **Extensions**     | EIP2612 Gas Sponsoring        | ⏳ Pending          |
| **Extensions**     | ERC20 Approval Gas Sponsoring | ⏳ Pending          |
| **Extensions**     | Sign-in with X (SIWX)         | ⏳ Pending          |
| **Extensions**     | Bazaar                        | ⏳ Pending          |
| **Versions**       | v1 protocol tests             | No longer relevant |
| **Infrastructure** | Remote facilitator support    | ⏳ Pending          |

### Extension Details

Extensions modify the base protocol behavior:

- **EIP2612 Gas Sponsoring** - Allows gasless transactions via permits
- **ERC20 Approval Gas Sponsoring** - Alternative gasless approach
- **Sign-in with X (SIWX)** - Authentication extension
- **Bazaar** - Marketplace extension (v2 only)

These will be tested with modifier suffixes:
```
v2-eip155-exact-rs-rs-rs.siwx.test.ts
v2-eip155-exact-rs-rs-rs.bazaar.test.ts
```

## Troubleshooting

### Common Issues

**Facilitator fails to start:**
- Check that `target/debug/x402-facilitator` exists (run `just build-all` from repo root)
- Verify environment variables are set in `protocol-compliance/.env`
- Check RPC URLs are accessible

**Port conflicts:**
- The harness uses `get-port` for automatic port allocation
- If ports are exhausted, check for zombie processes: `lsof -i :PORT`

**Solana tests failing:**
- Verify private keys are base58-encoded (not hex)
- Ensure Solana devnet RPC is accessible
- Check that accounts have sufficient devnet SOL or USDC

**EIP155 tests failing:**
- Verify private keys have `0x` prefix
- Ensure Base Sepolia RPC is accessible
- Check that accounts have sufficient testnet ETH or USDC

### Debug Mode

Run tests with verbose output:
```bash
cd protocol-compliance && pnpm test -- --verbose
```

## Contributing

When adding new tests:

1. Follow the naming convention: `v{version}-{namespace}-{scheme}-{client}-{server}-{facilitator}.test.ts`
2. Use handle-based lifecycle management (`spawn()` and `stop()`)
3. Set 120s timeout on `beforeAll` for service startup
4. Test both 402 (without payment) and 200 (with payment) scenarios
5. Add the test to the coverage table in this README

## Related Documentation

- [x402 Protocol Specs](../docs/specs/)
- [How to Write a Scheme](../docs/how-to-write-a-scheme.md)
- [Build Your Own Facilitator](../docs/build-your-own-facilitator.md)
