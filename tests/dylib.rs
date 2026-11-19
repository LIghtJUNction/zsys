#![cfg(feature = "dylib")]

use zsys::dylib::{Error, Library};

#[cfg(any(
    all(target_os = "linux", not(all(target_env = "musl", target_feature = "crt-static"))),
    target_os = "macos",
    windows
))]
mod native {
    use super::*;
    use std::{
        ffi::CString,
        path::{Path, PathBuf},
        process::Command,
        sync::atomic::{AtomicUsize, Ordering},
    };

    static NEXT: AtomicUsize = AtomicUsize::new(0);

    struct Fixture {
        directory: PathBuf,
        library: PathBuf,
    }

    impl Fixture {
        fn build() -> Self {
            let directory = std::env::temp_dir().join(format!(
                "zsys-dylib-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            std::fs::create_dir(&directory).unwrap();
            let library =
                directory.join(format!("{}zsys_fixture{}", std::env::consts::DLL_PREFIX, std::env::consts::DLL_SUFFIX));
            let source = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/dylib.rs");
            let output = Command::new(std::env::var_os("RUSTC").unwrap_or_else(|| "rustc".into()))
                .args(["--edition=2024", "--crate-type=cdylib", "--crate-name=zsys_fixture"])
                .arg(source)
                .arg("-o")
                .arg(&library)
                .output()
                .unwrap();
            assert!(output.status.success(), "fixture compilation failed: {}", String::from_utf8_lossy(&output.stderr));
            Self { directory, library }
        }
    }

    impl Drop for Fixture {
        fn drop(&mut self) {
            std::fs::remove_dir_all(&self.directory).unwrap();
        }
    }

    unsafe fn open_path(path: &Path) -> Result<Library, Error> {
        #[cfg(windows)]
        {
            use std::os::windows::ffi::OsStrExt;
            let path: Vec<u16> = path.as_os_str().encode_wide().chain(Some(0)).collect();
            // SAFETY: caller supplies a trusted independently built test library.
            unsafe { Library::open_wide(&path) }
        }
        #[cfg(not(windows))]
        {
            use std::os::unix::ffi::OsStrExt;
            let path = CString::new(path.as_os_str().as_bytes()).unwrap();
            // SAFETY: caller supplies a trusted independently built test library.
            unsafe { Library::open(&path) }
        }
    }

    fn fixture_fn(library: &Library) -> unsafe extern "C" fn(u32) -> u32 {
        // SAFETY: the independently compiled fixture exports this exact C ABI.
        unsafe {
            core::mem::transmute::<*mut core::ffi::c_void, unsafe extern "C" fn(u32) -> u32>(
                library.symbol(c"zsys_fixture_add_one").unwrap(),
            )
        }
    }

    #[test]
    fn native_reference_counts_symbols_and_explicit_close() {
        let fixture = Fixture::build();
        // SAFETY: this test controls the source and build of the native fixture.
        let mut first = unsafe { open_path(&fixture.library) }.unwrap();
        let mut second = unsafe { open_path(&fixture.library) }.unwrap();
        assert!(first.same_image(&second));
        assert!(first.is_open());
        assert_eq!(first.symbol(c"zsys_fixture_missing"), Err(Error::MissingSymbol));
        assert_eq!(unsafe { fixture_fn(&first)(41) }, 42);

        // SAFETY: no first-handle symbols or native work remain in use.
        unsafe { first.close() }.unwrap();
        assert!(!first.is_open());
        assert!(!first.same_image(&second));
        assert_eq!(first.symbol(c"zsys_fixture_add_one"), Err(Error::Closed));
        // The independently owned second reference must keep the image alive.
        assert_eq!(unsafe { fixture_fn(&second)(41) }, 42);
        unsafe { second.close() }.unwrap();
        unsafe { second.close() }.unwrap();
        assert!(!first.same_image(&second));

        // SAFETY: the nonexistent path cannot execute native code.
        let absent = unsafe { open_path(&fixture.directory.join("absent-library")) };
        assert!(matches!(absent, Err(Error::LoadFailed)));
    }

    #[test]
    fn dropping_handle_retains_native_image() {
        let fixture = Fixture::build();
        let output = Command::new(std::env::current_exe().unwrap())
            .args(["--exact", "native::pinning_child", "--ignored", "--nocapture"])
            .env("ZSYS_DYLIB_PIN_FIXTURE", &fixture.library)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "pinning subprocess failed: {}\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        // The child has exited, so even Windows can remove its formerly pinned DLL.
    }

    #[test]
    #[ignore = "invoked by dropping_handle_retains_native_image in a separate process"]
    fn pinning_child() {
        let path = std::env::var_os("ZSYS_DYLIB_PIN_FIXTURE").expect("fixture supplied by parent test");
        // SAFETY: parent test independently compiled this trusted fixture.
        let function = {
            let library = unsafe { open_path(Path::new(&path)) }.unwrap();
            fixture_fn(&library)
        };
        // SAFETY: dropping the handle deliberately retains the OS reference.
        assert_eq!(unsafe { function(41) }, 42);
    }

    #[cfg(windows)]
    #[test]
    fn windows_paths_preserve_unicode_and_validate_terminators() {
        assert!(matches!(unsafe { Library::open_wide(&[]) }, Err(Error::InvalidPath)));
        assert!(matches!(unsafe { Library::open_wide(&[65]) }, Err(Error::InvalidPath)));
        assert!(matches!(unsafe { Library::open_wide(&[65, 0, 66, 0]) }, Err(Error::InvalidPath)));
        let fixture = Fixture::build();
        // Exercise the narrow API independently on the generated ASCII path.
        if let Some(path) = fixture.library.to_str().filter(|path| path.is_ascii()) {
            let ascii = CString::new(path).unwrap();
            let mut narrow = unsafe { Library::open(&ascii) }.unwrap();
            unsafe { narrow.close() }.unwrap();
        }
        let unicode = fixture.directory.join("zsys-接口.dll");
        std::fs::rename(&fixture.library, &unicode).unwrap();
        let mut wide = unsafe { open_path(&unicode) }.unwrap();
        assert_eq!(unsafe { fixture_fn(&wide)(41) }, 42);
        unsafe { wide.close() }.unwrap();
    }
}

#[cfg(not(any(
    all(target_os = "linux", not(all(target_env = "musl", target_feature = "crt-static"))),
    target_os = "macos",
    windows
)))]
#[test]
fn unavailable_backends_return_explicit_error() {
    // SAFETY: unsupported targets never call a native loader.
    assert!(matches!(unsafe { Library::open(c"unused") }, Err(Error::Unsupported)));
}
