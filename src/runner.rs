use crate::data::{DataTestDesc, DataTestFn};
use crate::files::{FilesTestDesc, FilesTestFn};
use crate::test::{ShouldPanic, TestDesc, TestDescAndFn, TestFn, TestName};
use std::path::{Path, PathBuf};

fn derive_test_name(root: &Path, path: &Path, test_name: &str) -> String {
    let relative = path.strip_prefix(root).unwrap_or_else(|_| {
        panic!(
            "failed to strip prefix '{}' from path '{}'",
            root.display(),
            path.display()
        )
    });
    let mut test_name = real_name(test_name).to_string();
    test_name += "::";
    test_name += &relative.to_string_lossy();
    test_name
}

/// When compiling tests, Rust compiler collects all items marked with `#[test_case]` and passes
/// references to them to the test runner in a slice (like `&[&test_a, &test_b, &test_c]`). Since
/// we need a different descriptor for our data-driven tests than the standard one, we have two
/// options here:
///
/// 1. override standard `#[test]` handling and generate our own descriptor for regular tests, so
/// our runner can accept the descriptor of our own type.
/// 2. accept a trait object in a runner and make both standard descriptor and our custom descriptors
/// to implement that trait and use dynamic dispatch to dispatch on the descriptor type.
///
/// We go with the second approach as it allows us to keep standard `#[test]` processing.
#[doc(hidden)]
pub trait TestDescriptor {
    fn as_datatest_desc(&self) -> DatatestTestDesc;
}

impl TestDescriptor for TestDescAndFn {
    fn as_datatest_desc(&self) -> DatatestTestDesc {
        DatatestTestDesc::Test(self)
    }
}

impl TestDescriptor for FilesTestDesc {
    fn as_datatest_desc(&self) -> DatatestTestDesc {
        DatatestTestDesc::FilesTest(self)
    }
}

impl TestDescriptor for DataTestDesc {
    fn as_datatest_desc(&self) -> DatatestTestDesc {
        DatatestTestDesc::DataTest(self)
    }
}

#[doc(hidden)]
pub enum DatatestTestDesc<'a> {
    Test(&'a TestDescAndFn),
    FilesTest(&'a FilesTestDesc),
    DataTest(&'a DataTestDesc),
}

/// Helper function to iterate through all the files in the given directory, skipping hidden files,
/// and return an iterator of their paths.
fn iterate_directory(path: &Path) -> impl Iterator<Item = PathBuf> {
    walkdir::WalkDir::new(path)
        .into_iter()
        .map(Result::unwrap)
        .filter(|entry| {
            entry.file_type().is_file()
                && entry
                    .file_name()
                    .to_str()
                    .map_or(false, |s| !s.starts_with('.')) // Skip hidden files
        })
        .map(|entry| entry.path().to_path_buf())
}

struct FilesBenchFn(fn(&mut test::Bencher, &[PathBuf]), Vec<PathBuf>);

impl test::TDynBenchFn for FilesBenchFn {
    fn run(&self, harness: &mut test::Bencher) {
        (self.0)(harness, &self.1)
    }
}

/// Generate standard test descriptors ([`test::TestDescAndFn`]) from the descriptor of
/// `#[datatest::files(..)]`.
///
/// Scans all files in a given directory, finds matching ones and generates a test descriptor for
/// each of them.
fn render_files_test(desc: &FilesTestDesc, rendered: &mut Vec<TestDescAndFn>) {
    let root = Path::new(desc.root).to_path_buf();

    let pattern = desc.params[desc.pattern];
    let re = regex::Regex::new(pattern)
        .unwrap_or_else(|_| panic!("invalid regular expression: '{}'", pattern));

    let mut found = false;
    for path in iterate_directory(&root) {
        let input_path = path.to_string_lossy();
        if re.is_match(&input_path) {
            // Generate list of paths to pass to the test function. We generate a `PathBuf` for each
            // argument of the test function and pass them to the trampoline function in a slice.
            // See `datatest-derive` proc macro sources for more details.
            let mut paths = Vec::with_capacity(desc.params.len());

            let path_str = path.to_string_lossy();
            for (idx, param) in desc.params.iter().enumerate() {
                if idx == desc.pattern {
                    // Pattern path
                    paths.push(path.to_path_buf());
                } else {
                    let rendered_path = re.replace_all(&path_str, *param);
                    let rendered_path = Path::new(rendered_path.as_ref()).to_path_buf();
                    paths.push(rendered_path);
                }
            }

            let test_name = derive_test_name(&root, &path, desc.name);
            let ignore = desc.ignore
                || desc
                    .ignorefn
                    .map_or(false, |ignore_func| ignore_func(&path));

            let testfn = match desc.testfn {
                FilesTestFn::TestFn(testfn) => TestFn::DynTestFn(Box::new(move || testfn(&paths))),
                FilesTestFn::BenchFn(benchfn) => {
                    TestFn::DynBenchFn(Box::new(FilesBenchFn(benchfn, paths)))
                }
            };

            // Generate a standard test descriptor
            let desc = TestDescAndFn {
                desc: TestDesc {
                    name: TestName::DynTestName(test_name),
                    ignore,
                    should_panic: ShouldPanic::No,
                    allow_fail: false,
                },
                testfn,
            };

            rendered.push(desc);
            found = true;
        }
    }

    // We want to avoid silent fails due to typos in regexp!
    if !found {
        panic!(
            "no test cases found for test '{}'. Scanned directory: '{}' with pattern '{}'",
            desc.name, desc.root, pattern,
        );
    }
}

fn render_data_test(desc: &DataTestDesc, rendered: &mut Vec<TestDescAndFn>) {
    let prefix_name = real_name(&desc.name);

    let cases = (desc.describefn)();
    for case in cases {
        // FIXME: use name provided in `case`...

        let case_name = if let Some(n) = case.name {
            format!("{}::{} ({})", prefix_name, n, case.location)
        } else {
            format!("{}::{}", prefix_name, case.location)
        };

        let testfn = match case.case {
            DataTestFn::TestFn(testfn) => TestFn::DynTestFn(Box::new(|| testfn())),
            DataTestFn::BenchFn(benchfn) => TestFn::DynBenchFn(benchfn),
        };

        // Generate a standard test descriptor
        let desc = TestDescAndFn {
            desc: TestDesc {
                name: TestName::DynTestName(case_name),
                ignore: desc.ignore,
                should_panic: ShouldPanic::No,
                allow_fail: false,
            },
            testfn,
        };

        rendered.push(desc);
    }
}

/// We need to build our own slice of test descriptors to pass to `test::test_main`. We cannot
/// clone `TestFn`, so we do it via matching on variants. Not sure how to handle `Dynamic*` variants,
/// but we seem not to get them here anyway?.
fn clone_testfn(testfn: &TestFn) -> TestFn {
    match testfn {
        TestFn::StaticTestFn(func) => TestFn::StaticTestFn(*func),
        TestFn::StaticBenchFn(bench) => TestFn::StaticBenchFn(*bench),
        _ => unimplemented!("only static functions are supported"),
    }
}

/// Strip crate name. We use `module_path!` macro to generate this name, which includes crate name.
/// However, standard test library does not include crate name into a test name.
fn real_name(name: &str) -> &str {
    match name.find("::") {
        Some(pos) => &name[pos + 2..],
        None => name,
    }
}

/// When we have "--exact" option and test filter is exactly our "parent" test (which is nota a real
/// test, but a template for children tests), we adjust options a bit to run all children tests
/// instead.
fn adjust_for_test_name(opts: &mut crate::test::TestOpts, name: &str) {
    let real_test_name = real_name(name);
    if opts.filter_exact && opts.filter.as_ref().map_or(false, |s| s == real_test_name) {
        opts.filter_exact = false;
        opts.filter = Some(format!("{}::", real_test_name));
    }
}

/// Custom test runner. Expands test definitions given in the format our test framework understands
/// ([DataTestDesc]) into definitions understood by Rust test framework ([TestDescAndFn] structs).
/// For regular tests, mapping is one-to-one, for our data driven tests, we generate as many
/// descriptors as test cases we discovered.
///
/// # Notes
/// So, how does it work? We use a nightly-only feature of [custom_test_frameworks] that allows you
/// to annotate arbitrary function, const or static with `#[test_case]`. Attribute. Then, Rust
/// compiler would transform the code to pass all the discovered test cases as one big slice to the
/// test runner.
///
/// However, we also want to support standard `#[test]` without disrupting them as much as possible.
/// Internally, compiler would also desugar them to the `#[test_case]` attribute, but the type of
/// the descriptor struct would be a predefined type of `test::TestDescAndFn`. This type, however,
/// cannot represent all the additional information we need for our tests.
///
/// So we do a little trick here: we rely on the fact that compiler generates code exactly like in
/// the following snippet:
///
/// ```ignore
/// test::test_main_static(&[&__test_reexports::some::test1, &__test_reexports::some::test2])
/// ```
///
/// Then, we implement `TestDescriptor` trait for the standard test descriptor struct, which would
/// generate trait objects for these structs and pass a trait object instead. We do the same for
/// our structs and our trait object allows us to return the reference wrapped into an enum
/// distinguishing between three different test variants (standard tests, "files" tests and "data"
/// tests).
///
/// [custom_test_frameworks]: https://github.com/rust-lang/rust/blob/master/src/doc/unstable-book/src/language-features/custom-test-frameworks.md
/// See <https://blog.jrenner.net/rust/testing/2018/07/19/test-in-2018.html>
#[doc(hidden)]
pub fn runner(tests: &[&dyn TestDescriptor]) {
    let args = std::env::args().collect::<Vec<_>>();
    let mut opts = match crate::test::parse_opts(&args) {
        Some(Ok(o)) => o,
        Some(Err(msg)) => panic!("{:?}", msg),
        None => return,
    };

    let mut rendered: Vec<TestDescAndFn> = Vec::new();
    for input in tests.iter() {
        match input.as_datatest_desc() {
            DatatestTestDesc::Test(test) => {
                // Make a copy as we cannot take ownership
                rendered.push(TestDescAndFn {
                    desc: test.desc.clone(),
                    testfn: clone_testfn(&test.testfn),
                })
            }
            DatatestTestDesc::FilesTest(files) => {
                render_files_test(files, &mut rendered);
                adjust_for_test_name(&mut opts, &files.name);
            }
            DatatestTestDesc::DataTest(data) => {
                render_data_test(data, &mut rendered);
                adjust_for_test_name(&mut opts, &data.name);
            }
        }
    }

    // Run tests via standard runner!
    match crate::test::run_tests_console(&opts, rendered) {
        Ok(true) => {}
        Ok(false) => panic!("Some tests failed"),
        Err(e) => panic!("io error when running tests: {:?}", e),
    }
}

#[doc(hidden)]
pub fn assert_test_result<T: std::process::Termination>(result: T) {
    let code = result.report();
    assert_eq!(
        code, 0,
        "the test returned a termination value with a non-zero status code ({}) \
         which indicates a failure",
        code
    );
}
