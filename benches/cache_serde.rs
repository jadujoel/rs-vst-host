//! Benchmarks for ScanCache and plugin type serde — scan result serialization.

use divan::Bencher;
use rs_vst_host::vst3::cache::ScanCache;
use rs_vst_host::vst3::types::{PluginClassInfo, PluginModuleInfo};
use std::path::PathBuf;

fn main() {
    divan::main();
}

// ─── Helpers ───────────────────────────────────────────────────────────────

fn sample_class(i: usize) -> PluginClassInfo {
    PluginClassInfo {
        name: format!("Plugin Class {}", i),
        category: "Audio Module Class".to_string(),
        subcategories: Some("Fx|EQ".to_string()),
        vendor: Some(format!("Vendor {}", i)),
        version: Some("1.0.0".to_string()),
        sdk_version: Some("VST 3.7".to_string()),
        cid: [i as u8; 16],
    }
}

fn sample_module(i: usize, classes_per_module: usize) -> PluginModuleInfo {
    PluginModuleInfo {
        path: PathBuf::from(format!("/Library/Audio/Plug-Ins/VST3/Module{}.vst3", i)),
        factory_vendor: Some(format!("Vendor {}", i)),
        factory_url: Some("https://example.com".to_string()),
        factory_email: Some("test@example.com".to_string()),
        classes: (0..classes_per_module)
            .map(|j| sample_class(i * 100 + j))
            .collect(),
    }
}

fn sample_cache(num_modules: usize, classes_per_module: usize) -> ScanCache {
    ScanCache::new(
        (0..num_modules)
            .map(|i| sample_module(i, classes_per_module))
            .collect(),
    )
}

// ─── PluginClassInfo serde ─────────────────────────────────────────────────

#[divan::bench]
fn serialize_class_info(bencher: Bencher) {
    let info = sample_class(0);
    bencher.bench(|| serde_json::to_string(divan::black_box(&info)).unwrap());
}

#[divan::bench]
fn deserialize_class_info(bencher: Bencher) {
    let json = serde_json::to_string(&sample_class(0)).unwrap();
    bencher.bench(|| serde_json::from_str::<PluginClassInfo>(divan::black_box(&json)).unwrap());
}

// ─── PluginModuleInfo serde ────────────────────────────────────────────────

#[divan::bench(args = [1, 4, 8])]
fn serialize_module_info(bencher: Bencher, classes: usize) {
    bencher
        .with_inputs(|| sample_module(0, classes))
        .bench_refs(|info| serde_json::to_string(info).unwrap());
}

#[divan::bench(args = [1, 4, 8])]
fn deserialize_module_info(bencher: Bencher, classes: usize) {
    bencher
        .with_inputs(|| serde_json::to_string(&sample_module(0, classes)).unwrap())
        .bench_refs(|json| serde_json::from_str::<PluginModuleInfo>(json).unwrap());
}

// ─── ScanCache serde ───────────────────────────────────────────────────────

#[divan::bench(args = [4, 16, 64])]
fn serialize_cache(bencher: Bencher, num_modules: usize) {
    bencher
        .with_inputs(|| sample_cache(num_modules, 2))
        .bench_refs(|cache| serde_json::to_string(cache).unwrap());
}

#[divan::bench(args = [4, 16, 64])]
fn deserialize_cache(bencher: Bencher, num_modules: usize) {
    bencher
        .with_inputs(|| serde_json::to_string(&sample_cache(num_modules, 2)).unwrap())
        .bench_refs(|json| serde_json::from_str::<ScanCache>(json).unwrap());
}

#[divan::bench(args = [4, 16, 64])]
fn roundtrip_cache(bencher: Bencher, num_modules: usize) {
    bencher
        .with_inputs(|| sample_cache(num_modules, 2))
        .bench_refs(|cache| {
            let json = serde_json::to_string(cache).unwrap();
            divan::black_box(serde_json::from_str::<ScanCache>(&json).unwrap());
        });
}

// ─── ScanCache::new (timestamp generation) ─────────────────────────────────

#[divan::bench(args = [4, 16, 64])]
fn cache_new(bencher: Bencher, num_modules: usize) {
    bencher
        .with_inputs(|| {
            (0..num_modules)
                .map(|i| sample_module(i, 2))
                .collect::<Vec<_>>()
        })
        .bench_refs(|modules| ScanCache::new(modules.clone()));
}
