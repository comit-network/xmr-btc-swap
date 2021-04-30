use std::process::Command;

fn main() {
    let status = Command::new("docker")
        .arg("build")
        .arg("-f")
        .arg("./tor.Dockerfile")
        .arg(".")
        .arg("-t")
        .arg("testcontainers-tor:latest")
        .status()
        .unwrap();

    assert!(status.success());

    println!("cargo:rerun-if-changed=./tor.Dockerfile");
}
