use cmake::Config;

fn main() {
    let is_github_actions: bool = std::env::var("GITHUB_ACTIONS").is_ok();

    // Only rerun this when the bridge.rs or static_bridge.h file changes.
    println!("cargo:rerun-if-changed=src/bridge.rs");
    println!("cargo:rerun-if-changed=src/bridge.h");

    // Build with the monero library all dependencies required
    let mut config = Config::new("monero");
    let output_directory = config
        .build_target("wallet_api")
        // Builds currently fail in Release mode
        // .define("CMAKE_BUILD_TYPE", "Release")
        // .define("CMAKE_RELEASE_TYPE", "Release")
        // Force building static libraries
        .define("STATIC", "ON")
        .define("BUILD_SHARED_LIBS", "OFF")
        .define("BUILD_TESTS", "OFF")
        .define("Boost_USE_STATIC_LIBS", "ON")
        .define("Boost_USE_STATIC_RUNTIME", "ON")
        //// Disable support for ALL hardware wallets
        // Disable Trezor support completely
        .define("USE_DEVICE_TREZOR", "OFF")
        .define("USE_DEVICE_TREZOR_MANDATORY", "OFF")
        .define("USE_DEVICE_TREZOR_PROTOBUF_TEST", "OFF")
        .define("USE_DEVICE_TREZOR_LIBUSB", "OFF")
        .define("USE_DEVICE_TREZOR_UDP_RELEASE", "OFF")
        .define("USE_DEVICE_TREZOR_DEBUG", "OFF")
        .define("TREZOR_DEBUG", "OFF")
        // Prevent CMake from finding dependencies that could enable Trezor
        .define("CMAKE_DISABLE_FIND_PACKAGE_LibUSB", "ON")
        // Disable Ledger support
        .define("USE_DEVICE_LEDGER", "OFF")
        .define("CMAKE_DISABLE_FIND_PACKAGE_HIDAPI", "ON")
        .define("GTEST_HAS_ABSL", "OFF")
        // Use lightweight crypto library
        .define("MONERO_WALLET_CRYPTO_LIBRARY", "cn")
        .build_arg(match is_github_actions {
            true => "-j1",
            false => "-j",
        })
        .build();

    let monero_build_dir = output_directory.join("build");

    println!(
        "cargo:debug=Build directory: {}",
        output_directory.display()
    );

    // Add output directories to the link search path
    println!(
        "cargo:rustc-link-search=native={}",
        monero_build_dir.join("lib").display()
    );

    // Add additional link search paths for libraries in different directories
    println!(
        "cargo:rustc-link-search=native={}",
        monero_build_dir.join("contrib/epee/src").display()
    );
    println!(
        "cargo:rustc-link-search=native={}",
        monero_build_dir.join("external/easylogging++").display()
    );
    println!(
        "cargo:rustc-link-search=native={}",
        monero_build_dir
            .join("external/db_drivers/liblmdb")
            .display()
    );
    println!(
        "cargo:rustc-link-search=native={}",
        monero_build_dir.join("external/randomx").display()
    );
    println!("cargo:rustc-link-search=native=/usr/lib/x86_64-linux-gnu");

    println!(
        "cargo:rustc-link-search=native={}",
        monero_build_dir.join("src").display()
    );
    println!(
        "cargo:rustc-link-search=native={}",
        monero_build_dir.join("src/crypto").display()
    );
    println!(
        "cargo:rustc-link-search=native={}",
        monero_build_dir.join("src/net").display()
    );
    println!(
        "cargo:rustc-link-search=native={}",
        monero_build_dir.join("src/ringct").display()
    );
    println!(
        "cargo:rustc-link-search=native={}",
        monero_build_dir.join("src/checkpoints").display()
    );
    println!(
        "cargo:rustc-link-search=native={}",
        monero_build_dir.join("src/multisig").display()
    );
    println!(
        "cargo:rustc-link-search=native={}",
        monero_build_dir.join("src/cryptonote_basic").display()
    );
    println!(
        "cargo:rustc-link-search=native={}",
        monero_build_dir.join("src/common").display()
    );
    println!(
        "cargo:rustc-link-search=native={}",
        monero_build_dir.join("src/cryptonote_core").display()
    );
    println!(
        "cargo:rustc-link-search=native={}",
        monero_build_dir.join("src/hardforks").display()
    );
    println!(
        "cargo:rustc-link-search=native={}",
        monero_build_dir.join("src/blockchain_db").display()
    );
    println!(
        "cargo:rustc-link-search=native={}",
        monero_build_dir.join("src/device").display()
    );
    // device_trezor search path (stub version when disabled)
    println!(
        "cargo:rustc-link-search=native={}",
        monero_build_dir.join("src/device_trezor").display()
    );
    println!(
        "cargo:rustc-link-search=native={}",
        monero_build_dir.join("src/mnemonics").display()
    );
    println!(
        "cargo:rustc-link-search=native={}",
        monero_build_dir.join("src/rpc").display()
    );

    #[cfg(target_os = "macos")]
    {
        // Dynamically detect Homebrew installation prefix (works on both Apple Silicon and Intel Macs)
        let brew_prefix = std::process::Command::new("brew")
            .arg("--prefix")
            .output()
            .ok()
            .and_then(|o| String::from_utf8(o.stdout).ok())
            .map(|s| s.trim().to_string())
            .unwrap_or_else(|| "/opt/homebrew".into());

        // add homebrew search paths using dynamic prefix
        println!("cargo:rustc-link-search=native={}/lib", brew_prefix);
        println!(
            "cargo:rustc-link-search=native={}/opt/unbound/lib",
            brew_prefix
        );
        println!(
            "cargo:rustc-link-search=native={}/opt/expat/lib",
            brew_prefix
        );
        println!(
            "cargo:rustc-link-search=native={}/Cellar/protobuf@21/21.12_1/lib/",
            brew_prefix
        );

        // Add search paths for clang runtime libraries
        println !("cargo:rustc-link-search=native=/Library/Developer/CommandLineTools/usr/lib/clang/15.0.0/lib/darwin");
        println !("cargo:rustc-link-search=native=/Library/Developer/CommandLineTools/usr/lib/clang/16.0.0/lib/darwin");
        println !("cargo:rustc-link-search=native=/Library/Developer/CommandLineTools/usr/lib/clang/17.0.0/lib/darwin");
        println !("cargo:rustc-link-search=native=/Library/Developer/CommandLineTools/usr/lib/clang/18.0.0/lib/darwin");
    }

    // Link libwallet and libwallet_api statically
    println!("cargo:rustc-link-lib=static=wallet");
    println!("cargo:rustc-link-lib=static=wallet_api");

    // Link targets of monero codebase statically
    println!("cargo:rustc-link-lib=static=epee");
    println!("cargo:rustc-link-lib=static=easylogging");
    println!("cargo:rustc-link-lib=static=lmdb");
    println!("cargo:rustc-link-lib=static=randomx");
    println!("cargo:rustc-link-lib=static=cncrypto");
    println!("cargo:rustc-link-lib=static=net");
    println!("cargo:rustc-link-lib=static=ringct");
    println!("cargo:rustc-link-lib=static=ringct_basic");
    println!("cargo:rustc-link-lib=static=checkpoints");
    println!("cargo:rustc-link-lib=static=multisig");
    println!("cargo:rustc-link-lib=static=version");
    println!("cargo:rustc-link-lib=static=cryptonote_basic");
    println!("cargo:rustc-link-lib=static=cryptonote_format_utils_basic");
    println!("cargo:rustc-link-lib=static=common");
    println!("cargo:rustc-link-lib=static=cryptonote_core");
    println!("cargo:rustc-link-lib=static=hardforks");
    println!("cargo:rustc-link-lib=static=blockchain_db");
    println!("cargo:rustc-link-lib=static=device");
    // Link device_trezor (stub version when USE_DEVICE_TREZOR=OFF)
    println!("cargo:rustc-link-lib=static=device_trezor");
    println!("cargo:rustc-link-lib=static=mnemonics");
    println!("cargo:rustc-link-lib=static=rpc_base");

    // Static linking for boost
    println!("cargo:rustc-link-lib=static=boost_serialization");
    println!("cargo:rustc-link-lib=static=boost_filesystem");
    println!("cargo:rustc-link-lib=static=boost_thread");
    println!("cargo:rustc-link-lib=static=boost_chrono");

    // Link libsodium statically
    println!("cargo:rustc-link-lib=static=sodium");

    // Link OpenSSL statically
    println!("cargo:rustc-link-lib=static=ssl"); // This is OpenSSL (libsll)
    println!("cargo:rustc-link-lib=static=crypto"); // This is OpenSSLs crypto library (libcrypto)

    // Link unbound statically
    println!("cargo:rustc-link-lib=static=unbound");
    println!("cargo:rustc-link-lib=static=expat"); // Expat is required by unbound
    println!("cargo:rustc-link-lib=static=nghttp2");
    println!("cargo:rustc-link-lib=static=event");

    // Link protobuf statically
    println!("cargo:rustc-link-lib=static=protobuf");

    #[cfg(target_os = "macos")]
    {
        // Static archive is always present, dylib only on some versions.
        println!("cargo:rustc-link-lib=static=clang_rt.osx");

        // Minimum OS version you already add:
        println!("cargo:rustc-link-arg=-mmacosx-version-min=11.0");
    }

    // Build the CXX bridge
    let mut build = cxx_build::bridge("src/bridge.rs");

    #[cfg(target_os = "macos")]
    {
        build.flag_if_supported("-mmacosx-version-min=11.0");
    }

    build
        .flag_if_supported("-std=c++17")
        .include("src") // Include the bridge.h file
        .include("monero/src") // Includes the monero headers
        .include("monero/external/easylogging++") // Includes the easylogging++ headers
        .include("monero/contrib/epee/include") // Includes the epee headers for net/http_client.h
        .include("/opt/homebrew/include") // Homebrew include path for Boost
        .flag("-fPIC"); // Position independent code

    #[cfg(target_os = "macos")]
    {
        // Use the same dynamic brew prefix for include paths
        let brew_prefix = std::process::Command::new("brew")
            .arg("--prefix")
            .output()
            .ok()
            .and_then(|o| String::from_utf8(o.stdout).ok())
            .map(|s| s.trim().to_string())
            .unwrap_or_else(|| "/opt/homebrew".into());

        build.include(format!("{}/include", brew_prefix)); // Homebrew include path for Boost
    }

    build.compile("monero-sys");
}
