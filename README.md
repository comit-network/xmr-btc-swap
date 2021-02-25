<h1 align="center">tokio-tar</h1>
<div align="center">
 <strong>
   A tar archive reading/writing library for async Rust.
 </strong>
</div>

<br />

<div align="center">
  <!-- Crates version -->
  <a href="https://crates.io/crates/tokio-tar">
    <img src="https://img.shields.io/crates/v/tokio-tar.svg?style=flat-square"
    alt="Crates.io version" />
  </a>
  <!-- Downloads -->
  <a href="https://crates.io/crates/tokio-tar">
    <img src="https://img.shields.io/crates/d/tokio-tar.svg?style=flat-square"
      alt="Download" />
  </a>
  <!-- docs.rs docs -->
  <a href="https://docs.rs/tokio-tar">
    <img src="https://img.shields.io/badge/docs-latest-blue.svg?style=flat-square"
      alt="docs.rs docs" />
  </a>
</div>

<div align="center">
  <h3>
    <a href="https://docs.rs/tokio-tar">
      API Docs
    </a>
    <span> | </span>
    <a href="https://github.com/vorot93/tokio-tar/releases">
      Releases
    </a>
  </h3>
</div>
<br/>

> Based on the great [tar-rs](https://github.com/alexcrichton/tar-rs).

## Reading an archive

```rust,no_run
use tokio::io::stdin;
use tokio::prelude::*;

use tokio_tar::Archive;

fn main() {
    tokio::runtime::Runtime::new().unwrap().block_on(async {
        let mut ar = Archive::new(stdin());
        let mut entries = ar.entries().unwrap();
        while let Some(file) = entries.next().await {
            let f = file.unwrap();
            println!("{}", f.path().unwrap().display());
        }
    });
}
```

## Writing an archive

```rust,no_run
use tokio::fs::File;
use tokio_tar::Builder;

fn main() {
    tokio::runtime::Runtime::new().unwrap().block_on(async {
        let file = File::create("foo.tar").await.unwrap();
        let mut a = Builder::new(file);

        a.append_path("README.md").await.unwrap();
        a.append_file("lib.rs", &mut File::open("src/lib.rs").await.unwrap())
            .await
            .unwrap();
    });
}
```

# License

This project is licensed under either of

 * Apache License, Version 2.0, ([LICENSE-APACHE](LICENSE-APACHE) or
   http://www.apache.org/licenses/LICENSE-2.0)
 * MIT license ([LICENSE-MIT](LICENSE-MIT) or
   http://opensource.org/licenses/MIT)

at your option.

### Contribution

Unless you explicitly state otherwise, any contribution intentionally submitted
for inclusion in this project by you, as defined in the Apache-2.0 license,
shall be dual licensed as above, without any additional terms or conditions.
