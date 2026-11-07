use std::{
    fs,
    path::PathBuf,
    process::{Command, Output},
    sync::atomic::{AtomicU64, Ordering},
};
use zx81_rust::video::{HEIGHT, WIDTH};

struct Sandbox(PathBuf);
impl Sandbox {
    fn new() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let dir = std::env::temp_dir().join(format!(
            "zx81-rust-cli-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&dir).unwrap();
        fs::copy(env!("CARGO_BIN_EXE_zx81-rust"), dir.join("zx81-rust")).unwrap();
        Self(dir)
    }
    fn run(&self, args: &[&str]) -> Output {
        Command::new(self.0.join("zx81-rust"))
            .current_dir(&self.0)
            .args(args)
            .output()
            .unwrap()
    }
    fn check_image(&self, name: &str) {
        let ppm = fs::read(self.0.join(name)).unwrap();
        let header = format!("P6\n{WIDTH} {HEIGHT}\n255\n");
        assert!(ppm.starts_with(header.as_bytes()));
        assert_eq!(ppm.len(), header.len() + WIDTH * HEIGHT * 3);
        assert!(
            ppm[header.len()..].windows(3).any(|p| p == [0, 0, 0]),
            "The real raster should contain the BASIC cursor"
        );
    }
}
impl Drop for Sandbox {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[test]
fn copied_executable_boots_without_source_tree_rom_files_or_reference_projects() {
    let sandbox = Sandbox::new();
    let result = sandbox.run(&["--headless", "--frames", "5", "--screenshot", "basic.ppm"]);
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    sandbox.check_image("basic.ppm");
    assert_eq!(fs::read_dir(&sandbox.0).unwrap().count(), 2); // binary and screenshot only
}

#[test]
fn positional_p_loads_in_a_separate_process_and_invalid_files_fail_cleanly() {
    let sandbox = Sandbox::new();
    fs::write(
        sandbox.0.join("hello.p"),
        include_bytes!("../examples/hello.p"),
    )
    .unwrap();
    let result = sandbox.run(&[
        "hello.p",
        "--headless",
        "--frames",
        "5",
        "--screenshot",
        "loaded.ppm",
    ]);
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    assert!(String::from_utf8_lossy(&result.stdout).contains("Program loaded"));
    sandbox.check_image("loaded.ppm");
    fs::write(sandbox.0.join("broken.p"), [1, 2, 3]).unwrap();
    fs::write(sandbox.0.join("large.p"), vec![0; 2048]).unwrap();
    for args in [
        &["broken.p", "--headless"][..],
        &["missing.p", "--headless"],
        &["large.p", "--ram", "1", "--headless"],
    ] {
        let result = sandbox.run(args);
        assert!(!result.status.success());
        assert!(!String::from_utf8_lossy(&result.stderr).contains("panicked"));
    }
}
