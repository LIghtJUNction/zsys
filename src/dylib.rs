//! Allocation-free access to the platform's native dynamic-library loader.
//!
//! Enable the `dylib` feature; `std` and `alloc` are not required. Linux,
//! macOS and Windows are implemented. Other targets, including statically linked
//! musl, return [`Error::Unsupported`] without performing an OS operation.
//! The wrapper does not allocate Rust memory; the OS loader and native
//! initializers may allocate their own memory.
//!
//! A library stays loaded when its handle is dropped. Call [`Library::close`]
//! only after all code, data and native work using that image have stopped.
//! This explicit lifetime is useful to plugin runtimes that cannot safely unload
//! a library merely because one Rust owner has gone out of scope.

use core::{
    ffi::{CStr, c_void},
    fmt,
    marker::PhantomData,
    ptr::NonNull,
};

/// Whether this build has a native loader backend.
pub const SUPPORTED: bool = cfg!(any(
    all(target_os = "linux", not(all(target_env = "musl", target_feature = "crt-static"))),
    target_os = "macos",
    windows
));

/// A bounded loader failure; no allocation or thread-local error string is kept.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Error {
    /// This target has no usable native loader backend.
    Unsupported,
    /// The OS could not open the requested library.
    LoadFailed,
    /// The handle has already been explicitly closed.
    Closed,
    /// The requested symbol has no non-null address.
    MissingSymbol,
    /// The OS rejected unloading; the handle remains live.
    CloseFailed,
    /// A UTF-16 path is not terminated or contains an interior NUL.
    InvalidPath,
}

impl fmt::Display for Error {
    fn fmt(&self, out: &mut fmt::Formatter<'_>) -> fmt::Result {
        out.write_str(match self {
            Self::Unsupported => "native dynamic loading is unavailable on this target",
            Self::LoadFailed => "the native library could not be loaded",
            Self::Closed => "the native library handle is closed",
            Self::MissingSymbol => "the native symbol was not found",
            Self::CloseFailed => "the native library could not be closed",
            Self::InvalidPath => "the native path is not correctly NUL terminated",
        })
    }
}

impl core::error::Error for Error {}

/// A thread-local, explicitly closed native-library reference.
///
/// The handle is neither `Send` nor `Sync`: library initializers, finalizers
/// and client runtime state can have thread-affine requirements. Dropping it
/// deliberately retains the native reference; it does not unload code.
///
/// ```compile_fail
/// fn needs_send<T: Send>() {}
/// needs_send::<zsys::dylib::Library>();
/// ```
pub struct Library {
    handle: Option<NonNull<c_void>>,
    _local: PhantomData<*mut ()>,
}

impl Library {
    /// Load trusted native code using a NUL-terminated path.
    ///
    /// Unix paths use native bytes. On Windows this narrow API uses the ANSI
    /// code page; use `open_wide` for UTF-16 paths.
    ///
    /// # Safety
    ///
    /// Loading can execute native initializers with process privileges. The
    /// caller must trust the file and its dependencies, including symbol
    /// resolvers, and satisfy their platform and thread requirements.
    pub unsafe fn open(path: &CStr) -> Result<Self, Error> {
        if !SUPPORTED {
            return Err(Error::Unsupported);
        }
        // SAFETY: the caller supplies the native-code trust contract; CStr
        // supplies a valid NUL-terminated path for the duration of this call.
        let handle = unsafe { platform::open(path.as_ptr()) };
        Self::from_opened(handle)
    }

    /// Load a trusted library using a NUL-terminated UTF-16 path.
    ///
    /// # Safety
    ///
    /// The native-code and thread requirements of [`Self::open`] also apply.
    #[cfg(windows)]
    pub unsafe fn open_wide(path: &[u16]) -> Result<Self, Error> {
        if path.last() != Some(&0) || path[..path.len().saturating_sub(1)].contains(&0) {
            return Err(Error::InvalidPath);
        }
        // SAFETY: validation establishes the complete NUL-terminated slice;
        // native-code trust is delegated to this method's caller.
        Self::from_opened(unsafe { platform::open_wide(path.as_ptr()) })
    }

    fn from_opened(handle: *mut c_void) -> Result<Self, Error> {
        NonNull::new(handle).map(|handle| Self { handle: Some(handle), _local: PhantomData }).ok_or(Error::LoadFailed)
    }

    /// Resolve a non-null raw address from this live reference.
    ///
    /// Calling or dereferencing it remains unsafe. The caller must use the
    /// correct ABI/type and keep every reference to the native image valid.
    /// No symbol cache, allocation or typed function-pointer conversion occurs.
    pub fn symbol(&self, name: &CStr) -> Result<*mut c_void, Error> {
        let handle = self.handle.ok_or(Error::Closed)?;
        // SAFETY: this object owns a live OS loader reference and CStr supplies
        // the complete symbol string. Native resolvers are trusted by open.
        NonNull::new(unsafe { platform::symbol(handle.as_ptr(), name.as_ptr()) })
            .map(NonNull::as_ptr)
            .ok_or(Error::MissingSymbol)
    }

    /// Whether both live references have the same OS loader handle.
    ///
    /// Loading a path twice can return the same mapped image with two native
    /// reference counts. Closed references never compare as the same image.
    pub fn same_image(&self, other: &Self) -> bool {
        self.handle.is_some() && self.handle == other.handle
    }

    /// Whether this reference has not been explicitly closed.
    pub fn is_open(&self) -> bool {
        self.handle.is_some()
    }

    /// Release this reference; closing an already closed handle succeeds.
    ///
    /// If the OS reports a failure, this object keeps its handle for a retry.
    ///
    /// # Safety
    ///
    /// No object, buffer, thread, task, callback, destructor, reference or
    /// function pointer may still require this reference to keep the image
    /// loaded. Native finalizers must be safe to execute on the current thread.
    pub unsafe fn close(&mut self) -> Result<(), Error> {
        if let Some(handle) = self.handle {
            // SAFETY: the caller has stopped all uses that require this
            // reference, including native finalizers and asynchronous work.
            if !unsafe { platform::close(handle.as_ptr()) } {
                return Err(Error::CloseFailed);
            }
            self.handle = None;
        }
        Ok(())
    }
}

#[cfg(any(
    all(target_os = "linux", not(all(target_env = "musl", target_feature = "crt-static"))),
    target_os = "macos"
))]
mod platform {
    use core::ffi::{c_char, c_int, c_void};

    #[cfg_attr(target_os = "linux", link(name = "dl"))]
    unsafe extern "C" {
        fn dlopen(path: *const c_char, mode: c_int) -> *mut c_void;
        fn dlsym(handle: *mut c_void, name: *const c_char) -> *mut c_void;
        fn dlclose(handle: *mut c_void) -> c_int;
    }

    pub unsafe fn open(path: *const c_char) -> *mut c_void {
        #[cfg(target_os = "macos")]
        const LOCAL: c_int = 4;
        #[cfg(target_os = "linux")]
        const LOCAL: c_int = 0;
        // RTLD_NOW | RTLD_LOCAL: resolve eagerly without exporting symbols.
        unsafe { dlopen(path, 2 | LOCAL) }
    }

    pub unsafe fn symbol(handle: *mut c_void, name: *const c_char) -> *mut c_void {
        unsafe { dlsym(handle, name) }
    }

    pub unsafe fn close(handle: *mut c_void) -> bool {
        unsafe { dlclose(handle) == 0 }
    }
}

#[cfg(windows)]
mod platform {
    use core::ffi::{c_char, c_void};

    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn LoadLibraryExA(path: *const c_char, file: *mut c_void, flags: u32) -> *mut c_void;
        fn LoadLibraryExW(path: *const u16, file: *mut c_void, flags: u32) -> *mut c_void;
        fn GetProcAddress(handle: *mut c_void, name: *const c_char) -> *mut c_void;
        fn FreeLibrary(handle: *mut c_void) -> i32;
    }

    // LOAD_LIBRARY_SEARCH_DEFAULT_DIRS avoids the legacy current-directory search.
    const SEARCH_DEFAULT_DIRS: u32 = 0x1000;

    pub unsafe fn open(path: *const c_char) -> *mut c_void {
        unsafe { LoadLibraryExA(path, core::ptr::null_mut(), SEARCH_DEFAULT_DIRS) }
    }

    pub unsafe fn open_wide(path: *const u16) -> *mut c_void {
        unsafe { LoadLibraryExW(path, core::ptr::null_mut(), SEARCH_DEFAULT_DIRS) }
    }

    pub unsafe fn symbol(handle: *mut c_void, name: *const c_char) -> *mut c_void {
        unsafe { GetProcAddress(handle, name) }
    }

    pub unsafe fn close(handle: *mut c_void) -> bool {
        unsafe { FreeLibrary(handle) != 0 }
    }
}

#[cfg(not(any(
    all(target_os = "linux", not(all(target_env = "musl", target_feature = "crt-static"))),
    target_os = "macos",
    windows
)))]
mod platform {
    use core::ffi::{c_char, c_void};

    pub unsafe fn open(_: *const c_char) -> *mut c_void {
        core::ptr::null_mut()
    }

    pub unsafe fn symbol(_: *mut c_void, _: *const c_char) -> *mut c_void {
        core::ptr::null_mut()
    }

    pub unsafe fn close(_: *mut c_void) -> bool {
        false
    }
}
