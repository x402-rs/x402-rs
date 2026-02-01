# How to Write a Scheme for x402-rs

This guide explains how to create a custom payment scheme for the x402-rs **facilitator** (server-side). Schemes define how the facilitator verifies and settles payments on behalf of resource servers.

## What is a Scheme?

A **scheme** defines how a payment is verified and settled on a specific blockchain. It encapsulates:

- **Payload format** — The structure of payment data (signatures, transactions, authorizations)
- **Verification logic** — How to validate a payment is correct before execution
- **Settlement logic** — How to execute the payment on-chain
- **Supported chains** — Which blockchain networks the scheme works with

For example, the `exact` scheme implements ERC-3009 `transferWithAuthorization` for EVM chains and SPL token transfers for Solana. You might create a new scheme for subscription payments, escrow flows, or alternative token standards.

## Overview

| Concept               | Open/Closed | Description                                                                                                           |
|-----------------------|-------------|-----------------------------------------------------------------------------------------------------------------------|
| **Schemes**           | **Open**    | Widely extensible. Anyone can create custom schemes for new payment flows.                                            |
| **Protocol Versions** | Closed      | Fixed set: v1 and v2. Defined by the x402 specification. (v1 is legacy, v2 will probably live for the next few years) |
| **Chain Providers**   | Closed      | Predefined set for the implementation due to chain-specific complexity.                                               |

## Architecture

```mermaid
flowchart TB
    subgraph Registration
        SB[SchemeBlueprints] -->|by id| BP[X402SchemeBlueprint]
        BP -->|build with ChainProvider| H[Box dyn X402SchemeFacilitator]
    end

    subgraph Runtime
        SR[SchemeRegistry] -->|by_slug| H
        H -->|verify| VR[VerifyResponse]
        H -->|settle| SR2[SettleResponse]
        H -->|supported| SPR[SupportedResponse]
    end
```

## Crate Structure

Schemes are organized in chain-specific crates under `crates/chains/`:

```
crates/
├── x402-types/           # Core types and traits
│   └── src/scheme/       # X402SchemeId, X402SchemeFacilitator, etc.
├── x402-chain-solana/    # Solana-specific implementations
│   └── src/
│       ├── v1_solana_exact/
│       └── v2_solana_exact/
├── x402-chain-eip155/    # EVM-specific implementations
│   └── src/
│       ├── v1_eip155_exact/
│       └── v2_eip155_exact/
└── x402-chain-aptos/     # Aptos-specific implementations
    └── src/
        └── v2_aptos_exact/
```

Each scheme directory contains:
- `mod.rs` - Module exports and scheme ID implementation
- `facilitator.rs` - Facilitator implementation (server-side)
- `client.rs` - Client implementation (optional)
- `server.rs` - Server types (optional)
- `types.rs` - Scheme-specific types

## Naming Convention

Scheme IDs follow the pattern: `v{version}-{namespace}-{scheme}`

| ID | Struct Name | Directory |
|------|-------------|-----------|
| `v2-solana-exact` | `V2SolanaExact` | `v2_solana_exact/` |
| `v1-eip155-exact` | `V1Eip155Exact` | `v1_eip155_exact/` |
| `v2-solana-myscheme` | `V2SolanaMyscheme` | `v2_solana_myscheme/` |

This makes it easy to map between IDs, chain namespaces, scheme names, and code.

## Core Traits and Structs

### X402SchemeId

Provides identification for a scheme. This trait defines the scheme's version, namespace, and name:

```rust
pub trait X402SchemeId {
    /// The x402 protocol version (1 or 2). Defaults to 2.
    fn x402_version(&self) -> u8 {
        2
    }
    
    /// The chain namespace (e.g., "eip155", "solana")
    fn namespace(&self) -> &str;
    
    /// The scheme name (e.g., "exact", "myscheme")
    fn scheme(&self) -> &str;
    
    /// Computed ID: "v{version}-{namespace}-{scheme}"
    fn id(&self) -> String {
        format!(
            "v{}-{}-{}",
            self.x402_version(),
            self.namespace(),
            self.scheme()
        )
    }
}
```

### X402SchemeFacilitatorBuilder

Factory for creating scheme facilitators:

```rust
pub trait X402SchemeFacilitatorBuilder<P> {
    /// Creates a new scheme handler for the given chain provider.
    ///
    /// # Arguments
    ///
    /// * `provider` - The chain provider to use for on-chain operations
    /// * `config` - Optional scheme-specific configuration
    fn build(
        &self,
        provider: P,
        config: Option<serde_json::Value>,
    ) -> Result<Box<dyn X402SchemeFacilitator>, Box<dyn std::error::Error>>;
}
```

- The type parameter `P` represents the chain provider type (e.g., `&ChainProvider`, `Arc<SolanaChainProvider>`)
- The `build` method receives a chain provider—implementations typically use a chain-specific trait like `SolanaChainProviderLike` to access provider methods
- The optional `config` allows scheme-specific configuration (parse however you wish, see "Configure in JSON" section)

### X402SchemeBlueprint

A combined trait that requires both `X402SchemeId` and `X402SchemeFacilitatorBuilder`. This is automatically implemented for any type that implements both traits:

```rust
pub trait X402SchemeBlueprint<P>:
    X402SchemeId + for<'a> X402SchemeFacilitatorBuilder<&'a P>
{
}
impl<T, P> X402SchemeBlueprint<P> for T where
    T: X402SchemeId + for<'a> X402SchemeFacilitatorBuilder<&'a P>
{
}
```

The type parameter `P` represents the chain provider type that the blueprint can work with.

### X402SchemeFacilitator

Three core operations every scheme facilitator must implement:

```rust
#[async_trait::async_trait]
pub trait X402SchemeFacilitator: Send + Sync {
    async fn verify(&self, request: &proto::VerifyRequest)
        -> Result<proto::VerifyResponse, X402SchemeFacilitatorError>;
    async fn settle(&self, request: &proto::SettleRequest)
        -> Result<proto::SettleResponse, X402SchemeFacilitatorError>;
    async fn supported(&self)
        -> Result<proto::SupportedResponse, X402SchemeFacilitatorError>;
}
```

| Method      | Purpose                                            |
|-------------|----------------------------------------------------|
| `verify`    | Validate a payment without executing it.           |
| `settle`    | Execute the payment on-chain.                      |
| `supported` | Advertise what payment kinds this scheme supports. |

### SchemeHandlerSlug

At runtime, handlers are identified by a slug combining chain ID, version, and scheme name:

```rust
pub struct SchemeHandlerSlug {
    pub chain_id: ChainId,
    pub x402_version: u8,
    pub name: String,
}
```

This allows the same scheme blueprint to create different handlers for different chains.

## Step-by-Step Guide

### Step 1: Define Types

Use proto generics. For v2 schemes:

```rust
// In crates/chains/x402-chain-solana/src/v2_solana_myscheme/types.rs
use x402_types::proto::v2;

pub type PaymentRequirements = v2::PaymentRequirements<MyScheme, MyAmountType, MyAddressType, MyExtra>;
pub type PaymentPayload = v2::PaymentPayload<PaymentRequirements, MyPayload>;
pub type VerifyRequest = v2::VerifyRequest<PaymentPayload, PaymentRequirements>;
pub type SettleRequest = VerifyRequest;
```

### Step 2: Implement X402SchemeId

```rust
// In crates/chains/x402-chain-solana/src/v2_solana_myscheme/mod.rs
use x402_types::scheme::X402SchemeId;

pub struct V2SolanaMyscheme;

impl X402SchemeId for V2SolanaMyscheme {
    // x402_version() defaults to 2, no need to override

    fn namespace(&self) -> &str {
        "solana"
    }

    fn scheme(&self) -> &str {
        "myscheme"
    }
}
```

### Step 3: Implement X402SchemeFacilitatorBuilder

In the chain-specific crate, implement the builder for the chain-specific provider type:

```rust
// In crates/chains/x402-chain-solana/src/v2_solana_myscheme/facilitator.rs
use crate::chain::provider::SolanaChainProviderLike;
use x402_types::chain::ChainProviderOps;
use x402_types::scheme::X402SchemeFacilitator;

impl<P> X402SchemeFacilitatorBuilder<P> for V2SolanaMyscheme
where
    P: SolanaChainProviderLike + ChainProviderOps + Send + Sync + 'static,
{
    fn build(
        &self,
        provider: P,
        config: Option<serde_json::Value>,
    ) -> Result<Box<dyn X402SchemeFacilitator>, Box<dyn Error>>
    {
        // Optionally parse config here
        let config = config
            .map(serde_json::from_value::<V2SolanaMyschemeFacilitatorConfig>)
            .transpose()?
            .unwrap_or_default();

        Ok(Box::new(V2SolanaMyschemeFacilitator::new(provider, config)))
    }
}
```

Then, in the facilitator crate, implement the adapter for the generic `ChainProvider` enum:

```rust
// In facilitator/src/schemes.rs
#[cfg(feature = "chain-solana")]
use x402_chain_solana::V2SolanaMyscheme;

#[cfg(feature = "chain-solana")]
impl X402SchemeFacilitatorBuilder<&ChainProvider> for V2SolanaMyscheme {
    fn build(
        &self,
        provider: &ChainProvider,
        config: Option<serde_json::Value>,
    ) -> Result<Box<dyn X402SchemeFacilitator>, Box<dyn std::error::Error>> {
        let solana_provider = if let ChainProvider::Solana(provider) = provider {
            Arc::clone(provider)
        } else {
            return Err("V2SolanaMyscheme::build: provider must be a SolanaChainProvider".into());
        };
        self.build(solana_provider, config)
    }
}
```

### Step 4: Implement Facilitator

```rust
// In crates/chains/x402-chain-solana/src/v2_solana_myscheme/facilitator.rs
use crate::chain::provider::SolanaChainProviderLike;
use x402_types::chain::ChainProviderOps;
use x402_types::proto;
use x402_types::proto::v2;
use x402_types::scheme::{
    X402SchemeFacilitator, X402SchemeFacilitatorError,
};

/// Configuration for V2 Solana Myscheme facilitator
#[derive(Debug, Clone, serde::Deserialize, serde::Serialize)]
#[serde(default)]
pub struct V2SolanaMyschemeFacilitatorConfig {
    // Add your scheme-specific configuration fields here
}

impl Default for V2SolanaMyschemeFacilitatorConfig {
    fn default() -> Self {
        Self {
            // Set default values for your configuration
        }
    }
}

pub struct V2SolanaMyschemeFacilitator<P> {
    provider: P,
    config: V2SolanaMyschemeFacilitatorConfig,
}

impl<P> V2SolanaMyschemeFacilitator<P> {
    pub fn new(provider: P, config: V2SolanaMyschemeFacilitatorConfig) -> Self {
        Self { provider, config }
    }
}

#[async_trait::async_trait]
impl<P> X402SchemeFacilitator for V2SolanaMyschemeFacilitator<P>
where
    P: SolanaChainProviderLike + ChainProviderOps + Send + Sync,
{
    async fn verify(&self, request: &proto::VerifyRequest)
        -> Result<proto::VerifyResponse, X402SchemeFacilitatorError>
    {
        let request = types::VerifyRequest::from_proto(request.clone())?;
        // Your verification logic...
        Ok(proto::v2::VerifyResponse::valid(payer.to_string()).into())
    }

    async fn settle(&self, request: &proto::SettleRequest)
        -> Result<proto::SettleResponse, X402SchemeFacilitatorError>
    {
        // Your settlement logic...
        Ok(proto::v2::SettleResponse::Success { payer, transaction, network }.into())
    }

    async fn supported(&self) -> Result<proto::SupportedResponse, X402SchemeFacilitatorError> {
        let chain_id = self.provider.chain_id();
        let kinds = vec![proto::SupportedPaymentKind {
            x402_version: proto::v2::X402Version2.into(),
            scheme: "myscheme".to_string(),
            network: chain_id.to_string(),
            extra: None,
        }];
        let signers = {
            let mut signers = HashMap::with_capacity(1);
            signers.insert(chain_id, self.provider.signer_addresses());
            signers
        };
        Ok(proto::SupportedResponse {
            kinds,
            extensions: Vec::new(),
            signers,
        })
    }
}
```

### Step 5: Register the Scheme

For custom facilitators, register dynamically in the facilitator crate:

```rust,ignore
// In facilitator/src/schemes.rs
#[cfg(feature = "chain-solana")]
use x402_chain_solana::V2SolanaMyscheme;

// Then in your initialization code:
let blueprints = SchemeBlueprints::new().and_register(V2SolanaMyscheme);
```

### Step 6: Configure in JSON

```json
{
  "schemes": [
    {
      "enabled": true,
      "id": "v2-solana-myscheme",
      "chains": "solana:*",
      "config": { "yourOption": "value" }
    }
  ]
}
```

- `id`: The scheme blueprint ID (matches `X402SchemeId::id()`)
- `chains`: Pattern matching (`*` for all, `{a,b}` for specific chain references)
- `config`: Passed to your `build()` method

## Per-Chain Custom Handlers

A powerful feature of the scheme system is the ability to have **different handlers for the same scheme on different chains**. This is useful when:

- A specific chain requires custom logic (e.g., different gas handling, chain-specific optimizations)
- You want to override the default behavior for a particular chain
- You need chain-specific configuration

### How It Works

1. **Create a custom scheme blueprint** that extends or modifies the base scheme behavior
2. **Register it with a unique ID** (e.g., `v1-eip155-exact-custom`)
3. **Enable it for specific chains** in your config

### Example: Custom Handler for a Specific Chain

Suppose you want `eip155:3` to use custom logic while all other EVM chains use the standard `v1-eip155-exact`:

**Step 1: Create the custom scheme**

```rust
// In crates/chains/x402-chain-eip155/src/v1_eip155_exact_custom/mod.rs
use x402_types::scheme::X402SchemeId;

pub struct V1Eip155ExactCustom;

impl X402SchemeId for V1Eip155ExactCustom {
    fn x402_version(&self) -> u8 {
        1
    }

    fn namespace(&self) -> &str {
        "eip155"
    }

    fn scheme(&self) -> &str {
        "exact"  // Same scheme name - will handle "exact" payments
    }

    // Override the default ID to distinguish from the standard scheme
    fn id(&self) -> String {
        "v1-eip155-exact-custom".to_string()
    }
}

// In crates/chains/x402-chain-eip155/src/v1_eip155_exact_custom/facilitator.rs
use crate::chain::provider::Eip155ChainProviderLike;
use x402_types::chain::ChainProviderOps;
use x402_types::scheme::X402SchemeFacilitator;

impl<P> X402SchemeFacilitatorBuilder<P> for V1Eip155ExactCustom
where
    P: Eip155ChainProviderLike + ChainProviderOps + Send + Sync + 'static,
{
    fn build(&self, provider: P, config: Option<serde_json::Value>)
        -> Result<Box<dyn X402SchemeFacilitator>, Box<dyn Error>>
    {
        // Your custom facilitator with chain-specific logic
        Ok(Box::new(V1Eip155ExactCustomFacilitator::new(provider, config)))
    }
}

// In facilitator/src/schemes.rs
#[cfg(feature = "chain-eip155")]
use x402_chain_eip155::V1Eip155ExactCustom;

#[cfg(feature = "chain-eip155")]
impl X402SchemeFacilitatorBuilder<&ChainProvider> for V1Eip155ExactCustom {
    fn build(&self, provider: &ChainProvider, config: Option<serde_json::Value>)
        -> Result<Box<dyn X402SchemeFacilitator>, Box<dyn std::error::Error>>
    {
        let eip155_provider = if let ChainProvider::Eip155(provider) = provider {
            Arc::clone(provider)
        } else {
            return Err("V1Eip155ExactCustom::build: provider must be an Eip155ChainProvider".into());
        };
        self.build(eip155_provider, config)
    }
}
```

**Step 2: Register both schemes**

```rust,ignore
// In facilitator/src/schemes.rs
#[cfg(feature = "chain-eip155")]
use x402_chain_eip155::{V1Eip155Exact, V1Eip155ExactCustom};

// Then in your initialization code:
let blueprints = SchemeBlueprints::new()
    .and_register(V1Eip155Exact)        // Standard handler
    .and_register(V1Eip155ExactCustom); // Custom handler
```

**Step 3: Configure in JSON**

```json,ignore
{
  "chains": {
    "eip155:1": { ... },
    "eip155:3": { ... },
    "eip155:8453": { ... }
  },
  "schemes": [
    {
      "id": "v1-eip155-exact",
      "chains": "eip155:*"
    },
    {
      "id": "v1-eip155-exact-custom",
      "chains": "eip155:3",
      "config": { "customOption": "value" }
    }
  ]
}
```

### Key Points

- The **scheme name** (returned by `scheme()`) determines which payment requests the handler processes
- The **ID** (returned by `id()`) is used to match config entries to blueprints
- Multiple blueprints can have the same `scheme()` but different `id()` values
- The `chains` pattern in config determines which chain(s) each blueprint instance handles
- Each config entry creates a separate handler instance for matching chains

### Chain Pattern Matching

The `chains` field supports several patterns:

| Pattern | Matches |
|---------|---------|
| `eip155:84532` | Exact chain ID |
| `eip155:*` | All EVM chains |
| `solana:*` | All Solana chains |
| `eip155:{1,8453}` | Specific chain references |

## Contributing to Upstream x402-rs

If you want your scheme included in the default x402-rs distribution:

1. Create module structure under `crates/chains/x402-chain-{namespace}/src/v{version}_{namespace}_{scheme}/`
2. Add module declaration in the chain crate's `lib.rs`:
    ```rust
    pub mod v2_solana_myscheme;
    ```
3. Add the scheme to the chain crate's `Cargo.toml` features if needed
4. Register in the facilitator crate's `schemes.rs`:
    ```rust,ignore
    #[cfg(feature = "chain-solana")]
    use x402_chain_solana::V2SolanaMyscheme;

    // Then in your initialization code:
    let blueprints = SchemeBlueprints::new().and_register(V2SolanaMyscheme);
    ```

## Checklist for creating a new scheme

- [ ] Create module structure under `crates/chains/x402-chain-{namespace}/src/v{version}_{namespace}_{scheme}/`
- [ ] Declare a struct for your new scheme, for example, `V2SolanaMyscheme`
- [ ] Declare a struct for the scheme facilitator, for example, `V2SolanaMyschemeFacilitator<P>`
- [ ] Implement `X402SchemeId` for `V2SolanaMyscheme` with correct `x402_version()`, `namespace()`, and `scheme()`
- [ ] Optionally override `id()` for custom scheme variants
- [ ] Implement `X402SchemeFacilitatorBuilder<P>` for `V2SolanaMyscheme` with `build()` method in the chain crate
- [ ] Implement `X402SchemeFacilitatorBuilder<&ChainProvider>` for `V2SolanaMyscheme` in the facilitator crate
- [ ] Implement `X402SchemeFacilitator` (verify/settle/supported) for `V2SolanaMyschemeFacilitator<P>`
- [ ] Define concrete types for the scheme using proto v2 generics
- [ ] Add module declaration in the chain crate's `lib.rs`
- [ ] Register in `SchemeBlueprints` in the facilitator crate
- [ ] Configure in `config.json` with appropriate `id` and `chains` pattern, and update `config.json.example`
