// Native Windows contract tests derived from the two original consumers.
// Fixtures set and inspect security using independent Win32 calls. They are
// compiled and executed only on Windows; a Linux cfg pass is not their result.

mod object_only {
    use super::super::*;
    use std::{
        fs::{self, File, OpenOptions},
        io::{Read, Seek, SeekFrom, Write},
        os::windows::fs::OpenOptionsExt,
        path::PathBuf,
        sync::atomic::{AtomicU64, Ordering},
        time::{SystemTime, UNIX_EPOCH},
    };

    #[link(name = "advapi32")]
    unsafe extern "system" {
        fn ConvertSecurityDescriptorToStringSecurityDescriptorW(
            descriptor: *const c_void,
            revision: u32,
            information: u32,
            text: *mut *mut u16,
            length: *mut u32,
        ) -> i32;
    }

    fn protect_created(file: &File) -> Result<(), Error> {
        super::super::protect_created(file, DirectoryInheritance::ObjectOnly)
    }

    static SERIAL: AtomicU64 = AtomicU64::new(0);
    struct Temporary(PathBuf);
    impl Temporary {
        fn new() -> (Self, File) {
            let stamp = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos();
            let root = std::env::temp_dir().join(format!(
                "zsys-object-only-security-{}-{stamp}-{}",
                std::process::id(),
                SERIAL.fetch_add(1, Ordering::Relaxed)
            ));
            fs::create_dir(&root).unwrap();
            let file = OpenOptions::new()
                .read(true)
                .write(true)
                .create_new(true)
                .access_mode(0x80000000 | 0x40000000 | 0x00040000 | 0x00080000)
                .share_mode(0x1 | 0x2)
                .open(root.join("private"))
                .unwrap();
            (Self(root), file)
        }
    }
    impl Drop for Temporary {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn current_sid_text() -> String {
        let user = private_user().unwrap();
        let sid = unsafe { *(user.as_ptr().cast::<*mut c_void>()) };
        let mut text = ptr::null_mut();
        // SAFETY: user owns the TOKEN_USER storage and Local releases the native string.
        assert_ne!(unsafe { ConvertSidToStringSidW(sid, &mut text) }, 0);
        let memory = Local(text.cast());
        let mut length = 0;
        while length < 256 && unsafe { *text.add(length) } != 0 {
            length += 1;
        }
        assert!(length < 256);
        let value = String::from_utf16(unsafe { std::slice::from_raw_parts(text, length) }).unwrap();
        drop(memory);
        value
    }

    fn replace_test_dacl(file: &File, foreign: bool, protected: bool) {
        replace_test_dacl_flags(file, foreign, protected, "");
    }

    fn replace_test_dacl_flags(file: &File, foreign: bool, protected: bool, flags: &str) {
        let sid = current_sid_text();
        let sddl = format!(
            "D:{}(A;{flags};FA;;;{sid})(A;{flags};FA;;;SY){}",
            if protected { "P" } else { "" },
            if foreign { "(A;;FR;;;WD)" } else { "" }
        )
        .encode_utf16()
        .chain([0])
        .collect::<Vec<_>>();
        let mut descriptor = ptr::null_mut();
        // SAFETY: these tests only mutate their exclusively created temporary file.
        unsafe {
            assert_ne!(
                ConvertStringSecurityDescriptorToSecurityDescriptorW(
                    sddl.as_ptr(),
                    1,
                    &mut descriptor,
                    ptr::null_mut()
                ),
                0
            );
            let descriptor = Local(descriptor);
            let mut present = 0;
            let mut defaulted = 0;
            let mut acl = ptr::null_mut();
            assert_ne!(GetSecurityDescriptorDacl(descriptor.0, &mut present, &mut acl, &mut defaulted), 0);
            assert_ne!(present, 0);
            assert!(!acl.is_null());
            assert_eq!(
                SetSecurityInfo(
                    file.as_raw_handle(),
                    1,
                    4 | if protected { 0x80000000 } else { 0x20000000 },
                    ptr::null_mut(),
                    ptr::null_mut(),
                    acl,
                    ptr::null_mut()
                ),
                0
            );
        }
    }

    fn security_snapshot(file: &File) -> Vec<u16> {
        let mut descriptor = ptr::null_mut();
        let mut text = ptr::null_mut();
        let mut length = 0;
        // SAFETY: both native allocations are guarded until their contents are copied.
        unsafe {
            assert_eq!(
                GetSecurityInfo(
                    file.as_raw_handle(),
                    1,
                    1 | 4,
                    ptr::null_mut(),
                    ptr::null_mut(),
                    ptr::null_mut(),
                    ptr::null_mut(),
                    &mut descriptor
                ),
                0
            );
            let descriptor = Local(descriptor);
            assert_ne!(
                ConvertSecurityDescriptorToStringSecurityDescriptorW(descriptor.0, 1, 1 | 4, &mut text, &mut length),
                0
            );
            let text_memory = Local(text.cast());
            assert!((1..=65536).contains(&length));
            let copy = std::slice::from_raw_parts(text, length as usize).to_vec();
            drop(text_memory);
            copy
        }
    }

    #[test]
    fn creation_protects_owner_and_current_account_system_access_before_data() {
        let (_temporary, mut file) = Temporary::new();
        protect_created(&file).unwrap();
        check_private(&file).unwrap();
        file.write_all(b"private payload").unwrap();
        file.seek(SeekFrom::Start(0)).unwrap();
        let mut bytes = Vec::new();
        file.read_to_end(&mut bytes).unwrap();
        assert_eq!(bytes, b"private payload");
    }

    #[test]
    fn existing_foreign_read_access_is_rejected_without_rewriting_security_or_data() {
        let (_temporary, mut file) = Temporary::new();
        protect_created(&file).unwrap();
        file.write_all(b"existing payload").unwrap();
        replace_test_dacl(&file, true, true);
        let before = security_snapshot(&file);
        assert!(check_private(&file).is_err(), "Everyone read access must be rejected");
        assert!(before == security_snapshot(&file), "read-only validation changed the existing ACL");
        file.seek(SeekFrom::Start(0)).unwrap();
        let mut bytes = Vec::new();
        file.read_to_end(&mut bytes).unwrap();
        assert_eq!(bytes, b"existing payload");
    }

    #[test]
    fn existing_unprotected_dacl_is_rejected_without_repair() {
        let (_temporary, file) = Temporary::new();
        protect_created(&file).unwrap();
        replace_test_dacl(&file, false, false);
        let before = security_snapshot(&file);
        assert!(check_private(&file).is_err(), "inheritable security must be rejected");
        assert!(before == security_snapshot(&file), "validation must not protect an existing DACL");
    }

    #[test]
    fn existing_directories_are_read_without_changing_their_private_acl() {
        let (temporary, file) = Temporary::new();
        drop(file);
        let path = temporary.0.join("transactions");
        fs::create_dir(&path).unwrap();
        let directory = OpenOptions::new()
            .access_mode(0x0002_0000 | 0x0004_0000 | 0x0008_0000)
            .share_mode(0x1 | 0x2)
            .custom_flags(0x0200_0000 | 0x0020_0000)
            .open(&path)
            .unwrap();
        protect_created(&directory).unwrap();
        // Match the GUI's existing-directory reader: READ_CONTROL only, no repair.
        let reader = OpenOptions::new()
            .access_mode(0x0002_0000)
            .share_mode(0x1 | 0x2)
            .custom_flags(0x0200_0000 | 0x0020_0000)
            .open(&path)
            .unwrap();
        for flags in ["", "OICI"] {
            replace_test_dacl_flags(&directory, false, true, flags);
            let before = security_snapshot(&reader);
            check_private(&reader).unwrap();
            assert_eq!(security_snapshot(&reader), before);
        }
        for flags in ["OI", "CI", "OICINP", "OICIIO"] {
            replace_test_dacl_flags(&directory, false, true, flags);
            let before = security_snapshot(&reader);
            assert!(check_private(&reader).is_err(), "accepted {flags}");
            assert_eq!(security_snapshot(&reader), before);
        }
        replace_test_dacl_flags(&directory, true, true, "OICI");
        let before = security_snapshot(&reader);
        assert!(check_private(&reader).is_err());
        assert_eq!(security_snapshot(&reader), before);
        replace_test_dacl_flags(&directory, false, true, "OICI");
    }
}

mod directory_children {
    use super::super::*;
    use std::{fs, fs::File, os::windows::fs::OpenOptionsExt, path::PathBuf};

    fn protect(file: &File) -> Result<(), Error> {
        super::super::protect(file, DirectoryInheritance::FilesAndDirectories)
    }

    fn protect_created(file: &File) -> Result<(), Error> {
        super::super::protect_created(file, DirectoryInheritance::FilesAndDirectories)
    }

    static SERIAL: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

    struct Directory {
        path: PathBuf,
        file: File,
    }

    impl Directory {
        fn new() -> Self {
            let stamp = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos();
            let serial = SERIAL.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            let path = std::env::temp_dir().join(format!("zsys-directory-acl-{}-{stamp}-{serial}", std::process::id()));
            fs::create_dir(&path).unwrap();
            let file = File::options()
                .access_mode(0x0002_0000 | 0x0004_0000 | 0x0008_0000)
                .custom_flags(0x0200_0000)
                .open(&path)
                .unwrap();
            protect_created(&file).unwrap();
            Self { path, file }
        }
    }

    impl Drop for Directory {
        fn drop(&mut self) {
            let _ = protect(&self.file);
            let _ = fs::remove_dir_all(&self.path);
        }
    }

    // Seed known native ACLs independently of protect(), including the old
    // non-inheriting format and deliberately invalid inspection inputs.
    fn set_acl(file: &File, flags: &str, second: &str) {
        set_acl_entries(file, flags, flags, second, "FA");
    }

    fn set_acl_entries(file: &File, user_flags: &str, system_flags: &str, second: &str, rights: &str) {
        let user = private_user().unwrap();
        // SAFETY: token/descriptor allocations stay alive through the Win32 calls.
        unsafe {
            let mut raw = ptr::null_mut();
            assert_ne!(ConvertSidToStringSidW(*(user.as_ptr().cast::<*mut c_void>()), &mut raw), 0);
            let memory = Local(raw.cast());
            let mut length = 0;
            while length < 256 && *raw.add(length) != 0 {
                length += 1;
            }
            assert!(length < 256);
            let sid = String::from_utf16(std::slice::from_raw_parts(raw, length)).unwrap();
            drop(memory);
            let text = format!("D:P(A;{user_flags};{rights};;;{sid})(A;{system_flags};{rights};;;{second})")
                .encode_utf16()
                .chain([0])
                .collect::<Vec<_>>();
            let mut descriptor = ptr::null_mut();
            assert_ne!(
                ConvertStringSecurityDescriptorToSecurityDescriptorW(
                    text.as_ptr(),
                    1,
                    &mut descriptor,
                    ptr::null_mut(),
                ),
                0
            );
            let descriptor = Local(descriptor);
            let mut present = 0;
            let mut defaulted = 0;
            let mut acl = ptr::null_mut();
            assert_ne!(GetSecurityDescriptorDacl(descriptor.0, &mut present, &mut acl, &mut defaulted), 0);
            assert_ne!(present, 0);
            assert!(!acl.is_null());
            assert_eq!(
                SetSecurityInfo(
                    file.as_raw_handle(),
                    1,
                    0x80000004,
                    ptr::null_mut(),
                    ptr::null_mut(),
                    acl,
                    ptr::null_mut(),
                ),
                0
            );
        }
    }

    // Validate inherited children's effective grants without weakening the
    // production checker, which intentionally requires a protected, explicit ACL.
    fn assert_private_grants(file: &File, flags: u8) {
        #[repr(C)]
        struct AclSize {
            count: u32,
            used: u32,
            free: u32,
        }
        let user = private_user().unwrap();
        // SAFETY: GetSecurityInfo owns the descriptor; all ACEs stay inside it.
        unsafe {
            let mut acl = ptr::null_mut();
            let mut descriptor = ptr::null_mut();
            assert_eq!(
                GetSecurityInfo(
                    file.as_raw_handle(),
                    1,
                    4,
                    ptr::null_mut(),
                    ptr::null_mut(),
                    &mut acl,
                    ptr::null_mut(),
                    &mut descriptor,
                ),
                0
            );
            assert!(!descriptor.is_null() && !acl.is_null());
            let _descriptor = Local(descriptor);
            let mut size = AclSize { count: 0, used: 0, free: 0 };
            assert_ne!(
                GetAclInformation(acl, (&mut size as *mut AclSize).cast(), std::mem::size_of::<AclSize>() as u32, 2,),
                0
            );
            assert_eq!(size.count, 2);
            let mut system = [0usize; 16];
            let mut system_size = std::mem::size_of_val(&system) as u32;
            assert_ne!(CreateWellKnownSid(22, ptr::null(), system.as_mut_ptr().cast(), &mut system_size), 0);
            let user_sid = *(user.as_ptr().cast::<*mut c_void>());
            let mut found_user = false;
            let mut found_system = false;
            for index in 0..size.count {
                let mut ace = ptr::null_mut();
                assert_ne!(GetAce(acl, index, &mut ace), 0);
                assert!(!ace.is_null());
                let bytes = ace.cast::<u8>();
                assert_eq!(*bytes, 0); // ACCESS_ALLOWED_ACE_TYPE
                assert_eq!(*bytes.add(1), flags);
                assert_eq!(ptr::read_unaligned(bytes.add(4).cast::<u32>()), 0x001f_01ff);
                let sid = bytes.add(8).cast::<c_void>();
                let is_user = EqualSid(sid, user_sid) != 0;
                let is_system = EqualSid(sid, system.as_ptr().cast()) != 0;
                assert!(is_user || is_system);
                found_user |= is_user;
                found_system |= is_system;
            }
            assert!(found_user && found_system);
        }
    }

    #[test]
    fn directory_reprotection_preserves_inherited_children_and_new_private_files() {
        let directory = Directory::new();
        set_acl(&directory.file, "OICI", "SY");
        // Match desktop::private_dir and the installer's read-only check_dir.
        let desktop = File::options()
            .access_mode(0x0002_0000 | 0x0004_0000)
            .custom_flags(0x0200_0000 | 0x0020_0000)
            .open(&directory.path)
            .unwrap();
        let read_only = File::options()
            .access_mode(0x0002_0000)
            .custom_flags(0x0200_0000 | 0x0020_0000)
            .open(&directory.path)
            .unwrap();
        let existing = directory.path.join("existing.json");
        let child_dir = directory.path.join("child");
        fs::create_dir(&child_dir).unwrap();
        let nested = child_dir.join("existing.json");
        fs::write(&existing, b"old").unwrap();
        fs::write(&nested, b"nested").unwrap();
        assert_private_grants(&File::open(&existing).unwrap(), 0x10);
        assert_private_grants(&File::open(&nested).unwrap(), 0x10);
        for _ in 0..2 {
            protect(&desktop).unwrap();
            check_private(&read_only).unwrap();
            assert_eq!(fs::read(&existing).unwrap(), b"old");
            assert_eq!(fs::read(&nested).unwrap(), b"nested");
            fs::write(&existing, b"old").unwrap();
            fs::write(&nested, b"nested").unwrap();
        }
        let inherited_directory =
            File::options().access_mode(0x0002_0000).custom_flags(0x0200_0000 | 0x0020_0000).open(&child_dir).unwrap();
        assert_private_grants(&inherited_directory, 0x13);
        assert!(check_private(&inherited_directory).is_err());
        for parent in [&directory.path, &child_dir] {
            let new = parent.join("new.json");
            fs::write(&new, b"new").unwrap();
            assert_eq!(fs::read(&new).unwrap(), b"new");
            let file = File::open(&new).unwrap();
            assert_private_grants(&file, 0x10);
            assert!(check_private(&file).is_err());
        }
    }

    #[test]
    fn directory_inspection_accepts_legacy_acl_and_rejects_other_flags_and_subjects() {
        let directory = Directory::new();
        for flags in ["", "OICI"] {
            set_acl(&directory.file, flags, "SY");
            check_private(&directory.file).unwrap();
        }
        for flags in ["OI", "CI", "OICINP", "OICIIO"] {
            set_acl(&directory.file, flags, "SY");
            assert!(check_private(&directory.file).is_err(), "accepted {flags}");
        }
        // Protected assignment makes supplied ID ACEs explicit. Check the stored
        // ACL; actual inherited child ACEs are rejected in the propagation test.
        set_acl(&directory.file, "ID", "SY");
        assert_private_grants(&directory.file, 0);
        check_private(&directory.file).unwrap();
        set_acl(&directory.file, "OICI", "WD");
        assert!(check_private(&directory.file).is_err());
    }

    #[test]
    fn regular_file_protection_keeps_non_inheriting_explicit_private_acl() {
        let directory = Directory::new();
        let path = directory.path.join("secret.json");
        let file = File::options()
            .read(true)
            .write(true)
            .create_new(true)
            .access_mode(0x8000_0000 | 0x4000_0000 | 0x0004_0000 | 0x0008_0000)
            .open(&path)
            .unwrap();
        protect_created(&file).unwrap();
        assert_private_grants(&file, 0);
        check_private(&file).unwrap();
        set_acl(&file, "OICI", "SY");
        // Noncontainer assignment may normalize the requested propagation flags.
        // Require an explicit readback of the expected ACL, not input SDDL alone.
        assert_private_grants(&file, 0);
        check_private(&file).unwrap();
        protect(&file).unwrap();
        check_private(&file).unwrap();
        assert_private_grants(&file, 0);
        fs::write(&path, b"private").unwrap();
        assert_eq!(fs::read(&path).unwrap(), b"private");
    }
    #[test]
    fn inspection_keeps_per_entry_inheritance_and_access_mask_policy() {
        let directory = Directory::new();
        for (user_flags, system_flags) in [("", "OICI"), ("OICI", "")] {
            for rights in ["FR", "FW", "0x00000000"] {
                set_acl_entries(&directory.file, user_flags, system_flags, "SY", rights);
                check_private(&directory.file).unwrap();
            }
        }
    }
}
