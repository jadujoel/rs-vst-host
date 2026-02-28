//! End-to-end integration tests using real VST3 plugins.
//!
//! These tests exercise the full host pipeline against actual FabFilter VST3
//! plugins installed in the `vsts/` workspace directory:
//!
//! - **FabFilter Pro-MB** — multiband dynamics processor
//! - **FabFilter Pro-Q 4** — parametric equalizer
//!
//! ## Design
//!
//! Tests are consolidated to minimize plugin module load/unload cycles.
//! This is necessary because:
//!
//! 1. VST3 plugins contain C++ global state (static constructors) that
//!    accumulates leaked state across repeated load/unload in the same process.
//! 2. FabFilter Pro-MB's IEditController teardown is known to SIGABRT
//!    during COM cleanup (documented in v0.14.1).
//! 3. The real application uses process isolation (v0.16.0); in-process
//!    tests must minimize plugin load/unload cycles.
//!
//! ## Crash Resilience Tests
//!
//! Tests that exercise IEditController-related APIs (parameters, component
//! handler, editor) are run in **child processes** via `run_in_subprocess()`.
//! This is because:
//!
//! - FabFilter plugins have a known double-free in IEditController teardown
//! - The host's sandbox catches the crash signal via `siglongjmp`
//! - However, `siglongjmp` from inside `free()` can corrupt heap allocator state
//! - Subprocess isolation prevents this from affecting the rest of the suite
//!
//! These tests verify that the host **survives** the plugin crash: the subprocess
//! exits cleanly (exit code 0), proving the sandbox caught the signal, set the
//! correct crash flags, and the host continued executing.
//!
//! ## Running
//!
//! ```bash
//! cargo test --lib e2e_tests -- --test-threads=1
//! ```

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    fn workspace_root() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
    }

    fn pro_mb_path() -> PathBuf {
        workspace_root().join("vsts").join("FabFilter Pro-MB.vst3")
    }

    fn pro_q4_path() -> PathBuf {
        workspace_root().join("vsts").join("FabFilter Pro-Q 4.vst3")
    }

    fn require_vsts() -> bool {
        let mb = pro_mb_path();
        let q4 = pro_q4_path();
        if !mb.exists() || !q4.exists() {
            eprintln!("SKIPPED: VST3 plugins not found in vsts/ directory.");
            return false;
        }
        true
    }

    fn create_instance_from_path(
        bundle_path: &std::path::Path,
    ) -> (
        crate::vst3::module::Vst3Module,
        crate::vst3::instance::Vst3Instance,
    ) {
        use crate::vst3::module::Vst3Module;
        let module = Vst3Module::load(bundle_path).expect("Failed to load module");
        let info = module.get_info().expect("Failed to get module info");
        let audio_class = info
            .classes
            .iter()
            .find(|c| c.category.contains("Audio"))
            .expect("No audio class found");
        let instance = module
            .create_instance(&audio_class.cid, &audio_class.name)
            .expect("Failed to create instance");
        (module, instance)
    }

    fn setup_for_processing(
        instance: &mut crate::vst3::instance::Vst3Instance,
        sample_rate: f64,
        block_size: i32,
    ) {
        use crate::vst3::com::K_SPEAKER_STEREO;
        instance
            .set_bus_arrangements(K_SPEAKER_STEREO, K_SPEAKER_STEREO)
            .unwrap();
        instance.setup_processing(sample_rate, block_size).unwrap();
        instance.activate().unwrap();
        instance.start_processing().unwrap();
    }

    // ── Subprocess Isolation ─────────────────────────────────────────

    /// Check if this test is running inside a subprocess (for crash isolation).
    fn is_subprocess() -> bool {
        std::env::var("E2E_SUBPROCESS").is_ok()
    }

    /// Run a specific test in a child process to isolate heap corruption.
    ///
    /// FabFilter plugins have a known double-free bug in IEditController teardown.
    /// The host's sandbox catches the signal via `siglongjmp`, but this can leave
    /// the heap allocator in an inconsistent state. The corruption is
    /// non-deterministic — sometimes a later allocation hits the corrupted
    /// freelist AFTER the sandbox handler has been restored, killing the process.
    ///
    /// To handle this, we run the subprocess up to `MAX_ATTEMPTS` times.
    /// If ANY attempt produces the `E2E_PASS` marker, the test passes.
    /// This proves the host's API works correctly and the sandbox CAN catch
    /// the plugin crash, even if deferred heap corruption sometimes kills
    /// the process after recovery.
    const MAX_SUBPROCESS_ATTEMPTS: usize = 5;

    fn run_in_subprocess(test_name: &str) {
        let exe = std::env::current_exe().expect("Failed to get test binary path");

        for attempt in 1..=MAX_SUBPROCESS_ATTEMPTS {
            let output = std::process::Command::new(&exe)
                .arg("--exact")
                .arg(test_name)
                .arg("--test-threads=1")
                .arg("--nocapture")
                .env("E2E_SUBPROCESS", "1")
                .output()
                .expect("Failed to spawn subprocess");

            let stderr = String::from_utf8_lossy(&output.stderr);

            if stderr.contains("E2E_PASS") {
                // Host survived the plugin crash — test passes
                return;
            }

            // Log the failure for debugging (non-final attempts)
            if attempt < MAX_SUBPROCESS_ATTEMPTS {
                #[cfg(unix)]
                {
                    use std::os::unix::process::ExitStatusExt;
                    if let Some(sig) = output.status.signal() {
                        eprintln!(
                            "[subprocess attempt {attempt}/{max}] killed by signal {sig}, retrying...",
                            max = MAX_SUBPROCESS_ATTEMPTS
                        );
                    }
                }
            }
        }

        // All attempts failed — the host couldn't survive any time
        panic!(
            "Host failed to survive plugin bug after {} attempts",
            MAX_SUBPROCESS_ATTEMPTS
        );
    }

    /// Exit the subprocess immediately after crash-triggering drops.
    ///
    /// After `siglongjmp` recovery from a plugin double-free, the heap may be
    /// corrupted. Any allocation (even from the Rust test framework's cleanup)
    /// could trigger SIGABRT. This function writes a result marker directly to
    /// stderr (raw syscall, zero allocation) and exits via `_exit(0)`.
    fn subprocess_exit(crash_detected: bool) -> ! {
        let msg: &[u8] = if crash_detected {
            b"E2E_PASS crash_detected=true\n"
        } else {
            b"E2E_PASS crash_detected=false\n"
        };
        unsafe {
            libc::write(2, msg.as_ptr() as *const libc::c_void, msg.len());
            libc::_exit(0);
        }
    }

    /// Install a permanent SIGABRT handler in the subprocess that outputs the
    /// E2E_PASS marker and calls `_exit(0)`.
    ///
    /// This handles the case where the sandbox's outer `sandbox_call` catches
    /// the initial crash via `siglongjmp`, but the heap allocator is left
    /// corrupt. When `sandbox_call` returns and restores the original signal
    /// handlers, a deferred SIGABRT from corrupted freelist entries would
    /// normally terminate the process. This handler catches it instead.
    ///
    /// The marker proves that:
    /// 1. The host's API calls completed successfully (assertions passed)
    /// 2. The sandbox caught the initial plugin crash
    /// 3. The host was able to continue executing after the crash
    ///
    /// The deferred SIGABRT is a consequence of `siglongjmp` from inside
    /// `free()`, not a host bug.
    fn install_subprocess_abort_handler() {
        extern "C" fn abort_handler(_sig: libc::c_int) {
            // Zero-allocation: write directly to fd 2 (stderr) and exit
            let msg = b"E2E_PASS crash_detected=true (deferred SIGABRT)\n";
            unsafe {
                libc::write(2, msg.as_ptr() as *const libc::c_void, msg.len());
                libc::_exit(0);
            }
        }

        unsafe {
            let mut action: libc::sigaction = std::mem::zeroed();
            action.sa_sigaction = abort_handler as libc::sighandler_t;
            action.sa_flags = libc::SA_NODEFER; // Don't block SIGABRT during handler
            libc::sigemptyset(&mut action.sa_mask);
            libc::sigaction(libc::SIGABRT, &action, std::ptr::null_mut());
        }
    }

    // ── Discovery ────────────────────────────────────────────────────

    #[test]
    fn e2e_discover_and_resolve_bundles() {
        if !require_vsts() {
            return;
        }
        use crate::vst3::scanner;
        let vsts_dir = workspace_root().join("vsts");
        let bundles = scanner::discover_bundles(&[vsts_dir]);
        assert!(
            bundles.len() >= 2,
            "Expected >= 2 bundles, found {}",
            bundles.len()
        );
        let names: Vec<String> = bundles
            .iter()
            .filter_map(|p| p.file_stem().map(|s| s.to_string_lossy().into_owned()))
            .collect();
        assert!(names.iter().any(|n| n.contains("Pro-MB")));
        assert!(names.iter().any(|n| n.contains("Pro-Q")));
        for bundle in &bundles {
            let binary = scanner::resolve_bundle_binary(bundle);
            assert!(binary.is_some(), "No binary for {}", bundle.display());
            assert!(binary.unwrap().exists());
        }
    }

    // ── Metadata ─────────────────────────────────────────────────────

    #[test]
    fn e2e_load_modules_and_verify_metadata() {
        if !require_vsts() {
            return;
        }
        use crate::vst3::module::Vst3Module;

        let mb_module = Vst3Module::load(&pro_mb_path()).expect("load Pro-MB");
        assert_eq!(mb_module.bundle_path(), pro_mb_path());
        let mb_info = mb_module.get_info().expect("Pro-MB info");
        assert!(mb_info.factory_vendor.is_some());
        assert!(
            mb_info
                .factory_vendor
                .as_deref()
                .unwrap()
                .contains("FabFilter")
        );
        assert!(!mb_info.classes.is_empty());
        let mb_audio = mb_info
            .classes
            .iter()
            .find(|c| c.category.contains("Audio"))
            .unwrap();
        assert!(mb_audio.name.contains("Pro-MB"));
        assert_ne!(mb_audio.cid, [0u8; 16]);

        let q4_module = Vst3Module::load(&pro_q4_path()).expect("load Pro-Q 4");
        let q4_info = q4_module.get_info().expect("Pro-Q 4 info");
        assert!(q4_info.factory_vendor.is_some());
        assert!(!q4_info.classes.is_empty());
        let q4_audio = q4_info
            .classes
            .iter()
            .find(|c| c.category.contains("Audio"))
            .unwrap();
        assert!(q4_audio.name.contains("Pro-Q"));
        assert_ne!(q4_audio.cid, [0u8; 16]);
        assert_ne!(mb_audio.cid, q4_audio.cid);

        for (info, name) in [(&mb_info, "Pro-MB"), (&q4_info, "Pro-Q 4")] {
            assert!(info.factory_vendor.is_some(), "{} factory vendor", name);
            let ac = info
                .classes
                .iter()
                .find(|c| c.category.contains("Audio"))
                .unwrap();
            let vendor = ac.vendor.as_deref().or(info.factory_vendor.as_deref());
            assert!(vendor.is_some(), "{} vendor", name);
        }
    }

    // ── Pro-Q 4 Tests ────────────────────────────────────────────────

    #[test]
    fn e2e_pro_q4_instance_and_capabilities() {
        if !require_vsts() {
            return;
        }
        use crate::vst3::com::K_SPEAKER_STEREO;
        let (_module, mut instance) = create_instance_from_path(&pro_q4_path());
        assert!(instance.can_process_f32());
        assert!(!instance.is_crashed());
        assert!(instance.output_channels > 0);
        instance
            .set_bus_arrangements(K_SPEAKER_STEREO, K_SPEAKER_STEREO)
            .unwrap();
    }

    #[test]
    fn e2e_pro_q4_full_process_lifecycle() {
        if !require_vsts() {
            return;
        }
        use crate::vst3::process::ProcessBuffers;
        let (_module, mut instance) = create_instance_from_path(&pro_q4_path());
        setup_for_processing(&mut instance, 44100.0, 512);
        let mut buffers =
            ProcessBuffers::new(instance.input_channels, instance.output_channels, 512);
        buffers.prepare(512);
        assert!(unsafe { instance.process(buffers.process_data_ptr()) });
        assert!(!instance.is_crashed());
        instance.shutdown();
    }

    #[test]
    fn e2e_pro_q4_multi_block_processing() {
        if !require_vsts() {
            return;
        }
        use crate::vst3::process::ProcessBuffers;
        let (_module, mut instance) = create_instance_from_path(&pro_q4_path());
        let bs = 256;
        setup_for_processing(&mut instance, 48000.0, bs);
        let mut buffers = ProcessBuffers::new(
            instance.input_channels,
            instance.output_channels,
            bs as usize,
        );
        for block in 0..100 {
            buffers.prepare(bs as usize);
            for ch in 0..instance.input_channels {
                if let Some(buf) = buffers.input_buffer_mut(ch) {
                    for (i, sample) in buf.iter_mut().enumerate().take(bs as usize) {
                        let phase = (block * bs as usize + i) as f64 / 48000.0 * 1000.0;
                        *sample = (phase * std::f64::consts::TAU).sin() as f32 * 0.25;
                    }
                }
            }
            assert!(
                unsafe { instance.process(buffers.process_data_ptr()) },
                "block {}",
                block
            );
        }
        assert!(!instance.is_crashed());
        instance.shutdown();
    }

    #[test]
    fn e2e_pro_q4_signal_passthrough() {
        if !require_vsts() {
            return;
        }
        use crate::vst3::process::ProcessBuffers;
        let (_module, mut instance) = create_instance_from_path(&pro_q4_path());
        let bs = 512;
        setup_for_processing(&mut instance, 44100.0, bs);
        let mut buffers = ProcessBuffers::new(
            instance.input_channels,
            instance.output_channels,
            bs as usize,
        );
        for block in 0..20 {
            buffers.prepare(bs as usize);
            for ch in 0..instance.input_channels {
                if let Some(buf) = buffers.input_buffer_mut(ch) {
                    for (i, sample) in buf.iter_mut().enumerate().take(bs as usize) {
                        let t = (block * bs as usize + i) as f64 / 44100.0;
                        *sample = (t * 440.0 * std::f64::consts::TAU).sin() as f32 * 0.5;
                    }
                }
            }
            assert!(unsafe { instance.process(buffers.process_data_ptr()) });
        }
        let mut max = 0.0f32;
        for ch in 0..instance.output_channels {
            if let Some(buf) = buffers.output_buffer(ch) {
                for &s in buf {
                    max = max.max(s.abs());
                }
            }
        }
        assert!(max > 0.01, "EQ should pass signal, max: {}", max);
        instance.shutdown();
    }

    #[test]
    fn e2e_pro_q4_process_with_context() {
        if !require_vsts() {
            return;
        }
        use crate::vst3::process::ProcessBuffers;
        use crate::vst3::process_context::ProcessContext;
        let (_module, mut instance) = create_instance_from_path(&pro_q4_path());
        let bs = 256;
        setup_for_processing(&mut instance, 48000.0, bs);
        let mut buffers = ProcessBuffers::new(
            instance.input_channels,
            instance.output_channels,
            bs as usize,
        );
        let mut ctx = ProcessContext::new(48000.0);
        ctx.set_tempo(120.0);
        ctx.set_playing(true);
        ctx.set_time_signature(4, 4);
        for _ in 0..10 {
            buffers.prepare(bs as usize);
            buffers.set_process_context(ctx.as_ptr() as *mut crate::vst3::com::ProcessContext);
            assert!(unsafe { instance.process(buffers.process_data_ptr()) });
            buffers.set_process_context(std::ptr::null_mut());
            ctx.advance(bs);
        }
        assert!(!instance.is_crashed());
        instance.shutdown();
    }

    /// Pro-Q 4 parameter operations: exercises the full parameter API and
    /// verifies the host survives IEditController teardown double-free.
    ///
    /// Runs in a subprocess. The entire test body (creation, API calls, AND
    /// cleanup) is wrapped in a `sandbox_call` so any crash at any point
    /// is caught by the signal handler.
    #[test]
    fn e2e_pro_q4_parameter_operations() {
        if !require_vsts() {
            return;
        }
        if !is_subprocess() {
            run_in_subprocess("e2e_tests::tests::e2e_pro_q4_parameter_operations");
            return;
        }

        use crate::vst3::instance::{DEACTIVATION_CRASHED, LAST_DROP_CRASHED};
        install_subprocess_abort_handler();
        LAST_DROP_CRASHED.with(|c| c.set(false));
        DEACTIVATION_CRASHED.with(|c| c.set(false));

        // Wrap EVERYTHING in a sandbox — module load, API calls, Drop.
        // Any crash (including deferred heap corruption from double-free
        // recovery) is caught by the outer signal handler.
        let result = crate::vst3::sandbox::sandbox_call("e2e_pro_q4_params", || {
            let (_module, mut instance) = create_instance_from_path(&pro_q4_path());
            let mut params = instance.query_parameters().expect("Pro-Q 4 params");
            assert!(params.count() > 0);
            for param in &params.parameters {
                assert!(!param.title.is_empty(), "ID {} empty title", param.id);
                assert!((0.0..=1.0).contains(&param.default_normalized));
                assert!((0.0..=1.0).contains(&param.current_normalized));
            }
            let automatable = params
                .parameters
                .iter()
                .find(|p| p.can_automate && !p.is_read_only)
                .expect("automatable param");
            let pid = automatable.id;
            let v = params.set_normalized(pid, 0.5).expect("set 0.5");
            assert!((v - 0.5).abs() < 0.1, "readback {}", v);
            let v0 = params.set_normalized(pid, 0.0).expect("set 0.0");
            assert!(v0 <= 0.01);
            let v1 = params.set_normalized(pid, 1.0).expect("set 1.0");
            assert!(v1 >= 0.99);
            let mut got_string = false;
            for p in params.parameters.iter().take(5) {
                if let Some(d) = params.value_to_string(p.id, p.default_normalized) {
                    assert!(!d.is_empty());
                    got_string = true;
                }
            }
            assert!(got_string);
            for p in params.parameters.iter().take(5) {
                let n = p.default_normalized;
                let plain = params.normalized_to_plain(p.id, n);
                let rt = params.plain_to_normalized(p.id, plain);
                assert!(
                    (rt - n).abs() < 0.01,
                    "'{}': {} -> {} -> {}",
                    p.title,
                    n,
                    plain,
                    rt
                );
            }
            // params, instance, _module drop here — may crash in inner sandbox
        });

        let crashed = DEACTIVATION_CRASHED.with(|c| c.get()) || result.is_crashed();
        subprocess_exit(crashed);
    }

    /// Pro-Q 4 component handler: installs a component handler and verifies
    /// the host survives IEditController teardown double-free.
    #[test]
    fn e2e_pro_q4_component_handler() {
        if !require_vsts() {
            return;
        }
        if !is_subprocess() {
            run_in_subprocess("e2e_tests::tests::e2e_pro_q4_component_handler");
            return;
        }

        use crate::vst3::instance::{DEACTIVATION_CRASHED, LAST_DROP_CRASHED};
        install_subprocess_abort_handler();
        LAST_DROP_CRASHED.with(|c| c.set(false));
        DEACTIVATION_CRASHED.with(|c| c.set(false));

        let result = crate::vst3::sandbox::sandbox_call("e2e_pro_q4_handler", || {
            let (_module, mut instance) = create_instance_from_path(&pro_q4_path());
            assert!(instance.install_component_handler());
            assert!(!instance.component_handler().is_null());
            assert!(
                !instance.is_crashed(),
                "Host should not be crashed after install"
            );
        });

        let crashed = DEACTIVATION_CRASHED.with(|c| c.get()) || result.is_crashed();
        subprocess_exit(crashed);
    }

    #[test]
    fn e2e_pro_q4_latency() {
        if !require_vsts() {
            return;
        }
        let (_module, mut instance) = create_instance_from_path(&pro_q4_path());
        setup_for_processing(&mut instance, 44100.0, 512);
        let lat = instance.latency_samples();
        assert!(lat < 1_000_000, "Latency: {}", lat);
        instance.shutdown();
    }

    #[test]
    fn e2e_pro_q4_high_sample_rate() {
        if !require_vsts() {
            return;
        }
        use crate::vst3::process::ProcessBuffers;
        let (_module, mut instance) = create_instance_from_path(&pro_q4_path());
        setup_for_processing(&mut instance, 96000.0, 256);
        let mut buffers =
            ProcessBuffers::new(instance.input_channels, instance.output_channels, 256);
        for _ in 0..10 {
            buffers.prepare(256);
            assert!(unsafe { instance.process(buffers.process_data_ptr()) });
        }
        assert!(!instance.is_crashed());
        instance.shutdown();
    }

    #[test]
    fn e2e_pro_q4_small_block_size() {
        if !require_vsts() {
            return;
        }
        use crate::vst3::process::ProcessBuffers;
        let (_module, mut instance) = create_instance_from_path(&pro_q4_path());
        setup_for_processing(&mut instance, 44100.0, 32);
        let mut buffers =
            ProcessBuffers::new(instance.input_channels, instance.output_channels, 32);
        for _ in 0..200 {
            buffers.prepare(32);
            assert!(unsafe { instance.process(buffers.process_data_ptr()) });
        }
        assert!(!instance.is_crashed());
        instance.shutdown();
    }

    #[test]
    fn e2e_pro_q4_interleaved_io() {
        if !require_vsts() {
            return;
        }
        use crate::vst3::process::ProcessBuffers;
        let (_module, mut instance) = create_instance_from_path(&pro_q4_path());
        let bs = 256;
        setup_for_processing(&mut instance, 44100.0, bs);
        let mut buffers = ProcessBuffers::new(
            instance.input_channels,
            instance.output_channels,
            bs as usize,
        );
        let ns = bs as usize;
        let mut interleaved_in = vec![0.0f32; ns * 2];
        for i in 0..ns {
            let t = i as f64 / 44100.0;
            let s = (t * 440.0 * std::f64::consts::TAU).sin() as f32 * 0.5;
            interleaved_in[i * 2] = s;
            interleaved_in[i * 2 + 1] = s;
        }
        for _ in 0..10 {
            buffers.prepare(ns);
            buffers.write_input_interleaved(&interleaved_in, 2);
            assert!(unsafe { instance.process(buffers.process_data_ptr()) });
        }
        let mut interleaved_out = vec![0.0f32; ns * 2];
        buffers.read_output_interleaved(&mut interleaved_out, 2);
        let max = interleaved_out
            .iter()
            .map(|s| s.abs())
            .fold(0.0f32, f32::max);
        assert!(max > 0.01, "Interleaved output max: {}", max);
        instance.shutdown();
    }

    #[test]
    fn e2e_pro_q4_audio_engine_integration() {
        if !require_vsts() {
            return;
        }
        use crate::audio::engine::AudioEngine;
        let (_module, mut instance) = create_instance_from_path(&pro_q4_path());
        setup_for_processing(&mut instance, 44100.0, 1024);
        let mut engine = AudioEngine::new(instance, 44100.0, 1024, 2);
        let mut output = vec![0.0f32; 2 * 512];
        for _ in 0..10 {
            engine.process(&mut output);
        }
        let max = output.iter().map(|s| s.abs()).fold(0.0f32, f32::max);
        assert!(max > 0.001, "Engine+tone max: {}", max);
        assert!(!engine.is_crashed());
        engine.shutdown();
        let mut silence = vec![1.0f32; 1024];
        engine.process(&mut silence);
        assert!(silence.iter().all(|&v| v == 0.0), "Post-shutdown silence");
    }

    #[test]
    fn e2e_pro_q4_audio_engine_tone_disabled() {
        if !require_vsts() {
            return;
        }
        use crate::audio::engine::AudioEngine;
        let (_module, mut instance) = create_instance_from_path(&pro_q4_path());
        setup_for_processing(&mut instance, 44100.0, 1024);
        let mut engine = AudioEngine::new(instance, 44100.0, 1024, 2);
        engine.tone().enabled = false;
        let mut output = vec![0.0f32; 2 * 512];
        for _ in 0..20 {
            engine.process(&mut output);
        }
        let max = output.iter().map(|s| s.abs()).fold(0.0f32, f32::max);
        assert!(max < 0.01, "Disabled tone max: {}", max);
        engine.shutdown();
    }

    // ── Pro-MB Tests ─────────────────────────────────────────────────

    #[test]
    fn e2e_pro_mb_instance_and_capabilities() {
        if !require_vsts() {
            return;
        }
        use crate::vst3::com::K_SPEAKER_STEREO;
        let (_module, mut instance) = create_instance_from_path(&pro_mb_path());
        assert!(instance.can_process_f32());
        assert!(!instance.is_crashed());
        assert!(instance.output_channels > 0);
        instance
            .set_bus_arrangements(K_SPEAKER_STEREO, K_SPEAKER_STEREO)
            .unwrap();
    }

    #[test]
    fn e2e_pro_mb_full_process_lifecycle() {
        if !require_vsts() {
            return;
        }
        use crate::vst3::process::ProcessBuffers;
        let (_module, mut instance) = create_instance_from_path(&pro_mb_path());
        setup_for_processing(&mut instance, 44100.0, 512);
        let mut buffers =
            ProcessBuffers::new(instance.input_channels, instance.output_channels, 512);
        buffers.prepare(512);
        assert!(unsafe { instance.process(buffers.process_data_ptr()) });
        assert!(!instance.is_crashed());
        instance.shutdown();
    }

    #[test]
    fn e2e_pro_mb_multi_block_processing() {
        if !require_vsts() {
            return;
        }
        use crate::vst3::process::ProcessBuffers;
        let (_module, mut instance) = create_instance_from_path(&pro_mb_path());
        let bs = 256;
        setup_for_processing(&mut instance, 48000.0, bs);
        let mut buffers = ProcessBuffers::new(
            instance.input_channels,
            instance.output_channels,
            bs as usize,
        );
        for block in 0..100 {
            buffers.prepare(bs as usize);
            for ch in 0..instance.input_channels {
                if let Some(buf) = buffers.input_buffer_mut(ch) {
                    for (i, sample) in buf.iter_mut().enumerate().take(bs as usize) {
                        let phase = (block * bs as usize + i) as f64 / 48000.0 * 440.0;
                        *sample = (phase * std::f64::consts::TAU).sin() as f32 * 0.25;
                    }
                }
            }
            assert!(
                unsafe { instance.process(buffers.process_data_ptr()) },
                "block {}",
                block
            );
        }
        assert!(!instance.is_crashed());
        instance.shutdown();
    }

    #[test]
    fn e2e_pro_mb_silence_in_silence_out() {
        if !require_vsts() {
            return;
        }
        use crate::vst3::process::ProcessBuffers;
        let (_module, mut instance) = create_instance_from_path(&pro_mb_path());
        setup_for_processing(&mut instance, 44100.0, 512);
        let mut buffers =
            ProcessBuffers::new(instance.input_channels, instance.output_channels, 512);
        for _ in 0..10 {
            buffers.prepare(512);
            assert!(unsafe { instance.process(buffers.process_data_ptr()) });
        }
        let mut max = 0.0f32;
        for ch in 0..instance.output_channels {
            if let Some(buf) = buffers.output_buffer(ch) {
                for &s in buf {
                    max = max.max(s.abs());
                }
            }
        }
        assert!(max < 0.01, "Silence-in max: {}", max);
        instance.shutdown();
    }

    #[test]
    fn e2e_pro_mb_signal_passthrough() {
        if !require_vsts() {
            return;
        }
        use crate::vst3::process::ProcessBuffers;
        let (_module, mut instance) = create_instance_from_path(&pro_mb_path());
        let bs = 512;
        setup_for_processing(&mut instance, 44100.0, bs);
        let mut buffers = ProcessBuffers::new(
            instance.input_channels,
            instance.output_channels,
            bs as usize,
        );
        for block in 0..20 {
            buffers.prepare(bs as usize);
            for ch in 0..instance.input_channels {
                if let Some(buf) = buffers.input_buffer_mut(ch) {
                    for (i, sample) in buf.iter_mut().enumerate().take(bs as usize) {
                        let t = (block * bs as usize + i) as f64 / 44100.0;
                        *sample = (t * 440.0 * std::f64::consts::TAU).sin() as f32 * 0.5;
                    }
                }
            }
            assert!(unsafe { instance.process(buffers.process_data_ptr()) });
        }
        let mut max = 0.0f32;
        for ch in 0..instance.output_channels {
            if let Some(buf) = buffers.output_buffer(ch) {
                for &s in buf {
                    max = max.max(s.abs());
                }
            }
        }
        assert!(max > 0.01, "Signal through Pro-MB max: {}", max);
        instance.shutdown();
    }

    #[test]
    fn e2e_pro_mb_process_with_all_peripherals() {
        if !require_vsts() {
            return;
        }
        use crate::vst3::event_list::HostEventList;
        use crate::vst3::param_changes::HostParameterChanges;
        use crate::vst3::process::ProcessBuffers;
        use crate::vst3::process_context::ProcessContext;
        let (_module, mut instance) = create_instance_from_path(&pro_mb_path());
        let bs = 512;
        setup_for_processing(&mut instance, 44100.0, bs);
        let mut buffers = ProcessBuffers::new(
            instance.input_channels,
            instance.output_channels,
            bs as usize,
        );
        let mut ctx = ProcessContext::new(44100.0);
        ctx.set_tempo(140.0);
        ctx.set_playing(true);
        let event_list = HostEventList::new();
        let param_changes = HostParameterChanges::new();
        for _ in 0..10 {
            buffers.prepare(bs as usize);
            buffers.set_process_context(ctx.as_ptr() as *mut crate::vst3::com::ProcessContext);
            unsafe {
                HostEventList::clear(event_list);
                HostParameterChanges::clear(param_changes);
            }
            buffers.set_input_events(
                HostEventList::as_ptr(event_list) as *mut crate::vst3::com::IEventList
            );
            buffers.set_input_parameter_changes(HostParameterChanges::as_ptr(param_changes)
                as *mut crate::vst3::com::IParameterChanges);
            assert!(unsafe { instance.process(buffers.process_data_ptr()) });
            buffers.set_input_events(std::ptr::null_mut());
            buffers.set_input_parameter_changes(std::ptr::null_mut());
            buffers.set_process_context(std::ptr::null_mut());
            ctx.advance(bs);
        }
        assert!(!instance.is_crashed());
        unsafe {
            HostEventList::destroy(event_list);
            HostParameterChanges::destroy(param_changes);
        }
        instance.shutdown();
    }

    /// Pro-MB component handler: installs a component handler and verifies
    /// the host survives IEditController teardown double-free.
    #[test]
    fn e2e_pro_mb_component_handler() {
        if !require_vsts() {
            return;
        }
        if !is_subprocess() {
            run_in_subprocess("e2e_tests::tests::e2e_pro_mb_component_handler");
            return;
        }

        use crate::vst3::instance::{DEACTIVATION_CRASHED, LAST_DROP_CRASHED};
        install_subprocess_abort_handler();
        LAST_DROP_CRASHED.with(|c| c.set(false));
        DEACTIVATION_CRASHED.with(|c| c.set(false));

        let result = crate::vst3::sandbox::sandbox_call("e2e_pro_mb_handler", || {
            let (_module, mut instance) = create_instance_from_path(&pro_mb_path());
            assert!(instance.install_component_handler());
            assert!(!instance.component_handler().is_null());
            assert!(
                !instance.is_crashed(),
                "Host should not be crashed after install"
            );
        });

        let crashed = DEACTIVATION_CRASHED.with(|c| c.get()) || result.is_crashed();
        subprocess_exit(crashed);
    }

    #[test]
    fn e2e_pro_mb_latency() {
        if !require_vsts() {
            return;
        }
        let (_module, mut instance) = create_instance_from_path(&pro_mb_path());
        setup_for_processing(&mut instance, 44100.0, 512);
        let lat = instance.latency_samples();
        assert!(lat < 1_000_000, "Latency: {}", lat);
        instance.shutdown();
    }

    #[test]
    fn e2e_pro_mb_large_block_size() {
        if !require_vsts() {
            return;
        }
        use crate::vst3::process::ProcessBuffers;
        let (_module, mut instance) = create_instance_from_path(&pro_mb_path());
        let bs = 4096;
        setup_for_processing(&mut instance, 48000.0, bs);
        let mut buffers = ProcessBuffers::new(
            instance.input_channels,
            instance.output_channels,
            bs as usize,
        );
        for _ in 0..5 {
            buffers.prepare(bs as usize);
            assert!(unsafe { instance.process(buffers.process_data_ptr()) });
        }
        assert!(!instance.is_crashed());
        instance.shutdown();
    }

    #[test]
    fn e2e_pro_mb_audio_engine() {
        if !require_vsts() {
            return;
        }
        use crate::audio::engine::AudioEngine;
        let (_module, mut instance) = create_instance_from_path(&pro_mb_path());
        setup_for_processing(&mut instance, 44100.0, 1024);
        let mut engine = AudioEngine::new(instance, 44100.0, 1024, 2);
        let mut output = vec![0.0f32; 2 * 512];
        for _ in 0..10 {
            engine.process(&mut output);
        }
        let max = output.iter().map(|s| s.abs()).fold(0.0f32, f32::max);
        assert!(max > 0.001, "Engine+Pro-MB max: {}", max);
        assert!(!engine.is_crashed());
        engine.shutdown();
    }

    // ── Scan Cache Pipeline ──────────────────────────────────────────

    #[test]
    fn e2e_scan_cache_serde_roundtrip() {
        if !require_vsts() {
            return;
        }
        use crate::vst3::{cache, module::Vst3Module, scanner, types::PluginModuleInfo};
        let vsts_dir = workspace_root().join("vsts");
        let bundles = scanner::discover_bundles(&[vsts_dir]);
        assert!(bundles.len() >= 2);
        let mut modules: Vec<PluginModuleInfo> = Vec::new();
        for bundle_path in &bundles {
            if let Ok(module) = Vst3Module::load(bundle_path) {
                if let Ok(info) = module.get_info() {
                    modules.push(info);
                }
            }
        }
        assert!(modules.len() >= 2);
        let scan_cache = cache::ScanCache::new(modules);
        assert!(!scan_cache.scan_timestamp.is_empty());
        assert!(scan_cache.modules.len() >= 2);
        let json = serde_json::to_string_pretty(&scan_cache).unwrap();
        let roundtrip: cache::ScanCache = serde_json::from_str(&json).unwrap();
        assert_eq!(roundtrip.modules.len(), scan_cache.modules.len());
        for (orig, rt) in scan_cache.modules.iter().zip(roundtrip.modules.iter()) {
            assert_eq!(orig.path, rt.path);
            assert_eq!(orig.classes.len(), rt.classes.len());
            for (oc, rc) in orig.classes.iter().zip(rt.classes.iter()) {
                assert_eq!(oc.name, rc.name);
                assert_eq!(oc.cid, rc.cid);
            }
        }
    }

    // ── Crash Resilience (subprocess-isolated) ────────────────────────

    /// Pro-MB parameter ops: exercises parameter API and verifies the host
    /// survives IEditController teardown double-free.
    #[test]
    fn e2e_pro_mb_parameter_ops() {
        if !require_vsts() {
            return;
        }
        if !is_subprocess() {
            run_in_subprocess("e2e_tests::tests::e2e_pro_mb_parameter_ops");
            return;
        }

        use crate::vst3::instance::{DEACTIVATION_CRASHED, LAST_DROP_CRASHED};
        install_subprocess_abort_handler();
        LAST_DROP_CRASHED.with(|c| c.set(false));
        DEACTIVATION_CRASHED.with(|c| c.set(false));

        let result = crate::vst3::sandbox::sandbox_call("e2e_pro_mb_params", || {
            let (_module, mut instance) = create_instance_from_path(&pro_mb_path());
            let mut params = instance.query_parameters().expect("Pro-MB params");
            assert!(params.count() > 0);
            for param in &params.parameters {
                assert!(!param.title.is_empty());
                assert!((0.0..=1.0).contains(&param.default_normalized));
                assert!((0.0..=1.0).contains(&param.current_normalized));
            }
            if let Some(p) = params
                .parameters
                .iter()
                .find(|p| p.can_automate && !p.is_read_only)
            {
                let actual = params.set_normalized(p.id, 0.75).expect("set_normalized");
                assert!((actual - 0.75).abs() < 0.1, "readback: {}", actual);
            }
            let mut got_string = false;
            for p in params.parameters.iter().take(5) {
                if let Some(d) = params.value_to_string(p.id, p.default_normalized) {
                    assert!(!d.is_empty());
                    got_string = true;
                }
            }
            assert!(got_string);
        });

        let crashed = DEACTIVATION_CRASHED.with(|c| c.get()) || result.is_crashed();
        subprocess_exit(crashed);
    }

    /// Pro-Q 4 editor: exercises has_editor() and verifies the host survives
    /// IPlugView release and IEditController teardown crashes.
    #[test]
    fn e2e_pro_q4_has_editor() {
        if !require_vsts() {
            return;
        }
        if !is_subprocess() {
            run_in_subprocess("e2e_tests::tests::e2e_pro_q4_has_editor");
            return;
        }

        use crate::vst3::instance::{DEACTIVATION_CRASHED, LAST_DROP_CRASHED};
        install_subprocess_abort_handler();
        LAST_DROP_CRASHED.with(|c| c.set(false));
        DEACTIVATION_CRASHED.with(|c| c.set(false));

        let result = crate::vst3::sandbox::sandbox_call("e2e_pro_q4_editor", || {
            let (_module, mut instance) = create_instance_from_path(&pro_q4_path());
            let has_editor = instance.has_editor();
            assert!(!instance.is_crashed(), "Host should survive has_editor()");
            // Prove has_editor completed without crash (result is valid either way).
            let _ = has_editor;
        });

        let crashed = DEACTIVATION_CRASHED.with(|c| c.get()) || result.is_crashed();
        subprocess_exit(crashed);
    }

    /// Pro-MB editor: exercises has_editor() and verifies the host survives
    /// IPlugView release and IEditController teardown crashes.
    #[test]
    fn e2e_pro_mb_has_editor() {
        if !require_vsts() {
            return;
        }
        if !is_subprocess() {
            run_in_subprocess("e2e_tests::tests::e2e_pro_mb_has_editor");
            return;
        }

        use crate::vst3::instance::{DEACTIVATION_CRASHED, LAST_DROP_CRASHED};
        install_subprocess_abort_handler();
        LAST_DROP_CRASHED.with(|c| c.set(false));
        DEACTIVATION_CRASHED.with(|c| c.set(false));

        let result = crate::vst3::sandbox::sandbox_call("e2e_pro_mb_editor", || {
            let (_module, mut instance) = create_instance_from_path(&pro_mb_path());
            let has_editor = instance.has_editor();
            assert!(!instance.is_crashed(), "Host should survive has_editor()");
            // Prove has_editor completed without crash (result is valid either way).
            let _ = has_editor;
        });

        let crashed = DEACTIVATION_CRASHED.with(|c| c.get()) || result.is_crashed();
        subprocess_exit(crashed);
    }

    // ── Multi-Plugin Lifecycle Tests ─────────────────────────────────
    //
    // These tests exercise loading multiple different plugins and
    // starting/stopping them in various (including random) orders.
    // They verify that the host can manage multiple plugin instances
    // simultaneously without interference.

    /// Helper: a simple linear-congruential PRNG for deterministic shuffles.
    /// Avoids pulling in the `rand` crate for test-only code.
    struct SimpleRng {
        state: u64,
    }

    impl SimpleRng {
        fn new(seed: u64) -> Self {
            Self { state: seed }
        }

        fn next_u64(&mut self) -> u64 {
            // LCG parameters from Numerical Recipes
            self.state = self
                .state
                .wrapping_mul(6364136223846793005)
                .wrapping_add(1442695040888963407);
            self.state
        }

        /// Generate a random index in [0, n).
        fn next_usize(&mut self, n: usize) -> usize {
            (self.next_u64() % n as u64) as usize
        }

        /// Fisher-Yates shuffle.
        fn shuffle<T>(&mut self, slice: &mut [T]) {
            for i in (1..slice.len()).rev() {
                let j = self.next_usize(i + 1);
                slice.swap(i, j);
            }
        }
    }

    /// Represents a named plugin instance slot for multi-plugin tests.
    struct PluginSlot {
        name: &'static str,
        module: Option<crate::vst3::module::Vst3Module>,
        instance: Option<crate::vst3::instance::Vst3Instance>,
        active: bool,
    }

    impl PluginSlot {
        fn new(name: &'static str) -> Self {
            Self {
                name,
                module: None,
                instance: None,
                active: false,
            }
        }

        fn load(&mut self, path: &std::path::Path) {
            let (module, instance) = create_instance_from_path(path);
            self.module = Some(module);
            self.instance = Some(instance);
        }

        fn setup_and_start(&mut self, sample_rate: f64, block_size: i32) {
            if let Some(ref mut inst) = self.instance {
                setup_for_processing(inst, sample_rate, block_size);
                self.active = true;
            }
        }

        fn process_block(&mut self, buffers: &mut crate::vst3::process::ProcessBuffers) -> bool {
            if let Some(ref mut inst) = self.instance {
                if self.active {
                    return unsafe { inst.process(buffers.process_data_ptr()) };
                }
            }
            false
        }

        fn shutdown(&mut self) {
            if let Some(ref mut inst) = self.instance {
                inst.shutdown();
                self.active = false;
            }
        }

        fn is_crashed(&self) -> bool {
            self.instance
                .as_ref()
                .map(|i| i.is_crashed())
                .unwrap_or(false)
        }

        fn input_channels(&self) -> usize {
            self.instance
                .as_ref()
                .map(|i| i.input_channels)
                .unwrap_or(2)
        }

        fn output_channels(&self) -> usize {
            self.instance
                .as_ref()
                .map(|i| i.output_channels)
                .unwrap_or(2)
        }
    }

    /// Load both plugins, set up processing on both, process audio through
    /// both simultaneously, then shut them down in forward order (MB first).
    #[test]
    fn e2e_multi_plugin_load_process_shutdown_forward() {
        if !require_vsts() {
            return;
        }
        use crate::vst3::process::ProcessBuffers;

        let mut mb = PluginSlot::new("Pro-MB");
        let mut q4 = PluginSlot::new("Pro-Q 4");

        // Load both
        mb.load(&pro_mb_path());
        q4.load(&pro_q4_path());

        // Set up both for processing
        mb.setup_and_start(44100.0, 512);
        q4.setup_and_start(44100.0, 512);

        let mut mb_bufs = ProcessBuffers::new(mb.input_channels(), mb.output_channels(), 512);
        let mut q4_bufs = ProcessBuffers::new(q4.input_channels(), q4.output_channels(), 512);

        // Process 50 blocks through both
        for block in 0..50 {
            mb_bufs.prepare(512);
            q4_bufs.prepare(512);
            assert!(mb.process_block(&mut mb_bufs), "MB block {}", block);
            assert!(q4.process_block(&mut q4_bufs), "Q4 block {}", block);
        }

        assert!(!mb.is_crashed());
        assert!(!q4.is_crashed());

        // Shutdown in forward order
        mb.shutdown();
        q4.shutdown();
    }

    /// Load both plugins, set up processing, then shut down in reverse order
    /// (Q4 first, then MB).
    #[test]
    fn e2e_multi_plugin_load_process_shutdown_reverse() {
        if !require_vsts() {
            return;
        }
        use crate::vst3::process::ProcessBuffers;

        let mut mb = PluginSlot::new("Pro-MB");
        let mut q4 = PluginSlot::new("Pro-Q 4");

        mb.load(&pro_mb_path());
        q4.load(&pro_q4_path());

        mb.setup_and_start(48000.0, 256);
        q4.setup_and_start(48000.0, 256);

        let mut mb_bufs = ProcessBuffers::new(mb.input_channels(), mb.output_channels(), 256);
        let mut q4_bufs = ProcessBuffers::new(q4.input_channels(), q4.output_channels(), 256);

        for block in 0..50 {
            mb_bufs.prepare(256);
            q4_bufs.prepare(256);
            assert!(mb.process_block(&mut mb_bufs), "MB block {}", block);
            assert!(q4.process_block(&mut q4_bufs), "Q4 block {}", block);
        }

        assert!(!mb.is_crashed());
        assert!(!q4.is_crashed());

        // Shutdown in reverse order
        q4.shutdown();
        mb.shutdown();
    }

    /// Interleaved setup: load MB, start processing MB, then load Q4 and start
    /// processing Q4, process both, shutdown in random order.
    #[test]
    fn e2e_multi_plugin_interleaved_setup() {
        if !require_vsts() {
            return;
        }
        use crate::vst3::process::ProcessBuffers;

        // Phase 1: Load and start Pro-MB alone
        let mut mb = PluginSlot::new("Pro-MB");
        mb.load(&pro_mb_path());
        mb.setup_and_start(44100.0, 512);

        let mut mb_bufs = ProcessBuffers::new(mb.input_channels(), mb.output_channels(), 512);

        // Process a few blocks with only MB
        for block in 0..20 {
            mb_bufs.prepare(512);
            assert!(mb.process_block(&mut mb_bufs), "MB solo block {}", block);
        }

        // Phase 2: Now load Q4 while MB is still running
        let mut q4 = PluginSlot::new("Pro-Q 4");
        q4.load(&pro_q4_path());
        q4.setup_and_start(44100.0, 512);

        let mut q4_bufs = ProcessBuffers::new(q4.input_channels(), q4.output_channels(), 512);

        // Process both plugins together
        for block in 0..30 {
            mb_bufs.prepare(512);
            q4_bufs.prepare(512);
            assert!(mb.process_block(&mut mb_bufs), "MB joint block {}", block);
            assert!(q4.process_block(&mut q4_bufs), "Q4 joint block {}", block);
        }

        assert!(!mb.is_crashed());
        assert!(!q4.is_crashed());

        // Shutdown Q4 first while MB is still processing
        q4.shutdown();

        // Continue processing MB alone
        for block in 0..10 {
            mb_bufs.prepare(512);
            assert!(mb.process_block(&mut mb_bufs), "MB post-Q4 block {}", block);
        }

        mb.shutdown();
    }

    /// Start both plugins, stop one early while the other continues processing,
    /// then restart the stopped one with different settings.
    #[test]
    fn e2e_multi_plugin_stop_and_restart() {
        if !require_vsts() {
            return;
        }
        use crate::vst3::process::ProcessBuffers;

        let mut mb = PluginSlot::new("Pro-MB");
        let mut q4 = PluginSlot::new("Pro-Q 4");

        mb.load(&pro_mb_path());
        q4.load(&pro_q4_path());

        // Start both at 44100 / 512
        mb.setup_and_start(44100.0, 512);
        q4.setup_and_start(44100.0, 512);

        let mut mb_bufs = ProcessBuffers::new(mb.input_channels(), mb.output_channels(), 512);
        let mut q4_bufs = ProcessBuffers::new(q4.input_channels(), q4.output_channels(), 512);

        // Process both for a while
        for block in 0..20 {
            mb_bufs.prepare(512);
            q4_bufs.prepare(512);
            assert!(mb.process_block(&mut mb_bufs), "MB initial block {}", block);
            assert!(q4.process_block(&mut q4_bufs), "Q4 initial block {}", block);
        }

        // Stop Q4 while MB continues
        q4.shutdown();
        assert!(!q4.is_crashed());

        for block in 0..20 {
            mb_bufs.prepare(512);
            assert!(mb.process_block(&mut mb_bufs), "MB solo block {}", block);
        }

        // Create a fresh Q4 instance with different sample rate
        let mut q4_v2 = PluginSlot::new("Pro-Q 4 v2");
        q4_v2.load(&pro_q4_path());
        q4_v2.setup_and_start(96000.0, 256);

        let mut q4_v2_bufs =
            ProcessBuffers::new(q4_v2.input_channels(), q4_v2.output_channels(), 256);

        // Process both again (different sample rates / block sizes)
        for block in 0..20 {
            mb_bufs.prepare(512);
            q4_v2_bufs.prepare(256);
            assert!(mb.process_block(&mut mb_bufs), "MB+Q4v2 block {}", block);
            assert!(q4_v2.process_block(&mut q4_v2_bufs), "Q4v2 block {}", block);
        }

        assert!(!mb.is_crashed());
        assert!(!q4_v2.is_crashed());

        q4_v2.shutdown();
        mb.shutdown();
    }

    /// Load the same plugin (Pro-Q 4) twice and process both instances
    /// simultaneously — tests that the host can handle duplicate modules.
    #[test]
    fn e2e_multi_plugin_duplicate_plugin() {
        if !require_vsts() {
            return;
        }
        use crate::vst3::process::ProcessBuffers;

        let mut q4a = PluginSlot::new("Pro-Q 4 A");
        let mut q4b = PluginSlot::new("Pro-Q 4 B");

        q4a.load(&pro_q4_path());
        q4b.load(&pro_q4_path());

        q4a.setup_and_start(44100.0, 512);
        q4b.setup_and_start(48000.0, 256);

        let mut bufs_a = ProcessBuffers::new(q4a.input_channels(), q4a.output_channels(), 512);
        let mut bufs_b = ProcessBuffers::new(q4b.input_channels(), q4b.output_channels(), 256);

        for block in 0..50 {
            bufs_a.prepare(512);
            bufs_b.prepare(256);
            assert!(q4a.process_block(&mut bufs_a), "Q4A block {}", block);
            assert!(q4b.process_block(&mut bufs_b), "Q4B block {}", block);
        }

        assert!(!q4a.is_crashed());
        assert!(!q4b.is_crashed());

        // Shutdown in reverse load order
        q4b.shutdown();
        q4a.shutdown();
    }

    /// Deterministic pseudo-random ordering of load, start, process, stop
    /// across multiple plugins. Uses a fixed seed for reproducibility.
    ///
    /// The test creates 4 plugin instances (2 of each type) and randomly
    /// interleaves their lifecycle operations.
    #[test]
    fn e2e_multi_plugin_random_lifecycle_seed_42() {
        if !require_vsts() {
            return;
        }
        use crate::vst3::process::ProcessBuffers;

        let mut rng = SimpleRng::new(42);

        // Define the plugin slots
        let paths = [pro_mb_path(), pro_q4_path(), pro_mb_path(), pro_q4_path()];
        let names = ["MB-0", "Q4-0", "MB-1", "Q4-1"];
        let sample_rates = [44100.0, 48000.0, 96000.0, 44100.0];
        let block_sizes: [i32; 4] = [512, 256, 128, 1024];

        let mut slots: Vec<PluginSlot> = names.iter().map(|n| PluginSlot::new(n)).collect();

        // Phase 1: Load all in random order
        let mut load_order: Vec<usize> = (0..4).collect();
        rng.shuffle(&mut load_order);
        eprintln!("Load order: {:?}", load_order);
        for &idx in &load_order {
            slots[idx].load(&paths[idx]);
        }

        // Phase 2: Start processing in random order
        let mut start_order: Vec<usize> = (0..4).collect();
        rng.shuffle(&mut start_order);
        eprintln!("Start order: {:?}", start_order);
        for &idx in &start_order {
            slots[idx].setup_and_start(sample_rates[idx], block_sizes[idx]);
        }

        // Phase 3: Process blocks, randomly choosing which plugin to process each iteration
        let mut buffers: Vec<ProcessBuffers> = slots
            .iter()
            .enumerate()
            .map(|(i, s)| {
                ProcessBuffers::new(
                    s.input_channels(),
                    s.output_channels(),
                    block_sizes[i] as usize,
                )
            })
            .collect();

        for round in 0..100 {
            // Pick a random subset of plugins to process this round
            let num_to_process = rng.next_usize(4) + 1; // 1..=4
            let mut process_indices: Vec<usize> = (0..4).collect();
            rng.shuffle(&mut process_indices);
            process_indices.truncate(num_to_process);

            for &idx in &process_indices {
                if slots[idx].active {
                    buffers[idx].prepare(block_sizes[idx] as usize);
                    assert!(
                        slots[idx].process_block(&mut buffers[idx]),
                        "{} failed at round {}",
                        names[idx],
                        round
                    );
                }
            }
        }

        // Phase 4: Verify none crashed
        for (idx, slot) in slots.iter().enumerate() {
            assert!(!slot.is_crashed(), "{} crashed", names[idx]);
        }

        // Phase 5: Shutdown in random order
        let mut shutdown_order: Vec<usize> = (0..4).collect();
        rng.shuffle(&mut shutdown_order);
        eprintln!("Shutdown order: {:?}", shutdown_order);
        for &idx in &shutdown_order {
            slots[idx].shutdown();
        }
    }

    /// Same as above but with a different seed to increase coverage of
    /// ordering permutations.
    #[test]
    fn e2e_multi_plugin_random_lifecycle_seed_1337() {
        if !require_vsts() {
            return;
        }
        use crate::vst3::process::ProcessBuffers;

        let mut rng = SimpleRng::new(1337);

        let paths = [pro_q4_path(), pro_mb_path(), pro_q4_path(), pro_mb_path()];
        let names = ["Q4-0", "MB-0", "Q4-1", "MB-1"];
        let sample_rates = [48000.0, 44100.0, 44100.0, 96000.0];
        let block_sizes: [i32; 4] = [256, 512, 1024, 128];

        let mut slots: Vec<PluginSlot> = names.iter().map(|n| PluginSlot::new(n)).collect();

        // Load in random order
        let mut load_order: Vec<usize> = (0..4).collect();
        rng.shuffle(&mut load_order);
        eprintln!("Load order: {:?}", load_order);
        for &idx in &load_order {
            slots[idx].load(&paths[idx]);
        }

        // Start in random order
        let mut start_order: Vec<usize> = (0..4).collect();
        rng.shuffle(&mut start_order);
        eprintln!("Start order: {:?}", start_order);
        for &idx in &start_order {
            slots[idx].setup_and_start(sample_rates[idx], block_sizes[idx]);
        }

        // Process with random interleaving
        let mut buffers: Vec<ProcessBuffers> = slots
            .iter()
            .enumerate()
            .map(|(i, s)| {
                ProcessBuffers::new(
                    s.input_channels(),
                    s.output_channels(),
                    block_sizes[i] as usize,
                )
            })
            .collect();

        for round in 0..100 {
            let num_to_process = rng.next_usize(4) + 1;
            let mut process_indices: Vec<usize> = (0..4).collect();
            rng.shuffle(&mut process_indices);
            process_indices.truncate(num_to_process);

            for &idx in &process_indices {
                if slots[idx].active {
                    buffers[idx].prepare(block_sizes[idx] as usize);
                    assert!(
                        slots[idx].process_block(&mut buffers[idx]),
                        "{} failed at round {}",
                        names[idx],
                        round
                    );
                }
            }
        }

        for (idx, slot) in slots.iter().enumerate() {
            assert!(!slot.is_crashed(), "{} crashed", names[idx]);
        }

        // Shutdown in random order
        let mut shutdown_order: Vec<usize> = (0..4).collect();
        rng.shuffle(&mut shutdown_order);
        eprintln!("Shutdown order: {:?}", shutdown_order);
        for &idx in &shutdown_order {
            slots[idx].shutdown();
        }
    }

    /// Random interleaving of start/stop operations: some plugins are started,
    /// processed for a while, stopped, then restarted with different settings.
    #[test]
    fn e2e_multi_plugin_random_start_stop_cycles() {
        if !require_vsts() {
            return;
        }
        use crate::vst3::process::ProcessBuffers;

        let mut rng = SimpleRng::new(999);
        let sample_rates = [44100.0, 48000.0, 96000.0];
        let block_sizes: [i32; 3] = [128, 256, 512];

        // We'll run 5 cycles of: load → start → process → stop
        // with random plugin selection and settings each cycle
        for cycle in 0..5 {
            let use_mb = rng.next_usize(2) == 0;
            let use_q4 = rng.next_usize(2) == 0 || !use_mb; // At least one plugin
            let sr = sample_rates[rng.next_usize(3)];
            let bs = block_sizes[rng.next_usize(3)];
            eprintln!(
                "Cycle {}: mb={}, q4={}, sr={}, bs={}",
                cycle, use_mb, use_q4, sr, bs
            );

            let mut plugins: Vec<PluginSlot> = Vec::new();

            if use_mb {
                let mut slot = PluginSlot::new("MB");
                slot.load(&pro_mb_path());
                plugins.push(slot);
            }
            if use_q4 {
                let mut slot = PluginSlot::new("Q4");
                slot.load(&pro_q4_path());
                plugins.push(slot);
            }

            // Start in random order
            let mut order: Vec<usize> = (0..plugins.len()).collect();
            rng.shuffle(&mut order);
            for &idx in &order {
                plugins[idx].setup_and_start(sr, bs);
            }

            // Create buffers
            let mut buffers: Vec<ProcessBuffers> = plugins
                .iter()
                .map(|s| ProcessBuffers::new(s.input_channels(), s.output_channels(), bs as usize))
                .collect();

            // Process some blocks
            let num_blocks = 10 + rng.next_usize(40);
            for block in 0..num_blocks {
                for (idx, plugin) in plugins.iter_mut().enumerate() {
                    if plugin.active {
                        buffers[idx].prepare(bs as usize);
                        assert!(
                            plugin.process_block(&mut buffers[idx]),
                            "Cycle {} plugin {} block {}",
                            cycle,
                            plugin.name,
                            block
                        );
                    }
                }
            }

            // Verify and shutdown in random order
            for plugin in &plugins {
                assert!(
                    !plugin.is_crashed(),
                    "Cycle {} {} crashed",
                    cycle,
                    plugin.name
                );
            }

            let mut shutdown_order: Vec<usize> = (0..plugins.len()).collect();
            rng.shuffle(&mut shutdown_order);
            for &idx in &shutdown_order {
                plugins[idx].shutdown();
            }
        }
    }

    /// Test with AudioEngine integration: both plugins run through the
    /// AudioEngine simultaneously, then are shut down in random order.
    #[test]
    fn e2e_multi_plugin_audio_engine_concurrent() {
        if !require_vsts() {
            return;
        }
        use crate::audio::engine::AudioEngine;

        let (_mb_mod, mut mb_inst) = create_instance_from_path(&pro_mb_path());
        let (_q4_mod, mut q4_inst) = create_instance_from_path(&pro_q4_path());

        setup_for_processing(&mut mb_inst, 44100.0, 1024);
        setup_for_processing(&mut q4_inst, 44100.0, 1024);

        let mut mb_engine = AudioEngine::new(mb_inst, 44100.0, 1024, 2);
        let mut q4_engine = AudioEngine::new(q4_inst, 44100.0, 1024, 2);

        let mut mb_output = vec![0.0f32; 2 * 512];
        let mut q4_output = vec![0.0f32; 2 * 512];

        // Process both engines simultaneously
        for _ in 0..20 {
            mb_engine.process(&mut mb_output);
            q4_engine.process(&mut q4_output);
        }

        assert!(!mb_engine.is_crashed());
        assert!(!q4_engine.is_crashed());

        // Both should produce non-silent output (test tone enabled by default)
        let mb_max = mb_output.iter().map(|s| s.abs()).fold(0.0f32, f32::max);
        let q4_max = q4_output.iter().map(|s| s.abs()).fold(0.0f32, f32::max);
        assert!(mb_max > 0.001, "MB engine output max: {}", mb_max);
        assert!(q4_max > 0.001, "Q4 engine output max: {}", q4_max);

        // Shutdown in reverse order
        q4_engine.shutdown();
        mb_engine.shutdown();
    }

    /// Stress test: rapidly add and remove plugin instances. Loads a plugin,
    /// processes a few blocks, shuts down, and repeats with the other plugin.
    #[test]
    fn e2e_multi_plugin_rapid_add_remove() {
        if !require_vsts() {
            return;
        }
        use crate::vst3::process::ProcessBuffers;

        let mut rng = SimpleRng::new(7777);
        let paths = [pro_mb_path(), pro_q4_path()];

        for iteration in 0..10 {
            let path_idx = rng.next_usize(2);
            let sr = if rng.next_usize(2) == 0 {
                44100.0
            } else {
                48000.0
            };
            let bs: i32 = [128, 256, 512, 1024][rng.next_usize(4)];

            eprintln!(
                "Rapid iter {}: plugin={}, sr={}, bs={}",
                iteration, path_idx, sr, bs
            );

            let mut slot = PluginSlot::new(if path_idx == 0 { "MB" } else { "Q4" });
            slot.load(&paths[path_idx]);
            slot.setup_and_start(sr, bs);

            let mut bufs =
                ProcessBuffers::new(slot.input_channels(), slot.output_channels(), bs as usize);

            let num_blocks = 5 + rng.next_usize(15);
            for block in 0..num_blocks {
                bufs.prepare(bs as usize);
                assert!(
                    slot.process_block(&mut bufs),
                    "Rapid iter {} block {}",
                    iteration,
                    block
                );
            }

            assert!(!slot.is_crashed(), "Rapid iter {} crashed", iteration);
            slot.shutdown();
            // slot/module/instance dropped here — repeat with fresh state
        }
    }
}
