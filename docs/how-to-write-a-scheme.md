# How to Write a Scheme for x402-rs

This guide explains how to create a custom payment scheme for the x402-rs facilitator.

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
pub trait X402SchemeFacilitatorBuilder {
    /// Build a facilitator instance for a specific chain
    fn build(
        &self,
        provider: ChainProvider,
        config: Option<serde_json::Value>,
    ) -> Result<Box<dyn X402SchemeFacilitator>, Box<dyn std::error::Error>>;
}
```

- The `build` method receives a `ChainProvider` enum—match against the expected variant
- The optional `config` allows scheme-specific configuration (parse however you wish, see "Configure in JSON" section)

### X402SchemeBlueprint

A combined trait that requires both `X402SchemeId` and `X402SchemeFacilitatorBuilder`. This is automatically implemented for any type that implements both traits:

```rust
pub trait X402SchemeBlueprint: X402SchemeId + X402SchemeFacilitatorBuilder {}
impl<T> X402SchemeBlueprint for T where T: X402SchemeId + X402SchemeFacilitatorBuilder {}
```

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
use crate::proto::v2;

pub type PaymentRequirements = v2::PaymentRequirements<MyScheme, MyAmountType, MyAddressType, MyExtra>;
pub type PaymentPayload = v2::PaymentPayload<PaymentRequirements, MyPayload>;
pub type VerifyRequest = v2::VerifyRequest<PaymentPayload, PaymentRequirements>;
pub type SettleRequest = VerifyRequest;
```

### Step 2: Implement X402SchemeId

```rust
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

```rust
impl X402SchemeFacilitatorBuilder for V2SolanaMyscheme {
    fn build(&self, provider: ChainProvider, config: Option<serde_json::Value>)
        -> Result<Box<dyn X402SchemeFacilitator>, Box<dyn Error>>
    {
        let provider = match provider {
            ChainProvider::Solana(p) => p,
            _ => return Err("Requires SolanaChainProvider".into()),
        };
        // Optionally parse config here
        Ok(Box::new(V2SolanaMyschemeFacilitator { provider }))
    }
}
```

### Step 4: Implement Facilitator

```rust
pub struct V2SolanaMyschemeFacilitator {
    provider: Arc<SolanaChainProvider>,
}

#[async_trait::async_trait]
impl X402SchemeFacilitator for V2SolanaMyschemeFacilitator {
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
            x402_version: proto::X402Version::v2().into(),
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

For custom facilitators, register dynamically:
```rust,ignore
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

impl X402SchemeFacilitatorBuilder for V1Eip155ExactCustom {
    fn build(&self, provider: ChainProvider, config: Option<serde_json::Value>)
        -> Result<Box<dyn X402SchemeFacilitator>, Box<dyn Error>>
    {
        let provider = match provider {
            ChainProvider::Eip155(p) => p,
            _ => return Err("Requires Eip155ChainProvider".into()),
        };
        // Your custom facilitator with chain-specific logic
        Ok(Box::new(V1Eip155ExactCustomFacilitator { provider, config }))
    }
}
```

**Step 2: Register both schemes**

```rust,ignore
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

1. Create module structure under `src/scheme/v2_solana_myscheme/`
2. Add module declaration in `src/scheme/mod.rs`:
   ```rust
   pub mod v2_solana_myscheme;
   ```
3. Register in `SchemeBlueprints::full()`:
   ```rust,ignore
   .and_register(V2SolanaMyscheme)
   ```

## Checklist for creating a new scheme

- [ ] Declare a struct for your new scheme, for example, `V2SolanaMyscheme`
- [ ] Declare a struct for the scheme facilitator, for example, `V2SolanaMyschemeFacilitator`
- [ ] Implement `X402SchemeId` for `V2SolanaMyscheme` with correct `x402_version()`, `namespace()`, and `scheme()`
- [ ] Optionally override `id()` for custom scheme variants
- [ ] Implement `X402SchemeFacilitatorBuilder` for `V2SolanaMyscheme` with `build()` method
- [ ] Implement `X402SchemeFacilitator` (verify/settle/supported) for `V2SolanaMyschemeFacilitator`
- [ ] Define concrete types for the scheme using proto v2 generics
- [ ] Register in `SchemeBlueprints`
- [ ] Configure in `config.json` with appropriate `id` and `chains` pattern, and update `config.json.example`
