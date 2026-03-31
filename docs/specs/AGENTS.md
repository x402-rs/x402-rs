# AGENTS.md

This folder contains canonical x402 protocol specifications downloaded from the upstream [coinbase/x402](https://github.com/coinbase/x402) repository.

## Folder Structure

```
docs/specs/
├── CONTRIBUTING.md           # Guidelines for proposing new specs
├── README.md                 # Overview of the specs folder
├── scheme_template.md        # Template for new scheme overviews
├── scheme_impl_template.md   # Template for chain-specific implementations
├── transport_template.md     # Template for new transport specifications
├── extensions/               # x402 protocol extensions
│   ├── bazaar.md             # Resource discovery and cataloging
│   ├── eip2612_gas_sponsoring.md   # EIP-2612 gasless approval flow
│   ├── erc20_gas_sponsoring.md     # ERC-20 gasless approval flow
│   └── sign-in-with-x.md     # Wallet-based authentication (CAIP-122)
└── schemes/
    └── exact/                # "exact" payment scheme
        ├── scheme_exact.md           # Scheme overview
        ├── scheme_exact_algo.md      # Algorand implementation
        ├── scheme_exact_aptos.md     # Aptos implementation
        ├── scheme_exact_evm.md       # EVM implementation
        ├── scheme_exact_stellar.md   # Stellar implementation
        └── scheme_exact_sui.md       # Sui implementation
```

## What This Folder Contains

This folder contains **canonical specification documents** that define the x402 payment protocol. These are not local documentation—they are authoritative specifications that describe:

- **Schemes**: How funds are transferred (e.g., `exact` scheme transfers a specific amount)
- **Transports**: How x402 messages are transmitted (HTTP, MCP, A2A)
- **Extensions**: Optional features that extend the base protocol (gas sponsoring, authentication, discovery)
- **Network Implementations**: Chain-specific details for each scheme (EVM, Solana, Aptos, etc.)

## How to Update These Docs

When the upstream x402 repository updates its specs, you'll need to refresh these documents. Here's how:

### Step 1: Identify Updated Files

Check the upstream [specs folder](https://github.com/coinbase/x402/tree/main/specs) for changes. Look for:
- Modified files in `specs/`
- New files added to `specs/schemes/` or `specs/extensions/`
- Deleted files that may need cleanup

### Step 2: Download Updated Files

For each file that needs updating:

1. **Get the raw content** from `https://raw.githubusercontent.com/coinbase/x402/main/specs/<path-to-file>`
2. **Add frontmatter** at the top of the file (see format below)
3. **Save** to the corresponding path in `docs/specs/`

### Frontmatter Format

Every downloaded spec file must include this frontmatter with specific document type:

```markdown
---
Document Type: <Scheme Specification | Scheme Implementation | Extension Specification | Template | Overview | Contributing Guide>
Description: <Brief description of what this document contains>
Source: https://github.com/coinbase/x402/blob/main/specs/<path>
Downloaded At: YYYY-MM-DD
---
```

**Frontmatter Rules:**
1. **Document Type** MUST accurately reflect what the document is:
   - `Scheme Specification` - High-level scheme overview (e.g., `scheme_exact.md`)
   - `Scheme Implementation` - Chain-specific implementation details (e.g., `scheme_exact_evm.md`)
   - `Extension Specification` - Protocol extension documents (e.g., `bazaar.md`, `sign-in-with-x.md`)
   - `Template` - Templates for creating new documents (e.g., `scheme_template.md`)
   - `Overview` - General overview documents (e.g., `README.md`)
   - `Contributing Guide` - Contribution guidelines (e.g., `CONTRIBUTING.md`)
2. **Description** MUST briefly summarize what the document covers
3. **Source** MUST be the full GitHub URL to the original file
4. **Downloaded At** MUST be updated to the current date when modifying

**Important**: Replace `<path>` with the actual path in the upstream repo (e.g., `schemes/exact/scheme_exact_evm.md`).

### Step 3: Update the Date

Change the `Downloaded At` date to the current date when you update any file.

### Step 4: Verify Paths in Frontmatter

Ensure the frontmatter paths are correct:
- Upstream path: `specs/<path>` (where it comes from in the canonical repo)
- Full URL: `https://github.com/coinbase/x402/blob/main/specs/<path>` (the exact location)

## Adding New Spec Files

When new spec files are added to the upstream repo:

1. **Download** the new file(s) from the raw URL
2. **Add frontmatter** following the format above
3. **Create** the corresponding directory structure in `docs/specs/`
4. **Save** the file with appropriate frontmatter
5. **Update this AGENTS.md** to document any new folders or file patterns

## Handling Deleted Upstream Files

If the upstream repo deletes a spec file:

1. **Check** if the local file is still referenced by other docs
2. **Remove** the local file if it's no longer needed
3. **Update** any references in other documentation

## Verification Checklist

Before marking an update as complete, verify:

- [ ] **Frontmatter is present** on all spec files
- [ ] **Document Type is accurate** - reflects what the document actually is (Scheme Specification, Scheme Implementation, Extension Specification, Template, Overview, or Contributing Guide)
- [ ] **Description is meaningful** - briefly summarizes what the document covers
- [ ] **Source URL is correct** - matches upstream location
- [ ] **Downloaded At date is current**
- [ ] File content matches upstream (check a few sections)
- [ ] Directory structure mirrors upstream `specs/` folder
- [ ] No stale files from previous versions remain

## Related Documentation

- [Upstream x402 Repository](https://github.com/coinbase/x402)
- [Upstream Specs Folder](https://github.com/coinbase/x402/tree/main/specs)
- [x402 Protocol Website](https://x402.org)
- [Contributing Guide](./CONTRIBUTING.md) - Guidelines for proposing new specs upstream

## Notes for AI Assistants

- These files are **canonical specs**, not local documentation. Do not modify their technical content unless you're also updating the upstream repo.
- If you need to add local notes or context, add them **after** the frontmatter, not within it.
- When updating, always use the raw GitHub content URL, not the rendered HTML page.
- The `docs/specs/` folder in this repo corresponds to the `specs/` folder in the upstream coinbase/x402 repository.
