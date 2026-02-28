cargo build --release
./target/release/rs-vst-host scan --paths plugins/macos/vst3
./target/release/rs-vst-host gui --paths plugins/macos/vst3
