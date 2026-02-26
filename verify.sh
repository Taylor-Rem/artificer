#!/bin/bash

echo "═══════════════════════════════════════════"
echo "  ARTIFICER VERIFICATION SCRIPT"
echo "═══════════════════════════════════════════"
echo

# Color codes
RED='\033[0;31m'
GREEN='\033[0;32m'
NC='\033[0m'

check_passed=0
check_failed=0

check() {
    local label="$1"
    local result="$2"
    if [ "$result" -eq 0 ]; then
        echo -e "${GREEN}✓${NC} $label"
        ((check_passed++))
    else
        echo -e "${RED}✗${NC} $label"
        ((check_failed++))
    fi
}

# Prerequisites
echo "→ Checking prerequisites..."
rustc --version > /dev/null 2>&1; check "Rust compiler installed" $?
cargo --version > /dev/null 2>&1; check "Cargo installed" $?

# hardware.json
if [ -f "hardware.json" ]; then
    echo -e "${GREEN}✓${NC} hardware.json exists"
    ((check_passed++))
else
    echo -e "${RED}✗${NC} hardware.json missing — create it in the workspace root"
    ((check_failed++))
fi

# Compilation
echo
echo "→ Checking compilation..."
cargo check --workspace --quiet 2>&1 | grep -q "^error"
# grep exits 1 when nothing found (no errors = success), so invert
compile_ok=$([ $? -eq 1 ] && echo 0 || echo 1)
check "Workspace compiles" $compile_ok

# Directory structure
echo
echo "→ Checking project structure..."
dirs=(
    "crates/engine/src/agent"
    "crates/engine/src/api"
    "crates/engine/src/background"
    "crates/envoy/src"
    "crates/shared/src/db"
    "crates/shared/src/tools"
)
all_dirs_ok=0
for dir in "${dirs[@]}"; do
    if [ ! -d "$dir" ]; then
        echo -e "${RED}✗${NC} Missing directory: $dir"
        all_dirs_ok=1
    fi
done
check "Project structure valid" $all_dirs_ok

# Key files
echo
echo "→ Checking key files..."
files=(
    "crates/engine/src/agent/execution.rs"
    "crates/engine/src/agent/tool_execution.rs"
    "crates/engine/src/agent/tool_validation.rs"
    "crates/shared/src/db/mod.rs"
    "crates/shared/src/tools/mod.rs"
    "README.md"
    ".env.example"
)
all_files_ok=0
for file in "${files[@]}"; do
    if [ ! -f "$file" ]; then
        echo -e "${RED}✗${NC} Missing: $file"
        all_files_ok=1
    fi
done
check "Key files present" $all_files_ok

# Summary
echo
echo "═══════════════════════════════════════════"
echo "  VERIFICATION SUMMARY"
echo "═══════════════════════════════════════════"
echo -e "${GREEN}Passed: $check_passed${NC}"
echo -e "${RED}Failed: $check_failed${NC}"
echo

if [ $check_failed -eq 0 ]; then
    echo -e "${GREEN}✓ All checks passed! Ready to run.${NC}"
    echo
    echo "Next steps:"
    echo "  1. cargo run --bin artificer"
    echo "  2. cargo run --bin envoy"
    exit 0
else
    echo -e "${RED}✗ Some checks failed. Review errors above.${NC}"
    exit 1
fi
