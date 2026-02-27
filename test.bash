#!/usr/bin/env bash
set -uo pipefail

RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
CYAN='\033[0;36m'
BOLD='\033[1m'
RESET='\033[0m'

passed=0
failed=0
failures=()

run_step() {
    local name="$1"
    shift
    echo ""
    echo -e "${CYAN}━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━${RESET}"
    echo -e "${BOLD}  $name${RESET}"
    echo -e "${CYAN}━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━${RESET}"
    echo ""
    if "$@"; then
        echo -e "${GREEN}✓ $name${RESET}"
        passed=$((passed + 1))
    else
        echo -e "${RED}✗ $name${RESET}"
        failed=$((failed + 1))
        failures+=("$name")
    fi
}

echo -e "${BOLD}rs-vst-host — Full Test Suite${RESET}"
echo "Running all standard tests, Miri (Tree Borrows), and Miri (Stacked Borrows)..."

# ── 1. Standard unit tests ──────────────────────────────────────────────────

run_step "cargo test --lib (533 tests)" \
    cargo test --lib

# ── 2. Clippy lint check ────────────────────────────────────────────────────

run_step "cargo clippy (lint check)" \
    cargo clippy --lib

# ── 3. Miri — Tree Borrows (all compatible modules) ────────────────────────

run_step "Miri — Tree Borrows (109 tests)" \
    env MIRIFLAGS="-Zmiri-tree-borrows" \
    cargo +nightly miri test --lib -- \
        "vst3::event_list" "vst3::param_changes" "vst3::process" \
        "vst3::types" "midi::translate" "miri_tests"

# ── 4. Miri — Stacked Borrows (strict, excludes ProcessBuffers) ────────────

run_step "Miri — Stacked Borrows (70 tests)" \
    cargo +nightly miri test --lib -- \
        "vst3::event_list" "vst3::param_changes" \
        "vst3::types" "midi::translate" \
        "miri_tests::tests::miri_event" \
        "miri_tests::tests::miri_com" \
        "miri_tests::tests::miri_null" \
        "miri_tests::tests::miri_param"

# ── Summary ─────────────────────────────────────────────────────────────────

echo ""
echo -e "${CYAN}━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━${RESET}"
echo -e "${BOLD}  Summary${RESET}"
echo -e "${CYAN}━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━${RESET}"
echo ""
echo -e "  ${GREEN}Passed: $passed${RESET}"
echo -e "  ${RED}Failed: $failed${RESET}"

if [[ ${#failures[@]} -gt 0 ]]; then
    echo ""
    echo -e "  ${RED}Failed steps:${RESET}"
    for f in "${failures[@]}"; do
        echo -e "    ${RED}• $f${RESET}"
    done
    echo ""
    exit 1
else
    echo ""
    echo -e "  ${GREEN}All steps passed.${RESET}"
    echo ""
    exit 0
fi
