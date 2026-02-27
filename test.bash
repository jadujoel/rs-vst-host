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
echo "Running standard tests, Clippy, Miri (Tree Borrows + Stacked Borrows), and ASan..."

# ── 1. Standard unit tests ──────────────────────────────────────────────────

run_step "cargo test --lib (579 tests)" \
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

# ── 5. AddressSanitizer (all tests, skipping ASan-incompatible ones) ───────
#
# ASan instruments the compiled native code and catches real hardware-level
# memory errors: use-after-free, double-free, heap/stack buffer overflow,
# memory leaks, and allocator mismatches.
#
# Tests using libc::raise (signal sandbox) or malloc_zone_check are skipped
# because ASan's signal and malloc zone interception conflicts with them.

run_step "ASan — AddressSanitizer (564 tests)" \
    env RUSTFLAGS="-Z sanitizer=address" \
    cargo +nightly test --target aarch64-apple-darwin --lib -- \
        --skip test_heap_check_returns_true_in_clean_process \
        --skip test_sandbox_catches_raised_sigbus \
        --skip test_sandbox_catches_sigsegv \
        --skip test_sandbox_recovery_allows_subsequent_calls \
        --skip test_sandbox_catches_sigabrt \
        --skip test_sandbox_multiple_crashes_same_signal \
        --skip test_sandbox_alternating_crash_and_normal \
        --skip test_sandbox_crash_produces_backtrace \
        --skip test_clean_recovery_has_no_heap_corruption \
        --skip test_sandbox_crash_recovery_in_instance_context \
        --skip test_sandbox_catches_abort_during_cleanup \
        --skip test_last_drop_crashed_set_on_sandbox_crash \
        --skip test_crash_flags_set_together_on_com_crash \
        --skip test_module_drop_skips_unload_after_instance_crash \
        --skip test_check_heap_after_recovery_clean

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
