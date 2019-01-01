use crate::data::DataTestDesc;
use crate::files::FilesTestDesc;
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
        .map(|entry| entry.unwrap())
        .filter(|entry| {
            entry.file_type().is_file()
                && entry
                    .file_name()
                    .to_str()
                    .map_or(false, |s| !s.starts_with('.')) // Skip hidden files
        })
        .map(|entry| entry.path().to_path_buf())
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
            let testfn = desc.testfn;
            let ignore = desc.ignore
                || desc
                    .ignorefn
                    .map_or(false, |ignore_func| ignore_func(&path));

            // Generate a standard test descriptor
            let desc = TestDescAndFn {
                desc: TestDesc {
                    name: TestName::DynTestName(test_name),
                    ignore,
                    should_panic: ShouldPanic::No,
                    allow_fail: false,
                },
                testfn: TestFn::DynTestFn(Box::new(move || testfn(&paths))),
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

    let input = crate::read_to_string(Path::new(desc.root));
    let cases = (desc.describefn)(&input);
    for case in cases {
        // FIXME: use name provided in `case`...

        let testfn = case.testfn;

        let case_name = if let Some(n) = case.name {
            format!("{}::{}::{} (line {})", prefix_name, desc.root, n, case.line)
        } else {
            format!("{}::{}::line {}", prefix_name, desc.root, case.line)
        };
        // Generate a standard test descriptor
        let desc = TestDescAndFn {
            desc: TestDesc {
                name: TestName::DynTestName(case_name),
                ignore: desc.ignore,
                should_panic: ShouldPanic::No,
                allow_fail: false,
            },
            testfn: TestFn::DynTestFn(testfn),
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
#[doc(hidden)]
pub fn runner(tests: &[&TestDescriptor]) {
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
