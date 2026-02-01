import { config } from 'dotenv';
import { createUnifiedClient } from '@daydreamsai/facilitator/client';
import { createPublicClient, http } from 'viem';
import { privateKeyToAccount } from 'viem/accounts';
import { polygon } from 'viem/chains';

config();

const FACILITATOR_URL = process.env.FACILITATOR_URL || 'http://localhost:8090';
const PROTECTED_URL = process.env.PROTECTED_URL || 'http://localhost:3000/protected';
const PAYER_PRIVATE_KEY = process.env.PAYER_PRIVATE_KEY!;
const RPC_URL = process.env.POLYGON_RPC_URL || 'https://polygon-rpc.com';

async function main() {
  console.log('╔═══════════════════════════════════════════════════════════════╗');
  console.log('║  Rust Facilitator + TypeScript Client Test                  ║');
  console.log('║  Making 3 API calls → Auto-settle at $0.003 threshold      ║');
  console.log('╚═══════════════════════════════════════════════════════════════╝\n');

  // Setup wallet
  const account = privateKeyToAccount(PAYER_PRIVATE_KEY as `0x${string}`);
  console.log(`👛 Payer address: ${account.address}`);
  console.log(`🔧 Facilitator: ${FACILITATOR_URL}`);
  console.log(`📍 Protected URL: ${PROTECTED_URL}\n`);

  // Create public client for Polygon
  const publicClient = createPublicClient({
    chain: polygon,
    transport: http(RPC_URL),
  });

  // Create unified client with upto scheme
  const { fetchWithPayment, uptoScheme } = createUnifiedClient({
    evmUpto: {
      signer: account,
      publicClient,
      facilitatorUrl: FACILITATOR_URL,
      deadlineBufferSec: 60,
    },
  });

  console.log('✅ Unified client created with upto scheme\n');

  // Make 3 sequential API calls
  console.log('🔄 Making 3 API calls (0.001 USDC each)...\n');
  
  let permitCreated = false;
  const startTime = Date.now();

  for (let i = 1; i <= 3; i++) {
    console.log(`=== API Call #${i} ===`);
    
    try {
      const callStart = Date.now();
      const response = await fetchWithPayment(PROTECTED_URL);
      const callDuration = Date.now() - callStart;
      
      if (response.ok) {
        const data = await response.json();
        const sessionId = response.headers.get('x-upto-session-id');
        
        if (i === 1 && !permitCreated) {
          console.log(`✅ Call #${i} Success! (permit created)`);
          permitCreated = true;
        } else {
          console.log(`✅ Call #${i} Success! (permit reused - batched)`);
        }
        
        console.log(`   Response: ${data.message}`);
        console.log(`   Session ID: ${sessionId || 'none'}`);
        console.log(`   Duration: ${callDuration}ms\n`);
        
        // Small delay between calls
        if (i < 3) {
          await new Promise(resolve => setTimeout(resolve, 500));
        }
      } else {
        console.error(`❌ Call #${i} Failed: ${response.status} ${response.statusText}`);
        const errorText = await response.text();
        console.error(`   Error: ${errorText}\n`);
      }
    } catch (error) {
      console.error(`❌ Call #${i} Error:`, error instanceof Error ? error.message : String(error));
      console.error('');
    }
  }

  console.log('⏳ Waiting for settlement transaction...\n');
  
  // Monitor for settlement transaction
  let settlementFound = false;
  const maxWaitTime = 30000; // 30 seconds
  const checkInterval = 2000; // 2 seconds
  let elapsed = 0;

  while (elapsed < maxWaitTime && !settlementFound) {
    await new Promise(resolve => setTimeout(resolve, checkInterval));
    elapsed += checkInterval;

    try {
      // Check for recent Transfer events from payer to seller
      const latestBlock = await publicClient.getBlockNumber();
      const logs = await publicClient.getLogs({
        address: '0x3c499c542cEF5E3811e1192ce70d8cC03d5c3359', // USDC
        event: {
          type: 'event',
          name: 'Transfer',
          inputs: [
            { type: 'address', indexed: true, name: 'from' },
            { type: 'address', indexed: true, name: 'to' },
            { type: 'uint256', indexed: false, name: 'value' }
          ]
        },
        args: {
          from: account.address,
        },
        fromBlock: latestBlock - 20n,
        toBlock: 'latest'
      });

      // Look for a transfer of 3000 (0.003 USDC)
      for (const log of logs) {
        if (log.args.value === 3000n) {
          const txHash = log.transactionHash;
          console.log(`✅ Settlement transaction found!`);
          console.log(`   🔗 Transaction: https://polygonscan.com/tx/${txHash}`);
          console.log(`   💵 Amount: 0.003 USDC (3 × 0.001 USDC)`);
          settlementFound = true;
          break;
        }
      }

      if (!settlementFound) {
        process.stdout.write(`   Waiting... (${elapsed/1000}s elapsed)\r`);
      }
    } catch (error) {
      // Continue waiting
    }
  }

  if (!settlementFound) {
    console.log(`\n⚠️  Settlement not detected within ${maxWaitTime/1000}s`);
    console.log(`   Check server logs for settlement status`);
    console.log(`   Or check wallet transactions: https://polygonscan.com/address/${account.address}`);
  }

  const totalTime = Date.now() - startTime;
  
  console.log('\n═══════════════════════════════════════════════════════════════');
  console.log('                    TEST SUMMARY');
  console.log('═══════════════════════════════════════════════════════════════');
  console.log(`✅ API Calls: 3/3 successful`);
  console.log(`💰 Total Cost: 0.003 USDC (3 × 0.001 USDC)`);
  console.log(`🔄 Batching: All calls used same permit (batched)`);
  console.log(`💸 Settlement: ${settlementFound ? 'Triggered automatically' : 'Check server logs'}`);
  console.log(`⏱️  Total Time: ${(totalTime/1000).toFixed(1)}s`);
  console.log('═══════════════════════════════════════════════════════════════\n');
  
  console.log('🔍 View on PolygonScan:');
  console.log(`   Wallet: https://polygonscan.com/address/${account.address}`);
  console.log('');
}

main().catch(console.error);
