So, this doc is a thinking process for how to do x402 requests.

So, when do x402 request, you could get 402 status code or other. On "other" - you do a pass through.
On 402 - we should parse the requirements, select an applicable requirement, sign the payment and then retry with the
passed payment.

We have two x402 versions. V1: The requirements are in the response body.

```json
{
  "error": "X-PAYMENT header is required",
  "accepts": [
    {
      "scheme": "exact",
      "network": "base-sepolia",
      "maxAmountRequired": "1000",
      "resource": "http://localhost:3001/protected-route",
      "description": "Access to premium content",
      "mimeType": "application/json",
      "payTo": "0xfa3F54AE9C4287CA09a486dfaFaCe7d1d4095d93",
      "maxTimeoutSeconds": 300,
      "asset": "0x036CbD53842c5426634e7929541eC2318f3dCF7e",
      "outputSchema": {
        "input": {
          "type": "http",
          "method": "GET",
          "discoverable": true
        }
      },
      "extra": {
        "name": "USDC",
        "version": "2"
      }
    }
  ],
  "x402Version": 1
}
```

For v2 protocol, the payment requirements are in the header `payment-required`, encoded as base64.

```
payment-required: eyJ4NDAyVmVyc2lvbiI6MiwiZXJyb3IiOiJQYXltZW50IHJlcXVpcmVkIiwicmVzb3VyY2UiOnsidXJsIjoiaHR0cDovL2xvY2FsaG9zdDozMDAwL3Byb3RlY3RlZC1yb3V0ZSIsImRlc2NyaXB0aW9uIjoiQWNjZXNzIHRvIHByZW1pdW0gY29udGVudCIsIm1pbWVUeXBlIjoiIn0sImFjY2VwdHMiOlt7InNjaGVtZSI6ImV4YWN0IiwibmV0d29yayI6ImVpcDE1NTo4NDUzMiIsImFtb3VudCI6IjEwMDAwMCIsImFzc2V0IjoiMHgwMzZDYkQ1Mzg0MmM1NDI2NjM0ZTc5Mjk1NDFlQzIzMThmM2RDRjdlIiwicGF5VG8iOiIweGZhM0Y1NEFFOUM0Mjg3Q0EwOWE0ODZkZmFGYUNlN2QxZDQwOTVkOTMiLCJtYXhUaW1lb3V0U2Vjb25kcyI6MzAwLCJleHRyYSI6eyJuYW1lIjoiVVNEQyIsInZlcnNpb24iOiIyIn19XX0=
```

which is decoded to:

```json
{
  "x402Version": 2,
  "error": "Payment required",
  "resource": {
    "url": "http://localhost:3000/protected-route",
    "description": "Access to premium content",
    "mimeType": ""
  },
  "accepts": [
    {
      "scheme": "exact",
      "network": "eip155:84532",
      "amount": "100000",
      "asset": "0x036CbD53842c5426634e7929541eC2318f3dCF7e",
      "payTo": "0xfa3F54AE9C4287CA09a486dfaFaCe7d1d4095d93",
      "maxTimeoutSeconds": 300,
      "extra": {
        "name": "USDC",
        "version": "2"
      }
    }
  ]
}
```

On the requestor side we should be able to register schemes. One scheme handles a combination of protocol scheme, chain namespace, and scheme per se.
Like, "v1-eip155-exact" is for protocol v1, any eip155 chain, and "exact" scheme.

We can do trait X402SchemeClient with common methods for all schemes. Not sure what methods we'd need though. Each scheme client is individually instantiated,
probably using chain-namespace-specific wallets. We then register the schemes on X402Client instance. For starters, we could use just Vec<Box<dyn X402SchemeClient>>.

Let's focus on v2 first. I'd rather avoid cloning full 402 response. We can pass `&[u8]` to each X402SchemeClient.

One aspect to be dealt with later is how to select the best/most appropriate proposed payment (the one in accepts array). We could prefer chain, and we could prefer asset on chain, or something else via hook. What hook? Have no idea yet.

For funsies, let's parse payment proposal for V2Eip155Exact scheme. We'd need something like.

```rust
pub struct PaymentProposalV2<A> {
  pub x402_version: v2::X402Version2,
  pub resource: ResourceInfo,
  pub accepts: Vec<A>
}

pub type V2Eip155ExactPaymentProposal = PaymentProposalV2<PaymentRequirements>;
```

Here `PaymentRequirements` come from v2_eip155_exact/types.rs.

If inside the X402SchemeClient, this works fully all right. But, what if we have two X402SchemeClient instances in our disposal?

What kind of hook would we need to provide to select the best payment proposal? What could be its API?

---

## Selection Design (Hybrid Approach)

After discussion, we settled on a **hybrid approach**: first-match by default, with optional preferences/scoring override.

### Selection Flow

```
402 Response → Parse accepts[] → For each entry:
                                   → Ask each SchemeClient: "can you handle this?"
                                   → If yes, get a "Candidate" with metadata
                                 → Apply selector (default: first match, or custom)
                                 → Winner signs payment
```

### PaymentCandidate - Common Intermediate Type

We need a common struct that normalizes proposal data for selection purposes:

```rust
pub struct PaymentCandidate {
    pub chain_id: ChainId,
    pub asset: String,        // normalized address
    pub amount: U256,
    pub scheme: String,
    pub x402_version: u8,
    // Reference back to which scheme client + raw proposal
    pub(crate) client_index: usize,
    pub(crate) raw_proposal: serde_json::Value,
}
```

This allows selection logic to compare across different scheme types (EVM vs Solana, V1 vs V2) using common fields.

### PaymentSelector Trait

```rust
pub trait PaymentSelector {
    fn select<'a>(&self, candidates: &'a [PaymentCandidate]) -> Option<&'a PaymentCandidate>;
}

// Default implementation: first match wins
pub struct FirstMatch;
impl PaymentSelector for FirstMatch {
    fn select<'a>(&self, candidates: &'a [PaymentCandidate]) -> Option<&'a PaymentCandidate> {
        candidates.first()
    }
}

// Prefer specific chain
pub struct PreferChain(pub ChainId);
impl PaymentSelector for PreferChain {
    fn select<'a>(&self, candidates: &'a [PaymentCandidate]) -> Option<&'a PaymentCandidate> {
        candidates.iter().find(|c| c.chain_id == self.0)
            .or_else(|| candidates.first())
    }
}

// Prefer specific asset on specific chain
pub struct PreferAsset {
    pub chain_id: ChainId,
    pub asset: String,
}

// Max amount filter
pub struct MaxAmount(pub U256);
impl PaymentSelector for MaxAmount {
    fn select<'a>(&self, candidates: &'a [PaymentCandidate]) -> Option<&'a PaymentCandidate> {
        candidates.iter().find(|c| c.amount <= self.0)
    }
}
```

### Usage Example

```rust
// Simple: first match wins (default)
let x402_client = X402Client::new()
    .register(V2Eip155ExactClient::new(evm_signer))
    .register(V2SolanaExactClient::new(solana_keypair));

// With preferences: prefer Base Sepolia
let x402_client = X402Client::new()
    .register(V2Eip155ExactClient::new(evm_signer))
    .register(V2SolanaExactClient::new(solana_keypair))
    .with_selector(PreferChain(ChainId::BaseSepolia));

// With max amount guard
let x402_client = X402Client::new()
    .register(V2Eip155ExactClient::new(evm_signer))
    .with_selector(MaxAmount(U256::from(1_000_000))); // max 1 USDC
```

### Design Decisions

1. **Composable selectors?** No - keep it simple. One selector per client.
2. **Async selection?** Not now - sync only. Balance checks can be a future enhancement.
3. **Error handling?** Return middleware error when no candidate matches.

---

## X402SchemeClient Trait Design

Now let's define what each scheme client needs to do:

```rust
pub trait X402SchemeClient: Send + Sync {
    /// Check if this client can handle the given payment proposal.
    /// Called for each entry in the accepts array.
    fn can_handle(&self, version: u8, scheme: &str, network: &str) -> bool;
    
    /// Parse the raw accepts entry and extract common fields for selection.
    /// Only called if can_handle returned true.
    fn to_candidate(
        &self,
        raw: &serde_json::Value,
        client_index: usize,
    ) -> Result<PaymentCandidate, X402Error>;
    
    /// Sign the payment for the selected candidate.
    /// Returns the value for the X-Payment header.
    async fn sign_payment(
        &self,
        candidate: &PaymentCandidate,
    ) -> Result<String, X402Error>;
}
```

### Implementation for V2Eip155ExactClient

```rust
impl<S: Signer + Send + Sync> X402SchemeClient for V2Eip155ExactClient<S> {
    fn can_handle(&self, version: u8, scheme: &str, network: &str) -> bool {
        version == 2
            && scheme == "exact"
            && network.starts_with("eip155:")
    }
    
    fn to_candidate(
        &self,
        raw: &serde_json::Value,
        client_index: usize,
    ) -> Result<PaymentCandidate, X402Error> {
        // Parse into scheme-specific type first
        let req: v2_eip155_exact::PaymentRequirements =
            serde_json::from_value(raw.clone())?;
        
        Ok(PaymentCandidate {
            chain_id: req.network.clone(),
            asset: req.asset.to_string(),
            amount: req.amount,
            scheme: "exact".into(),
            x402_version: 2,
            client_index,
            raw_proposal: raw.clone(),
        })
    }
    
    async fn sign_payment(
        &self,
        candidate: &PaymentCandidate,
    ) -> Result<String, X402Error> {
        // Re-parse to get full typed requirements
        let req: v2_eip155_exact::PaymentRequirements =
            serde_json::from_value(candidate.raw_proposal.clone())?;
        
        // Build ERC-3009 authorization
        let authorization = build_authorization(&req, &self.signer)?;
        
        // Sign it
        let signature = self.signer.sign_typed_data(&authorization).await?;
        
        // Build PaymentPayload
        let payload = v2::PaymentPayload {
            x402_version: v2::X402Version2,
            accepted: req,
            resource: /* from context */,
            payload: ExactEvmPayload { signature, authorization },
        };
        
        // Encode as base64 for header
        let json = serde_json::to_vec(&payload)?;
        Ok(base64::encode(&json))
    }
}
```

### The Full Flow in Middleware

```rust
impl Middleware for X402Client {
    async fn handle(&self, req: Request, ext: &mut Extensions, next: Next<'_>) -> Result<Response> {
        let retry_req = req.try_clone();
        let res = next.clone().run(req, ext).await?;
        
        if res.status() != StatusCode::PAYMENT_REQUIRED {
            return Ok(res);
        }
        
        // 1. Parse payment requirements (V1 from body, V2 from header)
        let (version, accepts, resource) = parse_402_response(&res)?;
        
        // 2. Build candidates from all scheme clients
        let mut candidates = Vec::new();
        for (idx, raw) in accepts.iter().enumerate() {
            let scheme = raw.get("scheme").and_then(|v| v.as_str()).unwrap_or("");
            let network = raw.get("network").and_then(|v| v.as_str()).unwrap_or("");
            
            for (client_idx, client) in self.schemes.iter().enumerate() {
                if client.can_handle(version, scheme, network) {
                    if let Ok(candidate) = client.to_candidate(raw, client_idx) {
                        candidates.push(candidate);
                        break; // First matching client wins for this entry
                    }
                }
            }
        }
        
        // 3. Select best candidate
        let selected = self.selector.select(&candidates)
            .ok_or(X402Error::NoMatchingPaymentOption)?;
        
        // 4. Sign payment
        let client = &self.schemes[selected.client_index];
        let payment_header = client.sign_payment(selected).await?;
        
        // 5. Retry with payment
        let mut retry = retry_req.ok_or(X402Error::RequestNotCloneable)?;
        retry.headers_mut().insert("X-Payment", payment_header.parse()?);
        
        next.run(retry, ext).await
    }
}
```
