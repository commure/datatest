//! Support module for `#[datatest::files(..)]`
use rustc_test::Bencher;
use std::borrow::Borrow;
use std::path::{Path, PathBuf};

/// Used internally for `#[datatest::files(..)]` tests to distinguish regular tests versus benchmark
/// tests.
#[doc(hidden)]
pub enum FilesTestFn {
    TestFn(fn(&[PathBuf])),
    BenchFn(fn(&mut Bencher, &[PathBuf])),
}

/// Descriptor used internally for `#[datatest::files(..)]` tests.
#[doc(hidden)]
pub struct FilesTestDesc {
    pub name: &'static str,
    pub ignore: bool,
    pub root: &'static str,
    pub params: &'static [&'static str],
    pub pattern: usize,
    pub ignorefn: Option<fn(&Path) -> bool>,
    pub testfn: FilesTestFn,
}

/// Trait defining conversion into a function argument. We use it to convert discovered paths
/// to test data (captured as `&Path`) into what is expected by the function.
///
/// Why so complex? We need a way to convert an argument received as an element of slice  `&[PathBuf]`
/// and convert it into what the function expects.
///
/// The difficulty here is that for owned arguments we can create value and just pass it down to the
/// function. However, for arguments taking slices, we need to store value somewhere on the stack
/// and pass a reference.
///
/// Theoretically, our proc macro could generate a different piece of code to handle that, but
/// to avoid the complexity in proc macro, it always generates code in the form of:
///
/// ```ignore
/// TakeArg::take(&mut <#ty as DeriveArg>::derive(&paths_arg[#idx]))
/// ```
///
/// (`#ty` is the type of the function argument).
///
/// [`DeriveArg`] is responsible for converting `&PathBuf` into an "owned" form of `#ty`. For example,
/// owned form for both `&str` and `String` will be `String` and conversion will be reading the file
/// at given path.
///
/// [`TakeArg`] is responsible for deriving argument type from the mutable reference to the
/// `TakeArg::Derived`. The reason mutable referenc is used is because, again, the generated code
/// is the same, so we cannot take `self` in one case and `&self` in other case. Instead, we take
/// `&mut self` in `TakeArg` and either convert it to the shared reference or replace the value
/// with "sentinel" (empty value) and return taken out value as result.
///
/// We pre-define few conversions:
///
/// 1. `&Path` -> `&str`, `String` (reads file content into a string)
/// 2. `&Path` -> `&[u8]`, `Vec<u8>` (reads file content into a byte buffer)
/// 3. `&Path` -> `&Path` (gives path "as is")
///
/// Conversion is two step: first, we need to derive some value. Second, we need to either borrow
/// from that value (if we need `&str`, for example) or take from that value (if we need `String`,
/// for example) to pass an argument to the function.
#[doc(hidden)]
pub trait DeriveArg<'a>: 'a + Sized {
    /// Type to hold temporary value when going from `&Path` into target type.
    /// Necessary for conversions from `&Path` to `&str`,
    type Derived: TakeArg<'a, Self>;
    fn derive(path: &'a Path) -> Self::Derived;
}

// Strings

impl<'a> DeriveArg<'a> for &'a str {
    type Derived = String;
    fn derive(path: &'a Path) -> String {
        crate::read_to_string(path)
    }
}

impl<'a> DeriveArg<'a> for String {
    type Derived = String;
    fn derive(path: &'a Path) -> String {
        crate::read_to_string(path)
    }
}

// Byte slices

impl<'a> DeriveArg<'a> for &'a [u8] {
    type Derived = Vec<u8>;
    fn derive(path: &'a Path) -> Vec<u8> {
        crate::read_to_end(path)
    }
}

impl<'a> DeriveArg<'a> for Vec<u8> {
    type Derived = Vec<u8>;
    fn derive(path: &'a Path) -> Vec<u8> {
        crate::read_to_end(path)
    }
}

// Paths

impl<'a> DeriveArg<'a> for &'a Path {
    type Derived = &'a Path;

    fn derive(path: &'a Path) -> &'a Path {
        path
    }
}

#[doc(hidden)]
pub trait TakeArg<'a, T: 'a> {
    fn take(&'a mut self) -> T;
}

// If we can borrow, we are good!

impl<'a, T, Q> TakeArg<'a, &'a Q> for T
where
    Q: ?Sized,
    T: Borrow<Q>,
{
    fn take(&'a mut self) -> &'a Q {
        T::borrow(self)
    }
}

// Otherwise, take the value & leave the empty one. This is guaranteed (by our proc macro) to
// be only called once.

impl<'a> TakeArg<'a, String> for String {
    fn take(&mut self) -> String {
        std::mem::replace(self, String::new())
    }
}

impl<'a> TakeArg<'a, Vec<u8>> for Vec<u8> {
    fn take(&mut self) -> Vec<u8> {
        std::mem::replace(self, Vec::new())
    }
}
