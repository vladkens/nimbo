// Copyright (c) vladkens | MIT License | https://github.com/vladkens/nimbo

//! System libraries loaded at run time (`dlopen`), as Sublime does: no build-time C and no
//! binding crates. Each library's few functions are declared by hand with [fns], a struct of
//! function pointers filled by `dlsym`.

use std::ffi::{CStr, c_char, c_int, c_void};

// glibc (libc.so.6 since 2.34; Rust's std links it), <dlfcn.h>.
unsafe extern "C" {
  fn dlopen(name: *const c_char, flags: c_int) -> *mut c_void;
  fn dlsym(handle: *mut c_void, name: *const c_char) -> *mut c_void;
}

const RTLD_NOW: c_int = 2;

/// An open library, never closed: its functions live as long as the process.
pub struct Lib(*mut c_void);

impl Lib {
  /// The first of `names` (sonames, `c"libfreetype.so.6"`) that loads.
  pub fn open(names: &[&CStr]) -> Option<Self> {
    let h = names.iter().map(|n| unsafe { dlopen(n.as_ptr(), RTLD_NOW) }).find(|h| !h.is_null())?;
    Some(Self(h))
  }

  /// The address of `name` (NUL-terminated), as a function pointer type `T`.
  ///
  /// # Safety
  /// `T` must be the symbol's real signature, a pointer-sized `extern "C" fn`.
  pub unsafe fn sym<T: Copy>(&self, name: &str) -> Option<T> {
    let name = CStr::from_bytes_with_nul(name.as_bytes()).ok()?;
    let p = unsafe { dlsym(self.0, name.as_ptr()) };
    // SAFETY: the caller names the symbol's type; a fn pointer is a non-null pointer.
    (!p.is_null()).then(|| unsafe { std::mem::transmute_copy::<*mut c_void, T>(&p) })
  }
}

/// Declares a struct of a library's functions and a `load(&Lib) -> Option<Self>` that finds
/// them all by name.
macro_rules! fns {
  ($name:ident { $(fn $f:ident($($a:ident: $t:ty),* $(,)?) $(-> $r:ty)?;)* }) => {
    #[allow(non_snake_case)]
    struct $name {
      $($f: unsafe extern "C" fn($($t),*) $(-> $r)?,)*
    }

    impl $name {
      fn load(lib: &$crate::platform::linux::dl::Lib) -> Option<Self> {
        // SAFETY: each field's type is the C declaration of the symbol of its name.
        Some(Self { $($f: unsafe { lib.sym(concat!(stringify!($f), "\0")) }?,)* })
      }
    }
  };
}

pub(crate) use fns;
