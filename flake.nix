{
  description = "rust dev shell";

  inputs = {
    nixpkgs.url      = "github:NixOS/nixpkgs/nixos-unstable";
    rust-overlay.url = "github:oxalica/rust-overlay";
    flake-utils.url  = "github:numtide/flake-utils";
  };

  outputs = { self, nixpkgs, rust-overlay, flake-utils, ... }:
    flake-utils.lib.eachDefaultSystem (system:
      let
        overlays = [ (import rust-overlay) ];
        pkgs = import nixpkgs {
          inherit system overlays;
        };
      in
      with pkgs;
      {
        devShells.default = mkShell {
          nativeBuildInputs = [
            openssl
            pkg-config
            rust-bin.nightly.latest.default
            rust-analyzer
          ];
          LIBCLANG_PATH = "${pkgs.libclang.lib}/lib";
        };

        packages = 
        let
          test-network = writeShellScriptBin "test-network" ''
          set -e

          NUM_NODES=''${1:-5}
          TEST_DURATION=''${2:-30}
          SCRIPT_DIR="$(pwd)"
          LOG_DIR="''${SCRIPT_DIR}/test_logs"

          echo "🚀 Starting gossip discovery test with ''${NUM_NODES} nodes for ''${TEST_DURATION} seconds"
          echo "📁 Logs will be saved to: ''${LOG_DIR}"

          # Clean up any existing logs and create log directory
          rm -rf "''${LOG_DIR}"
          mkdir -p "''${LOG_DIR}"

          # Array to store process IDs
          declare -a PIDS=()
          declare -a NODE_IDS=()

          # Function to cleanup all processes on exit
          cleanup() {
              echo ""
              echo "🛑 Cleaning up processes..."
              for pid in "''${PIDS[@]}"; do
                  if kill -0 "$pid" 2>/dev/null; then
                      kill "$pid" 2>/dev/null || true
                      wait "$pid" 2>/dev/null || true
                  fi
              done
              echo "✅ All processes terminated"
          }

          # Set up cleanup on script exit
          trap cleanup EXIT INT TERM

          # Function to extract node ID from log output
          extract_node_id() {
              local log_file="$1"
              local timeout=10
              local count=0
              
              while [ $count -lt $timeout ]; do
                  if [ -f "$log_file" ]; then
                      # Look for the node ID in the startup message (tracing format or fallback)
                      # Strip ANSI codes first, then extract node_id
                      local node_id=$(sed 's/\[[0-9;]*m//g' "$log_file" 2>/dev/null | grep "node_id" | grep -o "[a-z0-9]\{64\}" | head -1)
                      if [ -z "$node_id" ]; then
                          # Fallback to old format
                          local node_id=$(grep -o "started with ID: [a-z0-9]*" "$log_file" 2>/dev/null | head -1 | cut -d' ' -f4)
                      fi
                      if [ -n "$node_id" ]; then
                          echo "$node_id"
                          return 0
                      fi
                  fi
                  sleep 0.5
                  ((count++))
              done
              return 1
          }

          echo "🌱 Starting seed node (node_0)..."
          RUST_LOG=info nix develop --command cargo run --example address_book_demo node_0 > "''${LOG_DIR}/node_0.log" 2>&1 &
          SEED_PID=$!
          PIDS+=($SEED_PID)

          # Give the seed node time to start and extract its ID
          echo "⏳ Waiting for seed node to initialize..."
          sleep 3

          SEED_NODE_ID=$(extract_node_id "''${LOG_DIR}/node_0.log")
          if [ -z "$SEED_NODE_ID" ]; then
              echo "❌ Failed to extract seed node ID from log"
              exit 1
          fi

          echo "✅ Seed node started with ID: ''${SEED_NODE_ID}"
          NODE_IDS+=("node_0:$SEED_NODE_ID")

          # Start remaining nodes, connecting them to the seed
          for ((i=1; i<NUM_NODES; i++)); do
              echo "🔗 Starting node_''${i} (connecting to seed: ''${SEED_NODE_ID})..."
              RUST_LOG=info nix develop --command cargo run --example address_book_demo "node_''${i}" "$SEED_NODE_ID" > "''${LOG_DIR}/node_''${i}.log" 2>&1 &
              NODE_PID=$!
              PIDS+=($NODE_PID)
              
              # Extract this node's ID
              sleep 2
              NODE_ID=$(extract_node_id "''${LOG_DIR}/node_''${i}.log")
              if [ -n "$NODE_ID" ]; then
                  NODE_IDS+=("node_''${i}:$NODE_ID")
                  echo "✅ node_''${i} started with ID: ''${NODE_ID}"
              else
                  echo "⚠️  Warning: Could not extract ID for node_''${i}"
                  NODE_IDS+=("node_''${i}:unknown")
              fi
              
              # Small delay between starting nodes
              sleep 1
          done

          echo ""
          echo "🕒 All ''${NUM_NODES} nodes started. Running test for ''${TEST_DURATION} seconds..."
          echo "📊 Monitor progress in real-time with: tail -f ''${LOG_DIR}/node_*.log"
          echo ""

          # Show node IDs
          echo "🔍 Node IDs:"
          for node_info in "''${NODE_IDS[@]}"; do
              echo "   ''${node_info}"
          done
          echo ""

          # Wait for the test duration
          sleep "$TEST_DURATION"

          echo "📈 Test completed! Analyzing final address book states..."
          echo ""

          # Analyze the final state of each node's address book
          declare -A ADDRESS_BOOKS=()
          declare -A PEER_LISTS=()

          for ((i=0; i<NUM_NODES; i++)); do
              log_file="''${LOG_DIR}/node_''${i}.log"
              if [ -f "$log_file" ]; then
                  # Look for tracing output with "Discovered new peer" messages
                  discovered_peers=$(sed 's/\[[0-9;]*m//g' "$log_file" 2>/dev/null | grep "Discovered new peer" | wc -l)
                  
                  if [ "$discovered_peers" -gt 0 ]; then
                      # Extract peer information from tracing logs (strip ANSI codes first)
                      peer_list=$(sed 's/\[[0-9;]*m//g' "$log_file" | grep "Discovered new peer" | sed 's/.*name=\([^ ]*\) .*/\1/' | tr '\n' ', ' | sed 's/, $//')
                      
                      ADDRESS_BOOKS["node_''${i}"]="$discovered_peers"
                      PEER_LISTS["node_''${i}"]="$peer_list"
                      
                      echo "📚 node_''${i}: ''${discovered_peers} peers discovered"
                      if [ -n "$peer_list" ] && [ "$peer_list" != "" ]; then
                          echo "   Peers: $peer_list"
                      else
                          echo "   Peers: (peer details not available)"
                      fi
                  else
                      # Fallback: look for old format address book updates
                      last_update=$(grep "Address Book updated" "$log_file" | tail -1)
                      if [ -n "$last_update" ]; then
                          # Extract the number of peers
                          peer_count=$(echo "$last_update" | grep -o '[0-9]* peers' | cut -d' ' -f1)
                          ADDRESS_BOOKS["node_''${i}"]="$peer_count"
                          
                          # Try to extract peer list from old format
                          awk '
                              /📚 Address Book Update:/ { in_section=1; peers=""; next }
                              in_section && /👥 Discovered peers/ { in_peers=1; next }
                              in_section && in_peers && /^[[:space:]]*•/ { 
                                  gsub(/^[[:space:]]*•[[:space:]]*/, "");
                                  gsub(/\\/, "");
                                  if (peers) peers = peers ", " $0; else peers = $0
                              }
                              in_section && (/🚀/ || /📚 Address Book Update:/ && NR > 1) { 
                                  if (peers) print peers; 
                                  in_section=0; in_peers=0; peers="" 
                              }
                              END { if (peers) print peers }
                          ' "$log_file" | tail -1 > /tmp/peers_''${i}.txt
                          
                          peer_list=$(cat /tmp/peers_''${i}.txt 2>/dev/null || echo "")
                          PEER_LISTS["node_''${i}"]="$peer_list"
                          
                          echo "📚 node_''${i}: ''${peer_count} peers discovered"
                          if [ -n "$peer_list" ] && [ "$peer_list" != "" ]; then
                              echo "   Peers: $peer_list"
                          else
                              echo "   Peers: (none listed)"
                          fi
                          rm -f /tmp/peers_''${i}.txt
                      else
                          ADDRESS_BOOKS["node_''${i}"]="0"
                          PEER_LISTS["node_''${i}"]=""
                          echo "📚 node_''${i}: No address book updates found"
                      fi
                  fi
              else
                  echo "❌ Log file for node_''${i} not found"
                  ADDRESS_BOOKS["node_''${i}"]="unknown"
                  PEER_LISTS["node_''${i}"]=""
              fi
          done

          echo ""

          # Check if all nodes have converged to the same address book size
          declare -a UNIQUE_COUNTS=()
          for count in "''${ADDRESS_BOOKS[@]}"; do
              if [[ ! " ''${UNIQUE_COUNTS[@]} " =~ " ''${count} " ]]; then
                  UNIQUE_COUNTS+=("$count")
              fi
          done

          if [ ''${#UNIQUE_COUNTS[@]} -eq 1 ]; then
              expected_peers=$((NUM_NODES - 1))  # Each node should see all others except itself
              actual_peers=''${UNIQUE_COUNTS[0]}
              
              if [ "$actual_peers" -eq "$expected_peers" ]; then
                  echo "✅ SUCCESS: All nodes converged to the same address book!"
                  echo "   Each node discovered ''${actual_peers} peers (expected: ''${expected_peers})"
              else
                  echo "⚠️  PARTIAL SUCCESS: All nodes have the same address book size (''${actual_peers})"
                  echo "   But expected ''${expected_peers} peers. Some nodes may not be fully connected."
              fi
          else
              echo "❌ CONVERGENCE FAILED: Nodes have different address book sizes:"
              for ((i=0; i<NUM_NODES; i++)); do
                  echo "   node_''${i}: ''${ADDRESS_BOOKS["node_''${i}"]} peers"
              done
          fi

          echo ""
          echo "📋 DETAILED ADDRESS BOOK COMPARISON:"
          echo "═══════════════════════════════════════"
          
          # Create a detailed comparison showing each node's view
          for ((i=0; i<NUM_NODES; i++)); do
              echo ""
              echo "🏷️  node_''${i} address book:"
              peer_count=''${ADDRESS_BOOKS["node_''${i}"]}
              peer_list=''${PEER_LISTS["node_''${i}"]}
              
              if [ "$peer_count" != "unknown" ] && [ "$peer_count" != "0" ]; then
                  echo "   📊 Count: $peer_count peers"
                  if [ -n "$peer_list" ] && [ "$peer_list" != "" ]; then
                      echo "   👥 Peers: $peer_list"
                  else
                      echo "   👥 Peers: (detailed list not available)"
                  fi
              else
                  echo "   📊 Count: $peer_count"
                  echo "   👥 Peers: (none or unknown)"
              fi
          done
          
          echo ""
          echo "═══════════════════════════════════════"
          echo ""
          echo "🔍 For detailed analysis, check the log files in: ''${LOG_DIR}/"
          echo "💡 Try increasing the test duration if convergence failed"
          echo ""
          echo "📊 Summary of test:"
          echo "   - Nodes spawned: ''${NUM_NODES}"
          echo "   - Test duration: ''${TEST_DURATION} seconds"
          echo "   - Unique address book sizes: ''${#UNIQUE_COUNTS[@]}"
          echo "   - Address book sizes: ''${UNIQUE_COUNTS[*]}"
        '';
        in
        {
          inherit test-network;
          default = test-network;
        };
      }
    );
}
